//! Home dashboard analytics: per-session volume, estimated 1RM progress, and
//! bodyweight-aware load for calisthenics.
//!
//! Effective load of a set = bodyweight(nearest date) * exercise.load_factor
//! + added weight. Volume = reps * load. Estimated 1RM = load * (1 + reps/30)
//! (Epley). Timed holds are excluded from volume/1RM (isometric).

use std::collections::HashMap;

use chrono::{Datelike, Duration, NaiveDate, Utc};
use diesel::prelude::*;
use serde::Deserialize;
use serde_json::json;

use crate::error::AppError;
use crate::models::{Session, SessionSet};
use crate::schema::{bodyweights, exercises, session_sets, sessions};

/// Fallback when the user hasn't logged any bodyweight yet.
const DEFAULT_BW_KG: f32 = 75.0;

#[derive(Deserialize)]
pub struct DashQuery {
    #[serde(default = "default_window")]
    pub window: String,
    #[serde(default)]
    pub offset: i32,
    /// Kept as a string so an empty `ex=` (sent before an exercise is picked)
    /// parses as "none" rather than failing query deserialization.
    #[serde(default)]
    pub ex: Option<String>,
}

impl DashQuery {
    fn exercise(&self) -> Option<i32> {
        self.ex.as_deref().and_then(|s| s.trim().parse().ok())
    }
}

fn default_window() -> String {
    "month".to_string()
}

fn add_months(d: NaiveDate, delta: i32) -> NaiveDate {
    let m0 = d.year() * 12 + (d.month() as i32 - 1) + delta;
    let y = m0.div_euclid(12);
    let m = m0.rem_euclid(12) as u32 + 1;
    NaiveDate::from_ymd_opt(y, m, 1).unwrap()
}

/// [start, end) date bounds and a human label for the window at `offset`
/// (0 = current period, -1 = previous, +1 = next).
fn window_bounds(window: &str, offset: i32, today: NaiveDate) -> (NaiveDate, NaiveDate, String) {
    match window {
        "week" => {
            let dow = today.weekday().num_days_from_sunday() as i64;
            let start = today - Duration::days(dow) + Duration::weeks(offset as i64);
            let end = start + Duration::weeks(1);
            let label = format!("Week of {}", start.format("%-d %b %Y"));
            (start, end, label)
        }
        "year" => {
            let y = today.year() + offset;
            let start = NaiveDate::from_ymd_opt(y, 1, 1).unwrap();
            let end = NaiveDate::from_ymd_opt(y + 1, 1, 1).unwrap();
            (start, end, y.to_string())
        }
        _ => {
            let base = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap();
            let start = add_months(base, offset);
            let end = add_months(start, 1);
            let label = start.format("%B %Y").to_string();
            (start, end, label)
        }
    }
}

/// Bodyweight logged nearest to `date`, or the default if none.
fn bw_for(log: &[(NaiveDate, f32)], date: NaiveDate) -> f32 {
    log.iter()
        .min_by_key(|(d, _)| (*d - date).num_days().abs())
        .map(|(_, w)| *w)
        .unwrap_or(DEFAULT_BW_KG)
}

pub fn dashboard_json(
    conn: &mut SqliteConnection,
    user_id: i32,
    q: &DashQuery,
) -> Result<serde_json::Value, AppError> {
    let selected_ex = q.exercise();
    let today = Utc::now().naive_utc().date();
    let (start, end, label) = window_bounds(&q.window, q.offset, today);
    let start_dt = start.and_hms_opt(0, 0, 0).unwrap();
    let end_dt = end.and_hms_opt(0, 0, 0).unwrap();

    let sess: Vec<Session> = sessions::table
        .filter(sessions::user_id.eq(user_id))
        .filter(sessions::performed_on.ge(start_dt))
        .filter(sessions::performed_on.lt(end_dt))
        .order(sessions::performed_on.asc())
        .load(conn)?;
    let sess_ids: Vec<i32> = sess.iter().map(|s| s.id).collect();
    let sets: Vec<SessionSet> = session_sets::table
        .filter(session_sets::session_id.eq_any(&sess_ids))
        .load(conn)?;

    let factors: HashMap<i32, f32> = exercises::table
        .select((exercises::watch_movement_id, exercises::load_factor))
        .load::<(i32, f32)>(conn)?
        .into_iter()
        .collect();
    let names: HashMap<i32, String> = exercises::table
        .select((exercises::watch_movement_id, exercises::name))
        .load::<(i32, String)>(conn)?
        .into_iter()
        .collect();
    let bw_log: Vec<(NaiveDate, f32)> = bodyweights::table
        .filter(bodyweights::user_id.eq(user_id))
        .order(bodyweights::measured_on.asc())
        .select((bodyweights::measured_on, bodyweights::weight_kg))
        .load(conn)?;

    // Fixed x-axis buckets: weekday over the week, day-of-month over the month,
    // or month over the year. Empty slots still render, so the scale is real.
    let bucket_count: usize = match q.window.as_str() {
        "week" => 7,
        "year" => 12,
        _ => (end - start).num_days() as usize,
    };
    let axis_labels: Vec<String> = (0..bucket_count)
        .map(|i| match q.window.as_str() {
            "week" => (start + Duration::days(i as i64)).format("%a %d/%m").to_string(),
            "year" => NaiveDate::from_ymd_opt(start.year(), i as u32 + 1, 1)
                .unwrap()
                .format("%b")
                .to_string(),
            _ => (i + 1).to_string(),
        })
        .collect();
    let bucket_of = |d: NaiveDate| -> usize {
        match q.window.as_str() {
            "week" => (d - start).num_days().clamp(0, 6) as usize,
            "year" => (d.month0() as usize).min(11),
            _ => (d.day0() as usize).min(bucket_count - 1),
        }
    };

    let mut vol_buckets = vec![0.0f32; bucket_count];
    let mut rm_buckets: Vec<Option<f32>> = vec![None; bucket_count];
    let mut total_volume = 0.0f32;
    let mut best_1rm: Option<f32> = None;
    let mut hold_secs = 0i64;

    for s in &sess {
        let bw = bw_for(&bw_log, s.performed_on.date());
        let bi = bucket_of(s.performed_on.date());
        let mut sess_1rm: Option<f32> = None;
        for st in sets.iter().filter(|x| x.session_id == s.id) {
            if st.is_timed {
                hold_secs += st.actual as i64;
                continue;
            }
            let factor = factors.get(&st.movement_id).copied().unwrap_or(0.0);
            let load = bw * factor + st.weight_kg.unwrap_or(0.0);
            let vol = st.actual as f32 * load;
            vol_buckets[bi] += vol;
            total_volume += vol;
            if Some(st.movement_id) == selected_ex && st.actual > 0 {
                let e1rm = load * (1.0 + st.actual as f32 / 30.0);
                sess_1rm = Some(sess_1rm.map_or(e1rm, |b: f32| b.max(e1rm)));
            }
        }
        if let Some(v) = sess_1rm {
            best_1rm = Some(best_1rm.map_or(v, |b: f32| b.max(v)));
            rm_buckets[bi] = Some(rm_buckets[bi].map_or(v, |b: f32| b.max(v)));
        }
    }

    // New PB: the selected exercise's best 1RM this window beats its best in
    // every earlier session.
    let is_pb = match (selected_ex, best_1rm) {
        (Some(exm), Some(window_best)) => {
            let prior: Vec<(chrono::NaiveDateTime, i32, Option<f32>)> = session_sets::table
                .inner_join(sessions::table)
                .filter(sessions::user_id.eq(user_id))
                .filter(session_sets::movement_id.eq(exm))
                .filter(session_sets::is_timed.eq(false))
                .filter(sessions::performed_on.lt(start_dt))
                .select((
                    sessions::performed_on,
                    session_sets::actual,
                    session_sets::weight_kg,
                ))
                .load(conn)?;
            let factor = factors.get(&exm).copied().unwrap_or(0.0);
            let mut prior_best = 0.0f32;
            for (pdate, reps, w) in prior {
                if reps <= 0 {
                    continue;
                }
                let load = bw_for(&bw_log, pdate.date()) * factor + w.unwrap_or(0.0);
                let e = load * (1.0 + reps as f32 / 30.0);
                prior_best = prior_best.max(e);
            }
            window_best > prior_best + 0.01
        }
        _ => false,
    };

    let axis: Vec<serde_json::Value> = (0..bucket_count)
        .map(|i| {
            json!({
                "label": axis_labels[i],
                "volume": vol_buckets[i].round(),
                "one_rm": rm_buckets[i].map(|v| (v * 10.0).round() / 10.0),
            })
        })
        .collect();

    // Exercise picker: rep-based movements that appear in any of the user's sessions.
    let mut picker_ids: Vec<i32> = session_sets::table
        .inner_join(sessions::table)
        .filter(sessions::user_id.eq(user_id))
        .filter(session_sets::is_timed.eq(false))
        .select(session_sets::movement_id)
        .distinct()
        .load(conn)?;
    picker_ids.sort_unstable();
    let exercises_json: Vec<serde_json::Value> = picker_ids
        .iter()
        .map(|mv| {
            json!({
                "movement_id": mv,
                "name": names.get(mv).cloned().unwrap_or_else(|| format!("#{mv}")),
            })
        })
        .collect();

    Ok(json!({
        "window": q.window,
        "offset": q.offset,
        "label": label,
        "can_next": q.offset < 0,
        "selected_exercise": selected_ex,
        "exercises": exercises_json,
        "axis": axis,
        "insights": {
            "total_volume": total_volume.round(),
            "session_count": sess.len(),
            "best_1rm": best_1rm.map(|v| (v * 10.0).round() / 10.0),
            "hold_secs": hold_secs,
            "bodyweight": (bw_for(&bw_log, today) as f64 * 10.0).round() / 10.0,
            "has_bodyweight": !bw_log.is_empty(),
            "pb": is_pb,
        }
    }))
}
