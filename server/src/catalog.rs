//! User-owned exercises: the live counterpart to `shared/exercises.json`.
//!
//! The seed file stays the source of truth for *built-in* movements — the watch
//! generates its compiled-in table from it, so those ids are a build-time
//! contract. Anything a user adds at runtime lives only in the DB and is
//! allocated a `watch_movement_id` from a reserved high range, so the two
//! allocators can never collide.
//!
//! That range is allocated *per user*: built-ins are globally unique (they must
//! match the watch's compiled table), but two users can each hold id 128 for
//! different movements, because a watch only ever receives its own owner's
//! workouts. Everything that maps an id back to an exercise must therefore
//! scope by owner — see `device.rs` on the upload path.
//!
//! The watch tolerates an id it has no entry for: `counter_config_default()`
//! falls back to Custom(0) and `movement_name()` renders "Unknown". Names for
//! custom movements need the string pool of SPEC §4.2/§4.4 (`customNameIdx`),
//! which isn't built yet — but because the packed record carries the *real* id,
//! uploaded sets still resolve to the right exercise server-side.

use diesel::prelude::*;

use crate::error::AppError;
use crate::models::Exercise;
use crate::schema::{exercises, workout_exercises};

/// Built-in movements live below this; user-created ones from here up. The
/// packed format carries the id as a u8 (SPEC §4.2), so 128..=255 is the pool —
/// and every user gets the whole pool to themselves.
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

/// Lowest free id in this user's custom range.
///
/// The range is per-user, so everyone gets all 128 slots — a global sequence
/// would let one account exhaust a pool the rest share, and since custom
/// exercises are invisible to other users, running out would look inexplicable.
///
/// Deleting frees the id for reuse. A watch holding a stale packed workout that
/// still references a reused id would name the wrong movement, but only until
/// its next sync: deletion is refused while any saved workout references the
/// exercise, and logged history keeps its own copy of the name.
fn next_movement_id(conn: &mut SqliteConnection, user_id: i32) -> Result<i32, AppError> {
    let used: Vec<i32> = exercises::table
        .filter(exercises::owner_user_id.eq(user_id))
        .filter(exercises::watch_movement_id.ge(CUSTOM_MOVEMENT_BASE))
        .select(exercises::watch_movement_id)
        .load(conn)?;
    let used: std::collections::HashSet<i32> = used.into_iter().collect();
    (CUSTOM_MOVEMENT_BASE..=MAX_MOVEMENT_ID)
        .find(|id| !used.contains(id))
        .ok_or_else(|| {
            AppError::BadRequest(format!(
                "you've used all {} custom exercise slots — delete one to free an id",
                MAX_MOVEMENT_ID - CUSTOM_MOVEMENT_BASE + 1
            ))
        })
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
        let movement_id = next_movement_id(conn, user_id)?;
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
        // No counter_configs row: that table is keyed by watch_movement_id with
        // no owner, and custom ids now repeat across users. Nothing is lost —
        // /api/device/counters only ships configs with confidence > 0, so an
        // untuned custom movement always fell back to the watch's compiled-in
        // Custom(0) profile anyway. Tuning one will need an owner column there.
        Ok(movement_id)
    })
}

/// Resolve a movement id coming off a watch back to an exercise name.
///
/// Must always be owner-scoped: custom ids repeat across users, so an unscoped
/// lookup could name *another* user's movement. Built-ins (owner NULL) and this
/// user's own rows can't collide, since the two occupy disjoint id ranges.
/// Returns an empty string for an id we don't know, matching the previous
/// behaviour of these call sites.
pub fn movement_name(
    conn: &mut SqliteConnection,
    user_id: i32,
    movement_id: i32,
) -> Result<String, AppError> {
    Ok(exercises::table
        .filter(exercises::watch_movement_id.eq(movement_id))
        .filter(
            exercises::owner_user_id
                .is_null()
                .or(exercises::owner_user_id.eq(user_id)),
        )
        .select(exercises::name)
        .first(conn)
        .optional()?
        .unwrap_or_default())
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
        owned(conn, user_id, id)?;
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
        // The movement id returns to this user's free pool.
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
    fn the_custom_pool_is_a_full_128_slots() {
        assert_eq!(MAX_MOVEMENT_ID - CUSTOM_MOVEMENT_BASE + 1, 128);
        // Every id in the pool must survive the u8 the packed format uses.
        assert!(u8::try_from(MAX_MOVEMENT_ID).is_ok());
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
