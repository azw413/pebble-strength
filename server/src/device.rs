use axum::extract::State;
use axum::http::header::AUTHORIZATION;
use axum::http::HeaderMap;
use axum::Json;
use base64::Engine;
use chrono::Utc;
use diesel::prelude::*;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::auth::hash_token;
use crate::error::AppError;
use crate::models::Device;
use crate::schema::{devices, exercises, recordings, users};
use crate::{db, pack, sessions, workouts as wk, AppState};

fn bearer_token(headers: &HeaderMap) -> Result<String, AppError> {
    headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_string)
        .ok_or(AppError::Unauthorized)
}

/// Resolve the user for a device API call: a valid Bearer device token, or —
/// with DEV_LOGIN=1 and no token — the dev account.
fn device_user(
    conn: &mut SqliteConnection,
    token: Result<String, AppError>,
    dev_fallback: bool,
) -> Result<i32, AppError> {
    match token {
        Ok(t) => {
            let hash = hash_token(&t);
            let device: Device = devices::table
                .filter(devices::token_hash.eq(&hash))
                .first(conn)
                .optional()?
                .ok_or(AppError::Unauthorized)?;
            diesel::update(devices::table.find(device.id))
                .set(devices::last_sync_at.eq(Utc::now().naive_utc()))
                .execute(conn)?;
            Ok(device.user_id)
        }
        Err(_) if dev_fallback => users::table
            .filter(users::google_sub.eq("dev:local"))
            .select(users::id)
            .first::<i32>(conn)
            .optional()?
            .ok_or(AppError::Unauthorized),
        Err(e) => Err(e),
    }
}

/// GET /api/device/workouts — everything the watch needs, packed.
pub async fn workouts(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let token = bearer_token(&headers);
    let dev_fallback = state.cfg.dev_login;
    let slots = db::run(&state.pool, move |conn| {
        let user_id = device_user(conn, token, dev_fallback)?;
        wk::packed_slots(conn, user_id)
    })
    .await?;

    let b64 = base64::engine::general_purpose::STANDARD;
    let slots_json: Vec<serde_json::Value> = slots
        .iter()
        .map(|(slot, title, bytes)| {
            json!({
                "slot": slot,
                "title": title,
                "size": bytes.len(),
                "sha256": hex::encode(&Sha256::digest(bytes)[..8]),
                "data": b64.encode(bytes),
            })
        })
        .collect();
    Ok(Json(json!({
        "format_version": pack::PACK_VERSION,
        "slots": slots_json,
    })))
}

#[derive(Deserialize)]
pub struct RecordingUpload {
    pub movement_id: i32,
    #[serde(default)]
    pub workout_name: String,
    #[serde(default)]
    pub set_index: i32,
    pub actual: i32,
    #[serde(default)]
    pub is_timed: bool,
    #[serde(default = "default_rate")]
    pub sample_rate: i32,
    pub sample_count: i32,
    #[serde(default)]
    pub truncated: bool,
    /// Base64 of packed little-endian i16 x,y,z triplets (mG).
    pub data: String,
}

fn default_rate() -> i32 {
    25
}

/// POST /api/device/recordings — labelled raw accel capture of one set (§6).
pub async fn upload_recording(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(up): Json<RecordingUpload>,
) -> Result<Json<serde_json::Value>, AppError> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&up.data)
        .map_err(|e| AppError::BadRequest(format!("bad base64: {e}")))?;
    if bytes.len() != up.sample_count as usize * 6 {
        return Err(AppError::BadRequest(format!(
            "sample_count {} does not match {} data bytes",
            up.sample_count,
            bytes.len()
        )));
    }
    if bytes.is_empty() {
        return Err(AppError::BadRequest("empty recording".into()));
    }

    let token = bearer_token(&headers);
    let dev_fallback = state.cfg.dev_login;
    let id = db::run(&state.pool, move |conn| {
        let user_id = device_user(conn, token, dev_fallback)?;
        let exercise_name: String = exercises::table
            .filter(exercises::watch_movement_id.eq(up.movement_id))
            .select(exercises::name)
            .first(conn)
            .optional()?
            .unwrap_or_default();
        let id = diesel::insert_into(recordings::table)
            .values((
                recordings::user_id.eq(user_id),
                recordings::movement_id.eq(up.movement_id),
                recordings::exercise_name.eq(&exercise_name),
                recordings::workout_name.eq(&up.workout_name),
                recordings::set_index.eq(up.set_index),
                recordings::actual.eq(up.actual),
                recordings::is_timed.eq(up.is_timed),
                recordings::sample_rate.eq(up.sample_rate),
                recordings::sample_count.eq(up.sample_count),
                recordings::truncated.eq(up.truncated),
                recordings::samples.eq(&bytes),
            ))
            .returning(recordings::id)
            .get_result::<i32>(conn)?;
        // Auto-log this set into a session (work time ≈ the accel capture span).
        let work = if up.sample_rate > 0 {
            Some(up.sample_count / up.sample_rate)
        } else {
            None
        };
        sessions::log_recording(
            conn,
            user_id,
            id,
            &up.workout_name,
            up.movement_id,
            &exercise_name,
            up.is_timed,
            up.actual,
            work,
            Utc::now().naive_utc(),
        )?;
        Ok(id)
    })
    .await?;
    Ok(Json(json!({ "id": id })))
}
