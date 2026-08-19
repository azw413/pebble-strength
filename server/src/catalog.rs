//! User-owned exercises: the live counterpart to `shared/exercises.json`.
//!
//! The seed file stays the source of truth for *built-in* movements — the watch
//! generates its compiled-in table from it, so those ids are a build-time
//! contract. Anything a user adds at runtime lives only in the DB and is
//! allocated a `watch_movement_id` from a reserved high range, so the two
//! allocators can never collide.
//!
//! The watch tolerates an id it has no entry for: `counter_config_default()`
//! falls back to Custom(0) and `movement_name()` renders "Unknown". Names for
//! custom movements need the string pool of SPEC §4.2/§4.4 (`customNameIdx`),
//! which isn't built yet — but because the packed record carries the *real* id,
//! uploaded sets still resolve to the right exercise server-side.

use diesel::prelude::*;

use crate::error::AppError;
use crate::models::Exercise;
use crate::schema::{counter_configs, exercises, workout_exercises};

/// Built-in movements live below this; user-created ones from here up. The
/// packed format carries the id as a u8 (SPEC §4.2), so 128..=255 is the pool.
pub const CUSTOM_MOVEMENT_BASE: i32 = 128;
pub const MAX_MOVEMENT_ID: i32 = 255;

/// Custom names are destined for the 256 B on-watch string pool, so keep them
/// short enough to be displayable there once that rail lands.
pub const MAX_NAME_LEN: usize = 24;

/// Movement patterns, in the order the filter chips appear.
pub const CATEGORIES: [(&str, &str); 6] = [
    ("push", "Push"),
    ("pull", "Pull"),
    ("hinge", "Hinge"),
    ("squat", "Squat"),
    ("core", "Core"),
    ("other", "Other"),
];

pub const BODY_AREAS: &[&str] =
    &["chest", "back", "shoulders", "arms", "legs", "core", "cardio", "other"];

pub const EQUIPMENT: &[&str] = &[
    "barbell",
    "dumbbell",
    "kettlebell",
    "machine",
    "cable",
    "rings",
    "band",
    "bodyweight",
];

/// The muscle vocabulary the body map understands (mirrors `MAP` in
/// exercises.html and `HMAP` in dashboard.html). A token outside this list
/// would simply never light up a region, so the form only offers these.
pub const MUSCLES: &[&str] = &[
    "pecs",
    "upper pecs",
    "lower pecs",
    "chest",
    "delts",
    "front delts",
    "side delts",
    "shoulders",
    "rear delts",
    "biceps",
    "brachialis",
    "triceps",
    "forearms",
    "grip",
    "wrists",
    "lats",
    "rhomboids",
    "upper back",
    "traps",
    "upper traps",
    "erectors",
    "abs",
    "core",
    "hip flexors",
    "obliques",
    "quads",
    "glutes",
    "hamstrings",
    "calves",
];

pub fn cat_label(key: &str) -> String {
    CATEGORIES
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, l)| l.to_string())
        .unwrap_or_else(|| key.to_string())
}

/// Form payload for creating or editing a custom exercise. Muscle lists arrive
/// as comma-separated tokens (the chip picker writes them into hidden inputs).
#[derive(serde::Deserialize)]
pub struct ExerciseForm {
    pub name: String,
    pub body_area: String,
    pub category: String,
    pub equipment: String,
    #[serde(default)]
    pub primary_muscles: String,
    #[serde(default)]
    pub secondary_muscles: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub load_factor: String,
    // Checkboxes: present ("on") when ticked, absent otherwise.
    #[serde(default)]
    pub default_timed: Option<String>,
    #[serde(default)]
    pub loadable: Option<String>,
    #[serde(default)]
    pub unilateral: Option<String>,
}

/// A validated exercise, ready to write.
#[derive(Debug)]
pub struct Validated {
    pub name: String,
    pub body_area: String,
    pub category: String,
    pub equipment: String,
    pub primary_muscles: String,
    pub secondary_muscles: String,
    pub description: String,
    pub load_factor: f32,
    pub default_timed: bool,
    pub loadable: bool,
    pub unilateral: bool,
}

fn checked(v: &Option<String>) -> bool {
    v.is_some()
}

/// Split a comma-separated muscle list, rejecting anything the body map can't
/// render. Order is preserved and duplicates are dropped.
fn parse_muscles(raw: &str, label: &str) -> Result<String, String> {
    let mut out: Vec<String> = Vec::new();
    for tok in raw.split(',') {
        let t = tok.trim().to_lowercase();
        if t.is_empty() {
            continue;
        }
        if !MUSCLES.contains(&t.as_str()) {
            return Err(format!("{label}: \"{t}\" is not a known muscle"));
        }
        if !out.contains(&t) {
            out.push(t);
        }
    }
    if out.len() > 6 {
        return Err(format!("{label}: at most 6 muscles"));
    }
    Ok(out.join(", "))
}

pub fn validate(form: &ExerciseForm) -> Result<Validated, String> {
    let name = form.name.trim().to_string();
    if name.is_empty() {
        return Err("name is required".into());
    }
    if name.chars().count() > MAX_NAME_LEN {
        return Err(format!("name must be {MAX_NAME_LEN} characters or fewer"));
    }
    if !BODY_AREAS.contains(&form.body_area.as_str()) {
        return Err("unknown body area".into());
    }
    if !CATEGORIES.iter().any(|(k, _)| *k == form.category) {
        return Err("unknown movement pattern".into());
    }
    if !EQUIPMENT.contains(&form.equipment.as_str()) {
        return Err("unknown equipment".into());
    }
    if form.description.chars().count() > 500 {
        return Err("description must be 500 characters or fewer".into());
    }

    let primary = parse_muscles(&form.primary_muscles, "primary movers")?;
    if primary.is_empty() {
        return Err("pick at least one primary mover".into());
    }
    let secondary = parse_muscles(&form.secondary_muscles, "secondary muscles")?;

    let lf_raw = form.load_factor.trim();
    let load_factor: f32 = if lf_raw.is_empty() {
        0.0
    } else {
        lf_raw.parse().map_err(|_| "bodyweight load must be a number".to_string())?
    };
    if !(0.0..=1.5).contains(&load_factor) {
        return Err("bodyweight load must be between 0 and 1.5".into());
    }

    Ok(Validated {
        name,
        body_area: form.body_area.clone(),
        category: form.category.clone(),
        equipment: form.equipment.clone(),
        primary_muscles: primary,
        secondary_muscles: secondary,
        description: form.description.trim().to_string(),
        load_factor,
        default_timed: checked(&form.default_timed),
        loadable: checked(&form.loadable),
        unilateral: checked(&form.unilateral),
    })
}

/// Next free id in the custom range. Ids are never reused — a deleted movement's
/// id stays retired, so old packed workouts and recordings can't be misread.
fn next_movement_id(conn: &mut SqliteConnection) -> Result<i32, AppError> {
    let highest: Option<i32> = exercises::table
        .filter(exercises::watch_movement_id.ge(CUSTOM_MOVEMENT_BASE))
        .select(diesel::dsl::max(exercises::watch_movement_id))
        .first(conn)?;
    let next = highest.map_or(CUSTOM_MOVEMENT_BASE, |m| m + 1);
    if next > MAX_MOVEMENT_ID {
        return Err(AppError::BadRequest(
            "no movement ids left — the packed format allows 128 custom exercises".into(),
        ));
    }
    Ok(next)
}

/// Reject a name that already exists in the user's view of the catalog, so the
/// builder's dropdown never shows two identical entries.
fn check_name_free(
    conn: &mut SqliteConnection,
    user_id: i32,
    name: &str,
    except: Option<i32>,
) -> Result<(), AppError> {
    let mut q = exercises::table
        .filter(exercises::name.eq(name))
        .filter(
            exercises::owner_user_id
                .is_null()
                .or(exercises::owner_user_id.eq(user_id)),
        )
        .into_boxed();
    if let Some(id) = except {
        q = q.filter(exercises::id.ne(id));
    }
    let clash: i64 = q.count().get_result(conn)?;
    if clash > 0 {
        return Err(AppError::BadRequest(format!("\"{name}\" is already in your catalog")));
    }
    Ok(())
}

pub fn create(
    conn: &mut SqliteConnection,
    user_id: i32,
    v: &Validated,
) -> Result<i32, AppError> {
    conn.transaction(|conn| {
        check_name_free(conn, user_id, &v.name, None)?;
        let movement_id = next_movement_id(conn)?;
        diesel::insert_into(exercises::table)
            .values((
                exercises::watch_movement_id.eq(movement_id),
                exercises::name.eq(&v.name),
                exercises::body_area.eq(&v.body_area),
                exercises::primary_muscles.eq(&v.primary_muscles),
                exercises::secondary_muscles.eq(&v.secondary_muscles),
                exercises::default_timed.eq(v.default_timed),
                exercises::category.eq(&v.category),
                exercises::equipment.eq(&v.equipment),
                exercises::loadable.eq(v.loadable),
                exercises::unilateral.eq(v.unilateral),
                exercises::description.eq(&v.description),
                exercises::is_builtin.eq(false),
                exercises::load_factor.eq(v.load_factor),
                exercises::owner_user_id.eq(user_id),
            ))
            .execute(conn)?;

        // Baseline counter config, mirroring seed.rs. confidence stays 0.0, so
        // /api/device/counters won't ship it — the watch counts a custom
        // movement with its compiled-in Custom(0) profile until we tune one.
        diesel::insert_into(counter_configs::table)
            .values((
                counter_configs::watch_movement_id.eq(movement_id),
                counter_configs::version.eq(1),
                counter_configs::active.eq(true),
                counter_configs::kind.eq(0),
                counter_configs::axis_mode.eq(0),
                counter_configs::lp_ms.eq(500),
                counter_configs::hp_ms.eq(3000),
                counter_configs::thr_pct.eq(40),
                counter_configs::min_rep_ms.eq(900),
                counter_configs::min_amp.eq(150),
                counter_configs::warmup_ms.eq(0),
                counter_configs::confidence.eq(0.0f32),
                counter_configs::enabled.eq(!v.default_timed),
            ))
            .execute(conn)?;
        Ok(movement_id)
    })
}

/// Load a custom exercise the user owns, or 404. Built-ins are never editable.
pub fn owned(conn: &mut SqliteConnection, user_id: i32, id: i32) -> Result<Exercise, AppError> {
    exercises::table
        .filter(exercises::id.eq(id))
        .filter(exercises::owner_user_id.eq(user_id))
        .first(conn)
        .optional()?
        .ok_or(AppError::NotFound)
}

pub fn update(
    conn: &mut SqliteConnection,
    user_id: i32,
    id: i32,
    v: &Validated,
) -> Result<(), AppError> {
    conn.transaction(|conn| {
        let ex = owned(conn, user_id, id)?;
        check_name_free(conn, user_id, &v.name, Some(id))?;
        diesel::update(exercises::table.find(id))
            .set((
                exercises::name.eq(&v.name),
                exercises::body_area.eq(&v.body_area),
                exercises::primary_muscles.eq(&v.primary_muscles),
                exercises::secondary_muscles.eq(&v.secondary_muscles),
                exercises::default_timed.eq(v.default_timed),
                exercises::category.eq(&v.category),
                exercises::equipment.eq(&v.equipment),
                exercises::loadable.eq(v.loadable),
                exercises::unilateral.eq(v.unilateral),
                exercises::description.eq(&v.description),
                exercises::load_factor.eq(v.load_factor),
            ))
            .execute(conn)?;
        // A hold has no rep counter to run; keep that in step with the edit.
        diesel::update(
            counter_configs::table
                .filter(counter_configs::watch_movement_id.eq(ex.watch_movement_id)),
        )
        .set(counter_configs::enabled.eq(!v.default_timed))
        .execute(conn)?;
        Ok(())
    })
}

pub fn delete(conn: &mut SqliteConnection, user_id: i32, id: i32) -> Result<(), AppError> {
    conn.transaction(|conn| {
        let ex = owned(conn, user_id, id)?;
        // Workouts reference exercises by row id; removing one out from under a
        // saved workout would leave it unpackable, so ask the user to unpick it.
        let used: i64 = workout_exercises::table
            .filter(workout_exercises::exercise_id.eq(id))
            .count()
            .get_result(conn)?;
        if used > 0 {
            return Err(AppError::BadRequest(format!(
                "{} is used by {used} workout exercise(s) — remove it from those workouts first",
                ex.name
            )));
        }
        diesel::delete(
            counter_configs::table
                .filter(counter_configs::watch_movement_id.eq(ex.watch_movement_id)),
        )
        .execute(conn)?;
        diesel::delete(exercises::table.find(id)).execute(conn)?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(name: &str) -> ExerciseForm {
        ExerciseForm {
            name: name.to_string(),
            body_area: "arms".to_string(),
            category: "push".to_string(),
            equipment: "dumbbell".to_string(),
            primary_muscles: "triceps".to_string(),
            secondary_muscles: String::new(),
            description: String::new(),
            load_factor: String::new(),
            default_timed: None,
            loadable: Some("on".to_string()),
            unilateral: None,
        }
    }

    #[test]
    fn accepts_a_plain_exercise() {
        let v = validate(&form("Triceps Kickback")).unwrap();
        assert_eq!(v.name, "Triceps Kickback");
        assert_eq!(v.primary_muscles, "triceps");
        assert!(v.loadable && !v.unilateral && !v.default_timed);
        assert_eq!(v.load_factor, 0.0);
    }

    #[test]
    fn rejects_unknown_muscle() {
        let mut f = form("Sternum Twist");
        f.primary_muscles = "sternum".to_string();
        assert!(validate(&f).unwrap_err().contains("not a known muscle"));
    }

    #[test]
    fn rejects_name_too_long_for_the_watch() {
        let f = form("Seated Incline Dumbbell Rear Delt Fly");
        assert!(validate(&f).unwrap_err().contains("24 characters"));
    }

    #[test]
    fn normalises_and_dedupes_muscles() {
        let mut f = form("Pullover");
        f.primary_muscles = " Lats , lats,  PECS ".to_string();
        assert_eq!(validate(&f).unwrap().primary_muscles, "lats, pecs");
    }

    #[test]
    fn rejects_out_of_range_load_factor() {
        let mut f = form("Weird Hold");
        f.load_factor = "2.0".to_string();
        assert!(validate(&f).unwrap_err().contains("between 0 and 1.5"));
    }

    #[test]
    fn custom_ids_never_overlap_the_seed() {
        // The seed file is the build-time contract with the watch; every id in
        // it must stay below the runtime range or the two allocators collide.
        let seed: serde_json::Value =
            serde_json::from_str(include_str!("../../shared/exercises.json")).unwrap();
        for e in seed["exercises"].as_array().unwrap() {
            let id = e["id"].as_i64().unwrap() as i32;
            assert!(id < CUSTOM_MOVEMENT_BASE, "seed id {id} is in the custom range");
        }
    }
}
