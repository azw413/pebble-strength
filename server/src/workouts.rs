use std::collections::HashMap;

use chrono::Utc;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::models::{Exercise, Workout, WorkoutExercise, WorkoutSet};
use crate::pack::{self, PackExercise, PackSet};
use crate::schema::{exercises, user_slots, workout_exercises, workout_sets, workouts};

pub const MAX_SLOT: i32 = 5;

#[derive(Deserialize, Serialize, Clone)]
pub struct WorkoutInput {
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub is_public: bool,
    #[serde(default)]
    pub slot: Option<i32>,
    pub exercises: Vec<ExerciseInput>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct ExerciseInput {
    pub exercise_id: i32,
    #[serde(default)]
    pub weight_kg: f32,
    #[serde(default)]
    pub is_timed: bool,
    #[serde(default)]
    pub is_amrap: bool,
    pub sets: Vec<SetInput>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct SetInput {
    pub target: i32,
    pub rest_secs: i32,
}

/// Full detail rows for a set of workouts, grouped and ordered.
type DetailMap = HashMap<i32, Vec<(WorkoutExercise, Exercise, Vec<WorkoutSet>)>>;

pub fn load_details(
    conn: &mut SqliteConnection,
    workout_ids: &[i32],
) -> Result<DetailMap, AppError> {
    let wes: Vec<(WorkoutExercise, Exercise)> = workout_exercises::table
        .inner_join(exercises::table)
        .filter(workout_exercises::workout_id.eq_any(workout_ids))
        .order((workout_exercises::workout_id.asc(), workout_exercises::position.asc()))
        .load(conn)?;
    let we_ids: Vec<i32> = wes.iter().map(|(we, _)| we.id).collect();
    let sets: Vec<WorkoutSet> = workout_sets::table
        .filter(workout_sets::workout_exercise_id.eq_any(&we_ids))
        .order(workout_sets::position.asc())
        .load(conn)?;
    let mut sets_by_we: HashMap<i32, Vec<WorkoutSet>> = HashMap::new();
    for s in sets {
        sets_by_we.entry(s.workout_exercise_id).or_default().push(s);
    }
    let mut out: DetailMap = HashMap::new();
    for (we, ex) in wes {
        let s = sets_by_we.remove(&we.id).unwrap_or_default();
        out.entry(we.workout_id).or_default().push((we, ex, s));
    }
    Ok(out)
}

pub fn to_pack_exercises(rows: &[(WorkoutExercise, Exercise, Vec<WorkoutSet>)]) -> Vec<PackExercise> {
    rows.iter()
        .map(|(we, ex, sets)| PackExercise {
            movement_id: ex.watch_movement_id as u8,
            timed: we.is_timed,
            amrap: we.is_amrap,
            weight_q: (we.weight_kg * 4.0).round().max(0.0) as u16,
            sets: sets
                .iter()
                .map(|s| PackSet { target: s.target.clamp(0, 255) as u8, rest_secs: s.rest_secs.clamp(0, 1275) as u16 })
                .collect(),
        })
        .collect()
}

pub fn to_input(
    rows: &[(WorkoutExercise, Exercise, Vec<WorkoutSet>)],
    w: &Workout,
    slot: Option<i32>,
) -> WorkoutInput {
    WorkoutInput {
        title: w.title.clone(),
        description: w.description.clone(),
        is_public: w.is_public,
        slot,
        exercises: rows
            .iter()
            .map(|(we, _, sets)| ExerciseInput {
                exercise_id: we.exercise_id,
                weight_kg: we.weight_kg,
                is_timed: we.is_timed,
                is_amrap: we.is_amrap,
                sets: sets
                    .iter()
                    .map(|s| SetInput { target: s.target, rest_secs: s.rest_secs })
                    .collect(),
            })
            .collect(),
    }
}

fn validate(input: &WorkoutInput, by_id: &HashMap<i32, Exercise>) -> Result<(), String> {
    let title = input.title.trim();
    if title.is_empty() || title.chars().count() > 64 {
        return Err("title must be 1–64 characters".into());
    }
    if input.description.chars().count() > 2000 {
        return Err("description too long".into());
    }
    if let Some(slot) = input.slot {
        if !(1..=MAX_SLOT).contains(&slot) {
            return Err(format!("slot must be 1–{MAX_SLOT}"));
        }
    }
    if input.exercises.is_empty() || input.exercises.len() > pack::MAX_EXERCISES {
        return Err(format!("workout must have 1–{} exercises", pack::MAX_EXERCISES));
    }
    for e in &input.exercises {
        let Some(ex) = by_id.get(&e.exercise_id) else {
            return Err("unknown exercise".into());
        };
        if e.sets.is_empty() || e.sets.len() > pack::MAX_SETS {
            return Err(format!("{}: each exercise needs 1–{} sets", ex.name, pack::MAX_SETS));
        }
        if !(0.0..=500.0).contains(&e.weight_kg) {
            return Err(format!("{}: weight must be 0–500 kg", ex.name));
        }
        for s in &e.sets {
            let max_target = if e.is_timed { 255 } else { 100 };
            let min_target = if e.is_amrap { 0 } else { 1 };
            if s.target < min_target || s.target > max_target {
                return Err(format!(
                    "{}: set target must be {min_target}–{max_target} {}",
                    ex.name,
                    if e.is_timed { "seconds" } else { "reps" }
                ));
            }
            if !(0..=1275).contains(&s.rest_secs) {
                return Err(format!("{}: rest must be 0–1275 s", ex.name));
            }
        }
    }
    Ok(())
}

fn pack_input(input: &WorkoutInput, by_id: &HashMap<i32, Exercise>) -> Result<Vec<u8>, String> {
    let pes: Vec<PackExercise> = input
        .exercises
        .iter()
        .map(|e| {
            let ex = &by_id[&e.exercise_id];
            PackExercise {
                movement_id: ex.watch_movement_id as u8,
                timed: e.is_timed,
                amrap: e.is_amrap,
                weight_q: (e.weight_kg * 4.0).round().max(0.0) as u16,
                sets: e
                    .sets
                    .iter()
                    .map(|s| PackSet { target: s.target as u8, rest_secs: s.rest_secs as u16 })
                    .collect(),
            }
        })
        .collect();
    pack::pack_workout(input.title.trim(), &pes)
}

/// Create or update a workout (with its exercises, sets, and slot assignment).
/// Returns (workout id, packed size).
pub fn save(
    conn: &mut SqliteConnection,
    user_id: i32,
    existing: Option<i32>,
    input: &WorkoutInput,
) -> Result<(i32, usize), AppError> {
    let ex_ids: Vec<i32> = input.exercises.iter().map(|e| e.exercise_id).collect();
    let known: Vec<Exercise> = exercises::table
        .filter(exercises::id.eq_any(&ex_ids))
        .load(conn)?;
    let by_id: HashMap<i32, Exercise> = known.into_iter().map(|e| (e.id, e)).collect();

    validate(input, &by_id).map_err(AppError::BadRequest)?;
    let packed = pack_input(input, &by_id).map_err(AppError::BadRequest)?;
    let size = packed.len();

    let title = input.title.trim().to_string();
    conn.transaction::<_, AppError, _>(|conn| {
        let wid = match existing {
            Some(id) => {
                let owned: Option<Workout> = workouts::table
                    .filter(workouts::id.eq(id))
                    .filter(workouts::owner_id.eq(user_id))
                    .first(conn)
                    .optional()?;
                if owned.is_none() {
                    return Err(AppError::NotFound);
                }
                diesel::update(workouts::table.find(id))
                    .set((
                        workouts::title.eq(&title),
                        workouts::description.eq(&input.description),
                        workouts::is_public.eq(input.is_public),
                        workouts::updated_at.eq(Utc::now().naive_utc()),
                    ))
                    .execute(conn)?;
                diesel::delete(
                    workout_exercises::table.filter(workout_exercises::workout_id.eq(id)),
                )
                .execute(conn)?;
                id
            }
            None => diesel::insert_into(workouts::table)
                .values((
                    workouts::owner_id.eq(user_id),
                    workouts::title.eq(&title),
                    workouts::description.eq(&input.description),
                    workouts::is_public.eq(input.is_public),
                ))
                .returning(workouts::id)
                .get_result::<i32>(conn)?,
        };

        for (pos, e) in input.exercises.iter().enumerate() {
            let we_id: i32 = diesel::insert_into(workout_exercises::table)
                .values((
                    workout_exercises::workout_id.eq(wid),
                    workout_exercises::position.eq(pos as i32),
                    workout_exercises::exercise_id.eq(e.exercise_id),
                    workout_exercises::weight_kg.eq(e.weight_kg),
                    workout_exercises::is_timed.eq(e.is_timed),
                    workout_exercises::is_amrap.eq(e.is_amrap),
                ))
                .returning(workout_exercises::id)
                .get_result(conn)?;
            for (spos, s) in e.sets.iter().enumerate() {
                diesel::insert_into(workout_sets::table)
                    .values((
                        workout_sets::workout_exercise_id.eq(we_id),
                        workout_sets::position.eq(spos as i32),
                        workout_sets::target.eq(s.target),
                        workout_sets::rest_secs.eq(s.rest_secs),
                    ))
                    .execute(conn)?;
            }
        }

        // Slot assignment: this workout vacates any slot it held; if a slot was
        // chosen it takes it over (bumping whatever occupied it).
        diesel::delete(
            user_slots::table
                .filter(user_slots::user_id.eq(user_id))
                .filter(user_slots::workout_id.eq(wid)),
        )
        .execute(conn)?;
        if let Some(slot) = input.slot {
            diesel::delete(
                user_slots::table
                    .filter(user_slots::user_id.eq(user_id))
                    .filter(user_slots::slot.eq(slot)),
            )
            .execute(conn)?;
            diesel::insert_into(user_slots::table)
                .values((
                    user_slots::user_id.eq(user_id),
                    user_slots::slot.eq(slot),
                    user_slots::workout_id.eq(wid),
                ))
                .execute(conn)?;
        }

        Ok((wid, size))
    })
}

pub fn slot_map(conn: &mut SqliteConnection, user_id: i32) -> Result<HashMap<i32, i32>, AppError> {
    // workout_id -> slot
    let rows: Vec<(i32, i32)> = user_slots::table
        .filter(user_slots::user_id.eq(user_id))
        .select((user_slots::workout_id, user_slots::slot))
        .load(conn)?;
    Ok(rows.into_iter().collect())
}

/// Pack every slot-assigned workout for a user: (slot, title, bytes).
pub fn packed_slots(
    conn: &mut SqliteConnection,
    user_id: i32,
) -> Result<Vec<(i32, String, Vec<u8>)>, AppError> {
    let rows: Vec<(i32, Workout)> = user_slots::table
        .inner_join(workouts::table)
        .filter(user_slots::user_id.eq(user_id))
        .order(user_slots::slot.asc())
        .select((user_slots::slot, workouts::all_columns))
        .load(conn)?;
    let ids: Vec<i32> = rows.iter().map(|(_, w)| w.id).collect();
    let mut details = load_details(conn, &ids)?;
    let mut out = Vec::new();
    for (slot, w) in rows {
        let rows = details.remove(&w.id).unwrap_or_default();
        let packed = pack::pack_workout(&w.title, &to_pack_exercises(&rows))
            .map_err(AppError::Internal)?;
        out.push((slot, w.title, packed));
    }
    Ok(out)
}
