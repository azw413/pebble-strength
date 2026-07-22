use axum::extract::{FromRequestParts, Query, State};
use axum::http::request::Parts;
use axum::response::Redirect;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use chrono::{Duration, Utc};
use diesel::prelude::*;
use rand::RngCore;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::error::AppError;
use crate::models::User;
use crate::schema::{users, web_sessions};
use crate::{db, AppState};

const SESSION_COOKIE: &str = "sid";
const STATE_COOKIE: &str = "oauth_state";
const SESSION_DAYS: i64 = 90;

pub fn random_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

pub fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn session_cookie(state: &AppState, token: String) -> Cookie<'static> {
    let mut c = Cookie::new(SESSION_COOKIE, token);
    c.set_path("/");
    c.set_http_only(true);
    c.set_same_site(SameSite::Lax);
    c.set_secure(state.cfg.base_url.starts_with("https://"));
    c
}

pub(crate) fn upsert_user(
    conn: &mut SqliteConnection,
    sub: &str,
    email: &str,
    name: &str,
) -> Result<User, AppError> {
    if let Some(user) = users::table
        .filter(users::google_sub.eq(sub))
        .first::<User>(conn)
        .optional()?
    {
        return Ok(user);
    }
    let user = diesel::insert_into(users::table)
        .values((
            users::google_sub.eq(sub),
            users::email.eq(email),
            users::display_name.eq(name),
        ))
        .get_result::<User>(conn)?;
    Ok(user)
}

fn create_session(conn: &mut SqliteConnection, user_id: i32) -> Result<String, AppError> {
    let token = random_token();
    let expires = (Utc::now() + Duration::days(SESSION_DAYS)).naive_utc();
    diesel::insert_into(web_sessions::table)
        .values((
            web_sessions::token_hash.eq(hash_token(&token)),
            web_sessions::user_id.eq(user_id),
            web_sessions::expires_at.eq(expires),
        ))
        .execute(conn)?;
    Ok(token)
}

/// Extractor: the logged-in user, or a redirect to the landing page.
pub struct CurrentUser(pub User);

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = Redirect;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let jar = CookieJar::from_headers(&parts.headers);
        let token = jar
            .get(SESSION_COOKIE)
            .map(|c| c.value().to_string())
            .ok_or_else(|| Redirect::to("/"))?;
        let hash = hash_token(&token);
        let user = db::run(&state.pool, move |conn| {
            let now = Utc::now().naive_utc();
            Ok(web_sessions::table
                .inner_join(users::table)
                .filter(web_sessions::token_hash.eq(hash))
                .filter(web_sessions::expires_at.gt(now))
                .select(users::all_columns)
                .first::<User>(conn)?)
        })
        .await
        .map_err(|_| Redirect::to("/"))?;
        Ok(CurrentUser(user))
    }
}

/// Extractor: the logged-in user if there is one, otherwise None. Never rejects.
pub struct OptionalUser(pub Option<User>);

impl FromRequestParts<AppState> for OptionalUser {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        Ok(OptionalUser(
            CurrentUser::from_request_parts(parts, state).await.ok().map(|c| c.0),
        ))
    }
}

// ---- Google OIDC ----

pub async fn google_start(State(state): State<AppState>, jar: CookieJar) -> (CookieJar, Redirect) {
    let Some(client_id) = state.cfg.google_client_id.clone() else {
        return (jar, Redirect::to("/"));
    };
    let csrf = random_token();
    let mut c = Cookie::new(STATE_COOKIE, csrf.clone());
    c.set_path("/");
    c.set_http_only(true);
    c.set_same_site(SameSite::Lax);
    let url = format!(
        "https://accounts.google.com/o/oauth2/v2/auth?client_id={client_id}\
         &redirect_uri={}/auth/google/callback&response_type=code\
         &scope=openid%20email%20profile&state={csrf}",
        state.cfg.base_url
    );
    (jar.add(c), Redirect::to(&url))
}

#[derive(Deserialize)]
pub struct CallbackParams {
    code: String,
    state: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
}

#[derive(Deserialize)]
struct UserInfo {
    sub: String,
    email: Option<String>,
    name: Option<String>,
}

pub async fn google_callback(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<CallbackParams>,
) -> Result<(CookieJar, Redirect), AppError> {
    let expected = jar.get(STATE_COOKIE).map(|c| c.value().to_string());
    if expected.as_deref() != Some(params.state.as_str()) {
        return Err(AppError::BadRequest("oauth state mismatch".into()));
    }
    let (Some(client_id), Some(client_secret)) = (
        state.cfg.google_client_id.clone(),
        state.cfg.google_client_secret.clone(),
    ) else {
        return Err(AppError::BadRequest("google sign-in not configured".into()));
    };

    let token: TokenResponse = state
        .http
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("code", params.code.as_str()),
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("redirect_uri", &format!("{}/auth/google/callback", state.cfg.base_url)),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let info: UserInfo = state
        .http
        .get("https://openidconnect.googleapis.com/v1/userinfo")
        .bearer_auth(&token.access_token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let session_token = db::run(&state.pool, move |conn| {
        let user = upsert_user(
            conn,
            &info.sub,
            info.email.as_deref().unwrap_or(""),
            info.name.as_deref().unwrap_or(""),
        )?;
        create_session(conn, user.id)
    })
    .await?;

    let jar = jar
        .remove(Cookie::from(STATE_COOKIE))
        .add(session_cookie(&state, session_token));
    Ok((jar, Redirect::to("/")))
}

// ---- Dev login (DEV_LOGIN=1 only) ----

pub async fn dev_login(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<(CookieJar, Redirect), AppError> {
    if !state.cfg.dev_login {
        return Err(AppError::NotFound);
    }
    let token = db::run(&state.pool, |conn| {
        let user = upsert_user(conn, "dev:local", "dev@localhost", "Dev User")?;
        create_session(conn, user.id)
    })
    .await?;
    Ok((jar.add(session_cookie(&state, token)), Redirect::to("/")))
}

pub async fn logout(State(state): State<AppState>, jar: CookieJar) -> Result<(CookieJar, Redirect), AppError> {
    if let Some(c) = jar.get(SESSION_COOKIE) {
        let hash = hash_token(c.value());
        db::run(&state.pool, move |conn| {
            diesel::delete(web_sessions::table.filter(web_sessions::token_hash.eq(hash)))
                .execute(conn)?;
            Ok(())
        })
        .await?;
    }
    Ok((jar.remove(Cookie::from(SESSION_COOKIE)), Redirect::to("/")))
}
