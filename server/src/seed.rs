use diesel::prelude::*;
use serde::Deserialize;

use crate::schema::exercises;

const SEED_JSON: &str = include_str!("../../shared/exercises.json");

#[derive(Deserialize)]
struct SeedFile {
    exercises: Vec<SeedExercise>,
}

#[derive(Deserialize)]
struct SeedExercise {
    id: i32,
    name: String,
    body_area: String,
    #[serde(default)]
    primary_muscles: Vec<String>,
    #[serde(default)]
    secondary_muscles: Vec<String>,
    #[serde(default)]
    default_timed: bool,
    #[serde(default)]
    load_factor: f32,
    #[serde(default)]
    profile: SeedProfile,
}

#[derive(Deserialize)]
#[serde(default)]
struct SeedProfile {
    axis: String,
    min_rep_ms: i32,
    smoothing: i32,
}

impl Default for SeedProfile {
    fn default() -> Self {
        SeedProfile { axis: "mag".to_string(), min_rep_ms: 900, smoothing: 5 }
    }
}

pub fn seed_exercises(conn: &mut SqliteConnection) -> Result<(), String> {
    let seed: SeedFile = serde_json::from_str(SEED_JSON).map_err(|e| e.to_string())?;
    for e in seed.exercises {
        diesel::insert_into(exercises::table)
            .values((
                exercises::watch_movement_id.eq(e.id),
                exercises::name.eq(&e.name),
                exercises::body_area.eq(&e.body_area),
                exercises::primary_muscles.eq(e.primary_muscles.join(", ")),
                exercises::secondary_muscles.eq(e.secondary_muscles.join(", ")),
                exercises::default_timed.eq(e.default_timed),
                exercises::profile_axis.eq(&e.profile.axis),
                exercises::profile_min_rep_ms.eq(e.profile.min_rep_ms),
                exercises::profile_smoothing.eq(e.profile.smoothing),
                exercises::is_builtin.eq(true),
                exercises::load_factor.eq(e.load_factor),
            ))
            .on_conflict(exercises::watch_movement_id)
            .do_update()
            .set((
                exercises::name.eq(&e.name),
                exercises::body_area.eq(&e.body_area),
                exercises::primary_muscles.eq(e.primary_muscles.join(", ")),
                exercises::secondary_muscles.eq(e.secondary_muscles.join(", ")),
                exercises::default_timed.eq(e.default_timed),
                exercises::profile_axis.eq(&e.profile.axis),
                exercises::profile_min_rep_ms.eq(e.profile.min_rep_ms),
                exercises::profile_smoothing.eq(e.profile.smoothing),
                exercises::load_factor.eq(e.load_factor),
            ))
            .execute(conn)
            .map_err(|err| format!("seeding {}: {err}", e.name))?;
    }
    Ok(())
}
