//! Logged workout sessions: grouping watch recordings into sessions, backfilling
//! historical recordings, and saving user edits. A session mirrors the workout
//! structure but holds what was actually performed (reps/holds, weight, work
//! time). See migration 2026-07-21-000001_sessions.

use std::collections::{HashMap, HashSet};

use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::Deserialize;

use crate::error::AppError;
use crate::schema::{recordings, session_sets, sessions};

/// A gap larger than this between consecutive sets starts a new session.
const SESSION_GAP_SECS: i64 = 3600;

/// Append one recorded set to the appropriate session (creating one if the last
/// set for this workout is older than the session gap). Returns the session id.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_arguments)]
pub fn log_recording(
    conn: &mut SqliteConnection,
    user_id: i32,
    recording_id: Option<i32>,
    client_set_id: Option<i64>,
    workout_name: &str,
    movement_id: i32,
    exercise_name: &str,
    is_timed: bool,
    actual: i32,
    work_secs: Option<i32>,
    performed_at: NaiveDateTime,
) -> Result<i32, AppError> {
    // Idempotent on the watch's stable set id: if this set is already logged
    // (e.g. the live accel upload beat the offline-queue flush, or a re-flush),
    // return its session unchanged instead of inserting a duplicate.
    if let Some(cid) = client_set_id {
        let existing: Option<i32> = session_sets::table
            .inner_join(sessions::table)
            .filter(sessions::user_id.eq(user_id))
            .filter(session_sets::client_set_id.eq(cid))
            .select(sessions::id)
            .first(conn)
            .optional()?;
        if let Some(sid) = existing {
            return Ok(sid);
        }
    }

    let last: Option<(i32, NaiveDateTime)> = session_sets::table
        .inner_join(sessions::table)
        .filter(sessions::user_id.eq(user_id))
        .filter(sessions::workout_name.eq(workout_name))
        .order(session_sets::performed_at.desc())
        .select((sessions::id, session_sets::performed_at))
        .first(conn)
        .optional()?;

    let session_id = match last {
        Some((sid, last_at)) if (performed_at - last_at).num_seconds().abs() <= SESSION_GAP_SECS => {
            sid
        }
        _ => diesel::insert_into(sessions::table)
            .values((
                sessions::user_id.eq(user_id),
                sessions::workout_name.eq(workout_name),
                sessions::performed_on.eq(performed_at),
                sessions::notes.eq(""),
            ))
            .returning(sessions::id)
            .get_result::<i32>(conn)?,
    };

    let position: i64 = session_sets::table
        .filter(session_sets::session_id.eq(session_id))
        .count()
        .get_result(conn)?;

    diesel::insert_into(session_sets::table)
        .values((
            session_sets::session_id.eq(session_id),
            session_sets::position.eq(position as i32),
            session_sets::movement_id.eq(movement_id),
            session_sets::exercise_name.eq(exercise_name),
            session_sets::is_timed.eq(is_timed),
            session_sets::actual.eq(actual),
            session_sets::weight_kg.eq::<Option<f32>>(None),
            session_sets::work_secs.eq(work_secs),
            session_sets::recording_id.eq(recording_id),
            session_sets::performed_at.eq(performed_at),
            session_sets::client_set_id.eq(client_set_id),
        ))
        .execute(conn)?;
    Ok(session_id)
}

/// Log any recordings not yet attached to a session, in chronological order so
/// grouping is stable. Idempotent — safe to run at every startup.
pub fn backfill(conn: &mut SqliteConnection) -> Result<usize, AppError> {
    let linked: HashSet<i32> = session_sets::table
        .filter(session_sets::recording_id.is_not_null())
        .select(session_sets::recording_id)
        .load::<Option<i32>>(conn)?
        .into_iter()
        .flatten()
        .collect();

    let recs: Vec<(i32, i32, i32, String, String, bool, i32, i32, i32, NaiveDateTime)> =
        recordings::table
            .order(recordings::recorded_at.asc())
            .select((
                recordings::id,
                recordings::user_id,
                recordings::movement_id,
                recordings::exercise_name,
                recordings::workout_name,
                recordings::is_timed,
                recordings::actual,
                recordings::sample_count,
                recordings::sample_rate,
                recordings::recorded_at,
            ))
            .load(conn)?;

    let mut n = 0;
    for (id, uid, mv, exn, won, timed, actual, count, rate, at) in recs {
        if linked.contains(&id) {
            continue;
        }
        let work = if rate > 0 { Some(count / rate) } else { None };
        log_recording(conn, uid, Some(id), None, &won, mv, &exn, timed, actual, work, at)?;
        n += 1;
    }
    Ok(n)
}

#[derive(Deserialize)]
pub struct SessionInput {
    pub workout_name: String,
    /// "YYYY-MM-DD".
    pub performed_on: String,
    #[serde(default)]
    pub notes: String,
    pub sets: Vec<SessionSetInput>,
}

#[derive(Deserialize)]
pub struct SessionSetInput {
    pub movement_id: i32,
    #[serde(default)]
    pub exercise_name: String,
    #[serde(default)]
    pub is_timed: bool,
    pub actual: i32,
    #[serde(default)]
    pub weight_kg: Option<f32>,
    #[serde(default)]
    pub work_secs: Option<i32>,
    #[serde(default)]
    pub recording_id: Option<i32>,
}

/// Update a session and rewrite its sets (delete-and-reinsert, mirroring the
/// workout builder). Preserves each recording-linked set's original
/// `performed_at` so derived rest stays meaningful across edits.
pub fn save(
    conn: &mut SqliteConnection,
    user_id: i32,
    session_id: i32,
    input: &SessionInput,
) -> Result<i32, AppError> {
    let day = chrono::NaiveDate::parse_from_str(input.performed_on.trim(), "%Y-%m-%d")
        .map_err(|e| AppError::BadRequest(format!("bad date: {e}")))?;
    let performed_on = day.and_hms_opt(12, 0, 0).unwrap();
    let name = input.workout_name.trim().to_string();

    conn.transaction::<_, AppError, _>(|conn| {
        let owned: Option<i32> = sessions::table
            .filter(sessions::id.eq(session_id))
            .filter(sessions::user_id.eq(user_id))
            .select(sessions::id)
            .first(conn)
            .optional()?;
        if owned.is_none() {
            return Err(AppError::NotFound);
        }

        diesel::update(sessions::table.find(session_id))
            .set((
                sessions::workout_name.eq(&name),
                sessions::performed_on.eq(performed_on),
                sessions::notes.eq(&input.notes),
            ))
            .execute(conn)?;

        let existing: Vec<(Option<i32>, NaiveDateTime)> = session_sets::table
            .filter(session_sets::session_id.eq(session_id))
            .select((session_sets::recording_id, session_sets::performed_at))
            .load(conn)?;
        let mut at_by_rec: HashMap<i32, NaiveDateTime> = HashMap::new();
        for (rid, at) in existing {
            if let Some(r) = rid {
                at_by_rec.insert(r, at);
            }
        }

        diesel::delete(session_sets::table.filter(session_sets::session_id.eq(session_id)))
            .execute(conn)?;

        for (pos, s) in input.sets.iter().enumerate() {
            let at = s
                .recording_id
                .and_then(|r| at_by_rec.get(&r).copied())
                .unwrap_or(performed_on);
            diesel::insert_into(session_sets::table)
                .values((
                    session_sets::session_id.eq(session_id),
                    session_sets::position.eq(pos as i32),
                    session_sets::movement_id.eq(s.movement_id),
                    session_sets::exercise_name.eq(&s.exercise_name),
                    session_sets::is_timed.eq(s.is_timed),
                    session_sets::actual.eq(s.actual),
                    session_sets::weight_kg.eq(s.weight_kg),
                    session_sets::work_secs.eq(s.work_secs),
                    session_sets::recording_id.eq(s.recording_id),
                    session_sets::performed_at.eq(at),
                ))
                .execute(conn)?;
        }
        Ok(session_id)
    })
}
