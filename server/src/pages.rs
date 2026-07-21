use askama::Template;
use axum::extract::{Path, State};
use axum::response::{Html, Redirect};
use axum::Form;
use diesel::prelude::*;
use serde::Deserialize;

use crate::auth::{self, CurrentUser, OptionalUser};
use crate::error::AppError;
use crate::models::{Device, Exercise, Workout};
use crate::schema::{bodyweights, devices, exercises, workouts};
use crate::{db, pack, workouts as wk, AppState};

/// serde_json → string safe to inline inside a <script> block.
fn script_json<T: serde::Serialize>(value: &T) -> Result<String, AppError> {
    let s = serde_json::to_string(value).map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(s.replace('<', "\\u003c"))
}

#[derive(Template)]
#[template(path = "landing.html")]
struct LandingTemplate {
    google_enabled: bool,
    dev_login: bool,
}

pub struct BwRow {
    pub date: String,
    pub weight: String,
}

#[derive(Template)]
#[template(path = "dashboard.html")]
struct DashboardTemplate {
    user_name: String,
    today: String,
    bodyweights: Vec<BwRow>,
}

/// Home: the dashboard when logged in, otherwise the landing page.
pub async fn home(
    State(state): State<AppState>,
    OptionalUser(user): OptionalUser,
) -> Result<Html<String>, AppError> {
    let Some(user) = user else {
        let tpl = LandingTemplate {
            google_enabled: state.cfg.google_client_id.is_some(),
            dev_login: state.cfg.dev_login,
        };
        return Ok(Html(tpl.render()?));
    };

    let user_name = if user.display_name.is_empty() {
        user.email.clone()
    } else {
        user.display_name.clone()
    };
    let bodyweights = db::run(&state.pool, move |conn| {
        let rows: Vec<(chrono::NaiveDate, f32)> = bodyweights::table
            .filter(bodyweights::user_id.eq(user.id))
            .order(bodyweights::measured_on.desc())
            .limit(8)
            .select((bodyweights::measured_on, bodyweights::weight_kg))
            .load(conn)?;
        Ok(rows
            .into_iter()
            .map(|(d, w)| BwRow {
                date: d.format("%Y-%m-%d").to_string(),
                weight: format!("{w}"),
            })
            .collect::<Vec<_>>())
    })
    .await?;

    let today = chrono::Utc::now().naive_utc().date().format("%Y-%m-%d").to_string();
    let tpl = DashboardTemplate { user_name, today, bodyweights };
    Ok(Html(tpl.render()?))
}

#[derive(Deserialize)]
pub struct BwForm {
    measured_on: String,
    weight_kg: f32,
}

/// Add (or replace) a bodyweight entry for a date.
pub async fn add_bodyweight(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Form(form): Form<BwForm>,
) -> Result<Redirect, AppError> {
    let date = chrono::NaiveDate::parse_from_str(form.measured_on.trim(), "%Y-%m-%d")
        .map_err(|e| AppError::BadRequest(format!("bad date: {e}")))?;
    let weight = form.weight_kg;
    db::run(&state.pool, move |conn| {
        diesel::delete(
            bodyweights::table
                .filter(bodyweights::user_id.eq(user.id))
                .filter(bodyweights::measured_on.eq(date)),
        )
        .execute(conn)?;
        diesel::insert_into(bodyweights::table)
            .values((
                bodyweights::user_id.eq(user.id),
                bodyweights::measured_on.eq(date),
                bodyweights::weight_kg.eq(weight),
            ))
            .execute(conn)?;
        Ok(())
    })
    .await?;
    Ok(Redirect::to("/"))
}

pub struct WorkoutCard {
    pub id: i32,
    pub title: String,
    pub description: String,
    pub slot_label: String,
    pub is_public: bool,
    pub exercise_count: usize,
    pub set_count: usize,
    pub packed_size: usize,
}

#[derive(Template)]
#[template(path = "workouts.html")]
struct WorkoutsTemplate {
    user_name: String,
    cards: Vec<WorkoutCard>,
    slots: Vec<String>,
    pack_cap: usize,
}

pub async fn workouts_page(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
) -> Result<Html<String>, AppError> {
    let user_name = if user.display_name.is_empty() { user.email.clone() } else { user.display_name.clone() };
    let (cards, slots) = db::run(&state.pool, move |conn| {
        let ws: Vec<Workout> = workouts::table
            .filter(workouts::owner_id.eq(user.id))
            .order(workouts::created_at.asc())
            .load(conn)?;
        let ids: Vec<i32> = ws.iter().map(|w| w.id).collect();
        let mut details = wk::load_details(conn, &ids)?;
        let slot_of = wk::slot_map(conn, user.id)?;

        let cards: Vec<WorkoutCard> = ws
            .into_iter()
            .map(|w| {
                let rows = details.remove(&w.id).unwrap_or_default();
                let packed_size = pack::pack_workout(&w.title, &wk::to_pack_exercises(&rows))
                    .map(|b| b.len())
                    .unwrap_or(0);
                WorkoutCard {
                    slot_label: slot_of
                        .get(&w.id)
                        .map(|s| format!("watch slot {s}"))
                        .unwrap_or_default(),
                    exercise_count: rows.len(),
                    set_count: rows.iter().map(|(_, _, s)| s.len()).sum(),
                    id: w.id,
                    title: w.title,
                    description: w.description,
                    is_public: w.is_public,
                    packed_size,
                }
            })
            .collect();

        let by_slot: std::collections::HashMap<i32, i32> =
            slot_of.iter().map(|(w, s)| (*s, *w)).collect();
        let title_of: std::collections::HashMap<i32, &str> =
            cards.iter().map(|c| (c.id, c.title.as_str())).collect();
        let slots = (1..=wk::MAX_SLOT)
            .map(|s| {
                by_slot
                    .get(&s)
                    .and_then(|wid| title_of.get(wid))
                    .map(|t| format!("{s}: {t}"))
                    .unwrap_or_else(|| format!("{s}: —"))
            })
            .collect();
        Ok((cards, slots))
    })
    .await?;

    let tpl = WorkoutsTemplate { user_name, cards, slots, pack_cap: pack::PACK_CAP };
    Ok(Html(tpl.render()?))
}

#[derive(Template)]
#[template(path = "builder.html")]
struct BuilderTemplate {
    heading: String,
    exercises_json: String,
    workout_json: String,
    workout_id_json: String,
}

pub async fn builder_new(
    State(state): State<AppState>,
    CurrentUser(_user): CurrentUser,
) -> Result<Html<String>, AppError> {
    let exs = db::run(&state.pool, |conn| {
        Ok(exercises::table
            .order((exercises::body_area.asc(), exercises::name.asc()))
            .load::<Exercise>(conn)?)
    })
    .await?;
    let tpl = BuilderTemplate {
        heading: "New workout".to_string(),
        exercises_json: script_json(&exs)?,
        workout_json: "null".to_string(),
        workout_id_json: "null".to_string(),
    };
    Ok(Html(tpl.render()?))
}

pub async fn builder_edit(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<i32>,
) -> Result<Html<String>, AppError> {
    let (exs, input) = db::run(&state.pool, move |conn| {
        let w: Workout = workouts::table
            .filter(workouts::id.eq(id))
            .filter(workouts::owner_id.eq(user.id))
            .first(conn)?;
        let details = wk::load_details(conn, &[w.id])?;
        let rows = details.get(&w.id).cloned().unwrap_or_default();
        let slot = wk::slot_map(conn, user.id)?.get(&w.id).copied();
        let input = wk::to_input(&rows, &w, slot);
        let exs = exercises::table
            .order((exercises::body_area.asc(), exercises::name.asc()))
            .load::<Exercise>(conn)?;
        Ok((exs, input))
    })
    .await?;
    let tpl = BuilderTemplate {
        heading: format!("Edit: {}", input.title),
        exercises_json: script_json(&exs)?,
        workout_json: script_json(&input)?,
        workout_id_json: id.to_string(),
    };
    Ok(Html(tpl.render()?))
}

pub struct DeviceRow {
    pub id: i32,
    pub label: String,
    pub created: String,
    pub last_sync: String,
}

#[derive(Template)]
#[template(path = "devices.html")]
struct DevicesTemplate {
    rows: Vec<DeviceRow>,
    new_token: Option<String>,
}

async fn render_devices(
    state: &AppState,
    user_id: i32,
    new_token: Option<String>,
) -> Result<Html<String>, AppError> {
    let rows = db::run(&state.pool, move |conn| {
        let ds: Vec<Device> = devices::table
            .filter(devices::user_id.eq(user_id))
            .order(devices::created_at.asc())
            .load(conn)?;
        Ok(ds
            .into_iter()
            .map(|d| DeviceRow {
                id: d.id,
                label: d.label,
                created: d.created_at.format("%Y-%m-%d").to_string(),
                last_sync: d
                    .last_sync_at
                    .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_else(|| "never".to_string()),
            })
            .collect())
    })
    .await?;
    let tpl = DevicesTemplate { rows, new_token };
    Ok(Html(tpl.render()?))
}

pub async fn devices_page(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
) -> Result<Html<String>, AppError> {
    render_devices(&state, user.id, None).await
}

#[derive(Deserialize)]
pub struct NewDeviceForm {
    label: String,
}

pub async fn create_device(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Form(form): Form<NewDeviceForm>,
) -> Result<Html<String>, AppError> {
    let token = auth::random_token();
    let hash = auth::hash_token(&token);
    let label = if form.label.trim().is_empty() { "Pebble".to_string() } else { form.label.trim().to_string() };
    db::run(&state.pool, move |conn| {
        diesel::insert_into(devices::table)
            .values((
                devices::user_id.eq(user.id),
                devices::token_hash.eq(&hash),
                devices::label.eq(&label),
            ))
            .execute(conn)?;
        Ok(())
    })
    .await?;
    render_devices(&state, user.id, Some(token)).await
}

pub struct RecordingRow {
    pub id: i32,
    pub when: String,
    pub exercise: String,
    pub workout: String,
    pub set_index: i32,
    pub label: String,
    pub duration: String,
    pub truncated: bool,
}

#[derive(Template)]
#[template(path = "recordings.html")]
struct RecordingsTemplate {
    rows: Vec<RecordingRow>,
}

pub async fn recordings_page(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
) -> Result<Html<String>, AppError> {
    use crate::schema::recordings;
    let rows = db::run(&state.pool, move |conn| {
        let rs: Vec<(i32, String, String, i32, i32, bool, i32, i32, bool, chrono::NaiveDateTime)> =
            recordings::table
                .filter(recordings::user_id.eq(user.id))
                .order(recordings::recorded_at.desc())
                .limit(200)
                .select((
                    recordings::id,
                    recordings::exercise_name,
                    recordings::workout_name,
                    recordings::set_index,
                    recordings::actual,
                    recordings::is_timed,
                    recordings::sample_rate,
                    recordings::sample_count,
                    recordings::truncated,
                    recordings::recorded_at,
                ))
                .load(conn)?;
        Ok(rs
            .into_iter()
            .map(|(id, ex, wo, set, actual, timed, rate, count, trunc, at)| RecordingRow {
                id,
                when: at.format("%Y-%m-%d %H:%M:%S").to_string(),
                exercise: if ex.is_empty() { "?".into() } else { ex },
                workout: wo,
                set_index: set + 1,
                label: if timed { format!("{actual} s hold") } else { format!("{actual} reps") },
                duration: format!("{:.1} s", count as f32 / rate.max(1) as f32),
                truncated: trunc,
            })
            .collect())
    })
    .await?;
    let tpl = RecordingsTemplate { rows };
    Ok(Html(tpl.render()?))
}

pub async fn recording_csv(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<i32>,
) -> Result<axum::response::Response, AppError> {
    use crate::schema::recordings;
    let (blob, rate) = db::run(&state.pool, move |conn| {
        let row: (Vec<u8>, i32) = recordings::table
            .filter(recordings::id.eq(id))
            .filter(recordings::user_id.eq(user.id))
            .select((recordings::samples, recordings::sample_rate))
            .first(conn)?;
        Ok(row)
    })
    .await?;

    let mut csv = String::with_capacity(blob.len() * 4);
    csv.push_str("t_ms,x,y,z\n");
    let step_ms = 1000.0 / rate.max(1) as f32;
    for (i, s) in blob.chunks_exact(6).enumerate() {
        let x = i16::from_le_bytes([s[0], s[1]]);
        let y = i16::from_le_bytes([s[2], s[3]]);
        let z = i16::from_le_bytes([s[4], s[5]]);
        csv.push_str(&format!("{:.0},{x},{y},{z}\n", i as f32 * step_ms));
    }
    use axum::http::header;
    use axum::response::IntoResponse;
    Ok((
        [
            (header::CONTENT_TYPE, "text/csv".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"recording_{id}.csv\""),
            ),
        ],
        csv,
    )
        .into_response())
}

pub async fn delete_device(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<i32>,
) -> Result<Redirect, AppError> {
    db::run(&state.pool, move |conn| {
        diesel::delete(
            devices::table
                .filter(devices::id.eq(id))
                .filter(devices::user_id.eq(user.id)),
        )
        .execute(conn)?;
        Ok(())
    })
    .await?;
    Ok(Redirect::to("/devices"))
}

// ---- Sessions ----

pub struct SessionCard {
    pub id: i32,
    pub name: String,
    pub date: String,
    pub exercises: usize,
    pub sets: usize,
    pub reps: i32,
    pub hold_secs: i32,
    pub work_secs: i32,
}

#[derive(Template)]
#[template(path = "sessions.html")]
struct SessionsTemplate {
    rows: Vec<SessionCard>,
}

pub async fn sessions_page(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
) -> Result<Html<String>, AppError> {
    use crate::schema::{session_sets, sessions};
    let rows = db::run(&state.pool, move |conn| {
        let ss: Vec<crate::models::Session> = sessions::table
            .filter(sessions::user_id.eq(user.id))
            .order(sessions::performed_on.desc())
            .load(conn)?;
        let ids: Vec<i32> = ss.iter().map(|s| s.id).collect();
        let sets: Vec<(i32, i32, i32, bool, Option<i32>)> = session_sets::table
            .filter(session_sets::session_id.eq_any(&ids))
            .select((
                session_sets::session_id,
                session_sets::movement_id,
                session_sets::actual,
                session_sets::is_timed,
                session_sets::work_secs,
            ))
            .load(conn)?;
        let cards = ss
            .into_iter()
            .map(|s| {
                let mine: Vec<&(i32, i32, i32, bool, Option<i32>)> =
                    sets.iter().filter(|r| r.0 == s.id).collect();
                let mut movements: Vec<i32> = mine.iter().map(|r| r.1).collect();
                movements.sort_unstable();
                movements.dedup();
                SessionCard {
                    id: s.id,
                    name: if s.workout_name.is_empty() {
                        "(unnamed)".into()
                    } else {
                        s.workout_name.clone()
                    },
                    date: s.performed_on.format("%A %Y-%m-%d").to_string(),
                    exercises: movements.len(),
                    sets: mine.len(),
                    reps: mine.iter().filter(|r| !r.3).map(|r| r.2).sum(),
                    hold_secs: mine.iter().filter(|r| r.3).map(|r| r.2).sum(),
                    work_secs: mine.iter().filter_map(|r| r.4).sum(),
                }
            })
            .collect();
        Ok(cards)
    })
    .await?;
    Ok(Html(SessionsTemplate { rows }.render()?))
}

#[derive(Template)]
#[template(path = "session.html")]
struct SessionEditTemplate {
    session_id: i32,
    heading: String,
    session_json: String,
    exercises_json: String,
}

pub async fn session_detail_page(
    State(state): State<AppState>,
    CurrentUser(user): CurrentUser,
    Path(id): Path<i32>,
) -> Result<Html<String>, AppError> {
    use crate::schema::{exercises, session_sets, sessions};
    let (session_json, exercises_json, heading) = db::run(&state.pool, move |conn| {
        let s: crate::models::Session = sessions::table
            .filter(sessions::id.eq(id))
            .filter(sessions::user_id.eq(user.id))
            .first(conn)
            .optional()?
            .ok_or(AppError::NotFound)?;
        let sets: Vec<crate::models::SessionSet> = session_sets::table
            .filter(session_sets::session_id.eq(id))
            .order(session_sets::position.asc())
            .load(conn)?;

        // Group consecutive same-movement sets into exercises; derive per-set
        // rest from the gap to the next set's start, minus this set's work time.
        let mut ex_json: Vec<serde_json::Value> = Vec::new();
        let mut i = 0;
        while i < sets.len() {
            let mv = sets[i].movement_id;
            let name = sets[i].exercise_name.clone();
            let timed = sets[i].is_timed;
            let mut group: Vec<serde_json::Value> = Vec::new();
            while i < sets.len() && sets[i].movement_id == mv {
                let cur = &sets[i];
                let rest = if i + 1 < sets.len() {
                    let d = (sets[i + 1].performed_at - cur.performed_at).num_seconds()
                        - cur.work_secs.unwrap_or(0) as i64;
                    Some(d.max(0))
                } else {
                    None
                };
                group.push(serde_json::json!({
                    "actual": cur.actual,
                    "weight_kg": cur.weight_kg,
                    "work_secs": cur.work_secs,
                    "rest_secs": rest,
                    "recording_id": cur.recording_id,
                }));
                i += 1;
            }
            ex_json.push(serde_json::json!({
                "movement_id": mv,
                "exercise_name": name,
                "is_timed": timed,
                "sets": group,
            }));
        }

        let session_json = serde_json::json!({
            "id": s.id,
            "workout_name": s.workout_name,
            "performed_on": s.performed_on.format("%Y-%m-%d").to_string(),
            "notes": s.notes,
            "exercises": ex_json,
        });

        let cat: Vec<serde_json::Value> = exercises::table
            .order(exercises::name.asc())
            .load::<Exercise>(conn)?
            .into_iter()
            .map(|e| {
                serde_json::json!({
                    "movement_id": e.watch_movement_id,
                    "name": e.name,
                    "body_area": e.body_area,
                    "default_timed": e.default_timed,
                })
            })
            .collect();

        let heading = if s.workout_name.is_empty() {
            format!("Session — {}", s.performed_on.format("%Y-%m-%d"))
        } else {
            format!("{} — {}", s.workout_name, s.performed_on.format("%Y-%m-%d"))
        };
        Ok((script_json(&session_json)?, script_json(&cat)?, heading))
    })
    .await?;

    let tpl = SessionEditTemplate {
        session_id: id,
        heading,
        session_json,
        exercises_json,
    };
    Ok(Html(tpl.render()?))
}
