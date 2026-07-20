use axum::extract::{Path, State};
use axum::Json;
use diesel::prelude::*;
use serde_json::json;

use crate::auth::CurrentUser;
use crate::error::AppError;
use crate::models::Workout;
use crate::schema::workouts;
use crate::{db, pack, workouts as wk, AppState};

pub async fn create_workout(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Json(input): Json<wk::WorkoutInput>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (id, size) = db::run(&state.pool, move |conn| wk::save(conn, user.id, None, &input)).await?;
    Ok(Json(json!({ "id": id, "packed_size": size })))
}

pub async fn update_workout(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<i32>,
    Json(input): Json<wk::WorkoutInput>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (id, size) =
        db::run(&state.pool, move |conn| wk::save(conn, user.id, Some(id), &input)).await?;
    Ok(Json(json!({ "id": id, "packed_size": size })))
}

pub async fn delete_workout(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, AppError> {
    db::run(&state.pool, move |conn| {
        let n = diesel::delete(
            workouts::table
                .filter(workouts::id.eq(id))
                .filter(workouts::owner_id.eq(user.id)),
        )
        .execute(conn)?;
        if n == 0 {
            return Err(AppError::NotFound);
        }
        Ok(())
    })
    .await?;
    Ok(Json(json!({ "deleted": true })))
}

/// Hex preview of the packed watch binary for a workout.
pub async fn packed_preview(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (title, bytes) = db::run(&state.pool, move |conn| {
        let w: Workout = workouts::table
            .filter(workouts::id.eq(id))
            .filter(workouts::owner_id.eq(user.id))
            .first(conn)?;
        let details = wk::load_details(conn, &[w.id])?;
        let rows = details.get(&w.id).cloned().unwrap_or_default();
        let bytes = pack::pack_workout(&w.title, &wk::to_pack_exercises(&rows))
            .map_err(AppError::BadRequest)?;
        Ok((w.title, bytes))
    })
    .await?;

    let hex: Vec<String> = bytes
        .chunks(16)
        .map(|row| row.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" "))
        .collect();
    Ok(Json(json!({
        "title": title,
        "size": bytes.len(),
        "cap": pack::PACK_CAP,
        "hex": hex,
    })))
}
