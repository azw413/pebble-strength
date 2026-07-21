use chrono::{NaiveDate, NaiveDateTime};
use diesel::prelude::*;
use serde::Serialize;

use crate::schema::*;

#[derive(Queryable, Identifiable, Clone, Debug)]
#[diesel(table_name = users)]
pub struct User {
    pub id: i32,
    pub google_sub: String,
    pub email: String,
    pub display_name: String,
    pub created_at: NaiveDateTime,
}

#[derive(Queryable, Identifiable, Clone, Debug)]
#[diesel(table_name = devices)]
pub struct Device {
    pub id: i32,
    pub user_id: i32,
    pub token_hash: String,
    pub label: String,
    pub last_sync_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
}

#[derive(Queryable, Identifiable, Clone, Debug, Serialize)]
#[diesel(table_name = exercises)]
pub struct Exercise {
    pub id: i32,
    pub watch_movement_id: i32,
    pub name: String,
    pub body_area: String,
    pub primary_muscles: String,
    pub secondary_muscles: String,
    pub default_timed: bool,
    pub profile_axis: String,
    pub profile_min_rep_ms: i32,
    pub profile_smoothing: i32,
    pub is_builtin: bool,
    pub load_factor: f32,
}

#[derive(Queryable, Identifiable, Clone, Debug)]
#[diesel(table_name = workouts)]
pub struct Workout {
    pub id: i32,
    pub owner_id: i32,
    pub title: String,
    pub description: String,
    pub is_public: bool,
    pub forked_from: Option<i32>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Queryable, Identifiable, Clone, Debug)]
#[diesel(table_name = workout_exercises)]
pub struct WorkoutExercise {
    pub id: i32,
    pub workout_id: i32,
    pub position: i32,
    pub exercise_id: i32,
    pub weight_kg: f32,
    pub is_timed: bool,
    pub is_amrap: bool,
}

#[derive(Queryable, Identifiable, Clone, Debug)]
#[diesel(table_name = workout_sets)]
pub struct WorkoutSet {
    pub id: i32,
    pub workout_exercise_id: i32,
    pub position: i32,
    pub target: i32,
    pub rest_secs: i32,
}

#[derive(Queryable, Identifiable, Clone, Debug, Serialize)]
#[diesel(table_name = sessions)]
pub struct Session {
    pub id: i32,
    pub user_id: i32,
    pub workout_name: String,
    pub performed_on: NaiveDateTime,
    pub notes: String,
    pub created_at: NaiveDateTime,
}

#[derive(Queryable, Identifiable, Clone, Debug, Serialize)]
#[diesel(table_name = session_sets)]
pub struct SessionSet {
    pub id: i32,
    pub session_id: i32,
    pub position: i32,
    pub movement_id: i32,
    pub exercise_name: String,
    pub is_timed: bool,
    pub actual: i32,
    pub weight_kg: Option<f32>,
    pub work_secs: Option<i32>,
    pub recording_id: Option<i32>,
    pub performed_at: NaiveDateTime,
}

#[derive(Queryable, Identifiable, Clone, Debug, Serialize)]
#[diesel(table_name = bodyweights)]
pub struct Bodyweight {
    pub id: i32,
    pub user_id: i32,
    pub measured_on: NaiveDate,
    pub weight_kg: f32,
    pub created_at: NaiveDateTime,
}
