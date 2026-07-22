mod api;
mod auth;
mod dashboard;
mod db;
mod device;
mod error;
mod models;
mod pack;
mod pages;
mod sample;
mod schema;
mod seed;
mod sessions;
mod workouts;

use std::env;
use std::sync::Arc;

use axum::routing::{get, post, put};
use axum::Router;
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!();

pub struct Config {
    pub base_url: String,
    pub google_client_id: Option<String>,
    pub google_client_secret: Option<String>,
    pub dev_login: bool,
}

#[derive(Clone)]
pub struct AppState {
    pub pool: db::Pool,
    pub cfg: Arc<Config>,
    pub http: reqwest::Client,
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let cfg = Config {
        base_url: env::var("BASE_URL").unwrap_or_else(|_| "http://localhost:8080".to_string()),
        google_client_id: env::var("GOOGLE_CLIENT_ID").ok().filter(|s| !s.is_empty()),
        google_client_secret: env::var("GOOGLE_CLIENT_SECRET").ok().filter(|s| !s.is_empty()),
        dev_login: env::var("DEV_LOGIN").map(|v| v == "1").unwrap_or(false),
    };

    let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| "strength.db".to_string());
    let pool = db::init_pool(&database_url);
    {
        let mut conn = pool.get().expect("db connection");
        conn.run_pending_migrations(MIGRATIONS).expect("migrations");
        seed::seed_exercises(&mut conn).expect("exercise seed");
        if cfg.dev_login {
            sample::ensure_dev_samples(&mut conn).expect("sample workouts");
        }
        // Group any recordings not yet attached to a session (idempotent).
        let logged = sessions::backfill(&mut conn).expect("session backfill");
        if logged > 0 {
            eprintln!("note: backfilled {logged} recording(s) into sessions");
        }
    }
    if cfg.google_client_id.is_none() {
        eprintln!("note: GOOGLE_CLIENT_ID not set — Google sign-in disabled");
    }
    if cfg.dev_login {
        eprintln!("note: DEV_LOGIN=1 — /auth/dev enabled, do not use in production");
    }

    let state = AppState {
        pool,
        cfg: Arc::new(cfg),
        http: reqwest::Client::new(),
    };

    let app = Router::new()
        .route("/", get(pages::home))
        .route("/privacy.html", get(pages::privacy))
        .route("/terms.html", get(pages::terms))
        .route("/watch/config", get(pages::watch_config))
        .route("/bodyweight", post(pages::add_bodyweight))
        .route("/bodyweight/delete", post(pages::delete_bodyweight))
        .route("/api/dashboard", get(api::dashboard))
        .route("/static/fonts/{name}", get(pages::font))
        .route("/workouts", get(pages::workouts_page))
        .route("/workouts/new", get(pages::builder_new))
        .route("/workouts/{id}", get(pages::workout_view))
        .route("/workouts/{id}/edit", get(pages::builder_edit))
        .route("/workouts/{id}/copy", post(pages::copy_workout))
        .route("/workouts/{id}/delete", post(pages::delete_workout_page))
        .route("/devices", get(pages::devices_page).post(pages::create_device))
        .route("/devices/{id}/delete", post(pages::delete_device))
        .route("/auth/google", get(auth::google_start))
        .route("/auth/google/callback", get(auth::google_callback))
        .route("/auth/dev", post(auth::dev_login))
        .route("/auth/logout", post(auth::logout))
        .route("/api/workouts", post(api::create_workout))
        .route(
            "/api/workouts/{id}",
            put(api::update_workout).delete(api::delete_workout),
        )
        .route("/api/workouts/{id}/packed", get(api::packed_preview))
        .route("/api/device/workouts", get(device::workouts))
        .route("/api/device/recordings", post(device::upload_recording))
        .route("/recordings", get(pages::recordings_page))
        .route("/recordings/{id}/csv", get(pages::recording_csv))
        .route("/sessions", get(pages::sessions_page))
        .route("/sessions/{id}", get(pages::session_detail_page))
        .route(
            "/api/sessions/{id}",
            put(api::update_session).delete(api::delete_session),
        )
        .with_state(state);

    let bind_host = env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let addr = format!("{bind_host}:{port}");
    println!("strength-server listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await.expect("bind");
    axum::serve(listener, app).await.expect("serve");
}
