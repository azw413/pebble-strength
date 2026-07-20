//! Dev-mode sample data: the four sessions of the "Rings & Strength" programme,
//! seeded into watch slots 1–4 for the dev user on first run.

use std::collections::HashMap;

use diesel::prelude::*;

use crate::error::AppError;
use crate::schema::{exercises, workouts};
use crate::workouts::{ExerciseInput, SetInput, WorkoutInput};
use crate::{auth, workouts as wk};

fn reps(id: i32, weight_kg: f32, n: usize, target: i32, rest: i32) -> ExerciseInput {
    ExerciseInput {
        exercise_id: id,
        weight_kg,
        is_timed: false,
        is_amrap: false,
        sets: vec![SetInput { target, rest_secs: rest }; n],
    }
}

fn hold(id: i32, n: usize, secs: i32, rest: i32) -> ExerciseInput {
    ExerciseInput {
        exercise_id: id,
        weight_kg: 0.0,
        is_timed: true,
        is_amrap: false,
        sets: vec![SetInput { target: secs, rest_secs: rest }; n],
    }
}

pub fn ensure_dev_samples(conn: &mut SqliteConnection) -> Result<(), AppError> {
    let user = auth::upsert_user(conn, "dev:local", "dev@localhost", "Dev User")?;
    let existing: i64 = workouts::table
        .filter(workouts::owner_id.eq(user.id))
        .count()
        .get_result(conn)?;
    if existing > 0 {
        return Ok(());
    }

    let ids: HashMap<String, i32> = exercises::table
        .select((exercises::name, exercises::id))
        .load::<(String, i32)>(conn)?
        .into_iter()
        .collect();
    let ex = |name: &str| -> Result<i32, AppError> {
        ids.get(name)
            .copied()
            .ok_or_else(|| AppError::Internal(format!("sample data: missing exercise '{name}'")))
    };

    // Starting doses from the programme's phase 1: low end of each range,
    // ~90 s rest on easier sets, 150 s before hard pull/dip sets.
    let plans = vec![
        WorkoutInput {
            title: "Day A - Pull & Core".into(),
            description: "Rings & Strength — vertical + horizontal pulling, biceps, grip, trunk. \
                          Chin-ups: full range or 5 s negatives; add a rep whenever you can."
                .into(),
            is_public: false,
            slot: Some(1),
            exercises: vec![
                reps(ex("Chin-up")?, 0.0, 4, 2, 150),
                reps(ex("Ring Row")?, 0.0, 3, 8, 90),
                reps(ex("Biceps Curl")?, 12.0, 3, 10, 90),
                hold(ex("Hollow-body Hold")?, 3, 20, 90),
            ],
        },
        WorkoutInput {
            title: "Day B - Push & Core".into(),
            description: "Rings & Strength — horizontal + vertical pushing, ring support, triceps, \
                          trunk. Support hold: arms locked, rings turned slightly out; build toward \
                          45–60 s total. Dips: start from support, lower under control."
                .into(),
            is_public: false,
            slot: Some(2),
            exercises: vec![
                hold(ex("Ring Support Hold")?, 3, 15, 90),
                reps(ex("Push-up")?, 0.0, 3, 8, 90),
                reps(ex("Dip")?, 0.0, 3, 3, 150),
                reps(ex("Pike Push-up")?, 0.0, 3, 6, 90),
                reps(ex("Overhead Press")?, 12.0, 3, 6, 90),
                hold(ex("Plank")?, 3, 30, 60),
            ],
        },
        WorkoutInput {
            title: "Day C - Pull Legs Skill".into(),
            description: "Rings & Strength — pulling, straight-arm skill prep, lower body. Pull-ups: \
                          assisted / negatives on the road to the first strict rep. Skill holds fresh \
                          and short, never to fatigue. Split squats are per leg."
                .into(),
            is_public: false,
            slot: Some(3),
            exercises: vec![
                reps(ex("Pull-up")?, 0.0, 4, 2, 150),
                hold(ex("Tuck Front Lever")?, 3, 8, 90),
                hold(ex("German Hang")?, 3, 10, 90),
                reps(ex("Bulgarian Split Squat")?, 12.0, 3, 8, 90),
                reps(ex("Romanian Deadlift")?, 12.0, 3, 10, 90),
                hold(ex("Dead Hang")?, 3, 20, 90),
            ],
        },
        WorkoutInput {
            title: "Day D - Push Legs Skill".into(),
            description: "Rings & Strength — pushing, planche/cross prep, lower body. Planche lean: \
                          hands by waist, lean forward, protract. Pistols: box or ring-assisted, per \
                          leg. Hollow rocks timed."
                .into(),
            is_public: false,
            slot: Some(4),
            exercises: vec![
                hold(ex("Planche Lean")?, 3, 10, 90),
                reps(ex("Pseudo-planche Push-up")?, 0.0, 3, 6, 90),
                reps(ex("Dip")?, 0.0, 3, 3, 150),
                reps(ex("Goblet Squat")?, 12.0, 3, 12, 90),
                reps(ex("Pistol Squat")?, 0.0, 3, 3, 90),
                reps(ex("Calf Raise")?, 0.0, 3, 15, 60),
                hold(ex("Hollow Rock")?, 3, 20, 60),
            ],
        },
    ];

    for plan in &plans {
        wk::save(conn, user.id, None, plan)?;
    }
    eprintln!("note: seeded {} Rings & Strength sample workouts for dev@localhost", plans.len());
    Ok(())
}
