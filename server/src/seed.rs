use diesel::prelude::*;
use serde::Deserialize;

use crate::schema::{counter_configs, exercises};

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
    #[serde(default = "default_category")]
    category: String,
    #[serde(default = "default_equipment")]
    equipment: String,
    #[serde(default)]
    loadable: bool,
    #[serde(default)]
    unilateral: bool,
    #[serde(default)]
    description: String,
    #[serde(default)]
    profile: SeedProfile,
}

fn default_category() -> String {
    "other".to_string()
}
fn default_equipment() -> String {
    "bodyweight".to_string()
}

// Full parametric CounterConfig, as carried in shared/exercises.json. Defaults
// match the counter_configs column defaults, so a movement that only specifies
// e.g. min_rep_ms still gets a complete, sane config.
#[derive(Deserialize)]
#[serde(default)]
struct SeedProfile {
    axis: String,
    lp_ms: i32,
    hp_ms: i32,
    thr_pct: i32,
    min_rep_ms: i32,
    min_amp: i32,
    warmup_ms: i32,
    confidence: f32,
}

impl Default for SeedProfile {
    fn default() -> Self {
        SeedProfile {
            axis: "mag".to_string(),
            lp_ms: 500,
            hp_ms: 3000,
            thr_pct: 40,
            min_rep_ms: 900,
            min_amp: 150,
            warmup_ms: 0,
            confidence: 0.0,
        }
    }
}

/// axis label -> CounterConfig.axis_mode (0 auto / 1 x / 2 y / 3 z / 4 |linear|)
fn axis_mode(axis: &str) -> i32 {
    match axis {
        "x" => 1,
        "y" => 2,
        "z" => 3,
        "mag" | "linear" => 4,
        _ => 0, // "auto" / anything else -> pick the max-variance axis on device
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
                exercises::category.eq(&e.category),
                exercises::equipment.eq(&e.equipment),
                exercises::loadable.eq(e.loadable),
                exercises::unilateral.eq(e.unilateral),
                exercises::description.eq(&e.description),
                exercises::is_builtin.eq(true),
                exercises::load_factor.eq(e.load_factor),
            ))
            .on_conflict(exercises::watch_movement_id)
            .do_update()
            // Refresh the catalog facts on every boot; leave prescription defaults
            // (min/max/default_reps, rest) and owner untouched so manual edits stick.
            .set((
                exercises::name.eq(&e.name),
                exercises::body_area.eq(&e.body_area),
                exercises::primary_muscles.eq(e.primary_muscles.join(", ")),
                exercises::secondary_muscles.eq(e.secondary_muscles.join(", ")),
                exercises::default_timed.eq(e.default_timed),
                exercises::category.eq(&e.category),
                exercises::equipment.eq(&e.equipment),
                exercises::loadable.eq(e.loadable),
                exercises::unilateral.eq(e.unilateral),
                exercises::description.eq(&e.description),
                exercises::load_factor.eq(e.load_factor),
            ))
            .execute(conn)
            .map_err(|err| format!("seeding {}: {err}", e.name))?;

        // Baseline (v1) parametric counter config from the JSON profile. Seed once;
        // never clobber — once tuned in the DB (or superseded by a newer version),
        // seeding leaves it alone.
        diesel::insert_into(counter_configs::table)
            .values((
                counter_configs::watch_movement_id.eq(e.id),
                counter_configs::version.eq(1),
                counter_configs::active.eq(true),
                counter_configs::kind.eq(0),
                counter_configs::axis_mode.eq(axis_mode(&e.profile.axis)),
                counter_configs::lp_ms.eq(e.profile.lp_ms),
                counter_configs::hp_ms.eq(e.profile.hp_ms),
                counter_configs::thr_pct.eq(e.profile.thr_pct),
                counter_configs::min_rep_ms.eq(e.profile.min_rep_ms),
                counter_configs::min_amp.eq(e.profile.min_amp),
                counter_configs::warmup_ms.eq(e.profile.warmup_ms),
                counter_configs::confidence.eq(e.profile.confidence),
                // enabled only for rep movements; a hold has no rep counter to run.
                counter_configs::enabled.eq(!e.default_timed),
            ))
            .on_conflict((counter_configs::watch_movement_id, counter_configs::version))
            .do_nothing()
            .execute(conn)
            .map_err(|err| format!("seeding counter for {}: {err}", e.name))?;
    }
    Ok(())
}
