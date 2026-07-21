diesel::table! {
    users (id) {
        id -> Integer,
        google_sub -> Text,
        email -> Text,
        display_name -> Text,
        created_at -> Timestamp,
    }
}

diesel::table! {
    web_sessions (token_hash) {
        token_hash -> Text,
        user_id -> Integer,
        expires_at -> Timestamp,
    }
}

diesel::table! {
    devices (id) {
        id -> Integer,
        user_id -> Integer,
        token_hash -> Text,
        label -> Text,
        last_sync_at -> Nullable<Timestamp>,
        created_at -> Timestamp,
    }
}

diesel::table! {
    exercises (id) {
        id -> Integer,
        watch_movement_id -> Integer,
        name -> Text,
        body_area -> Text,
        primary_muscles -> Text,
        secondary_muscles -> Text,
        default_timed -> Bool,
        profile_axis -> Text,
        profile_min_rep_ms -> Integer,
        profile_smoothing -> Integer,
        is_builtin -> Bool,
    }
}

diesel::table! {
    workouts (id) {
        id -> Integer,
        owner_id -> Integer,
        title -> Text,
        description -> Text,
        is_public -> Bool,
        forked_from -> Nullable<Integer>,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

diesel::table! {
    workout_exercises (id) {
        id -> Integer,
        workout_id -> Integer,
        position -> Integer,
        exercise_id -> Integer,
        weight_kg -> Float,
        is_timed -> Bool,
        is_amrap -> Bool,
    }
}

diesel::table! {
    workout_sets (id) {
        id -> Integer,
        workout_exercise_id -> Integer,
        position -> Integer,
        target -> Integer,
        rest_secs -> Integer,
    }
}

diesel::table! {
    user_slots (user_id, slot) {
        user_id -> Integer,
        slot -> Integer,
        workout_id -> Integer,
    }
}

diesel::table! {
    recordings (id) {
        id -> Integer,
        user_id -> Integer,
        movement_id -> Integer,
        exercise_name -> Text,
        workout_name -> Text,
        set_index -> Integer,
        actual -> Integer,
        is_timed -> Bool,
        sample_rate -> Integer,
        sample_count -> Integer,
        truncated -> Bool,
        samples -> Binary,
        recorded_at -> Timestamp,
    }
}

diesel::table! {
    sessions (id) {
        id -> Integer,
        user_id -> Integer,
        workout_name -> Text,
        performed_on -> Timestamp,
        notes -> Text,
        created_at -> Timestamp,
    }
}

diesel::table! {
    session_sets (id) {
        id -> Integer,
        session_id -> Integer,
        position -> Integer,
        movement_id -> Integer,
        exercise_name -> Text,
        is_timed -> Bool,
        actual -> Integer,
        weight_kg -> Nullable<Float>,
        work_secs -> Nullable<Integer>,
        recording_id -> Nullable<Integer>,
        performed_at -> Timestamp,
    }
}

diesel::joinable!(web_sessions -> users (user_id));
diesel::joinable!(sessions -> users (user_id));
diesel::joinable!(session_sets -> sessions (session_id));
diesel::joinable!(devices -> users (user_id));
diesel::joinable!(workout_exercises -> workouts (workout_id));
diesel::joinable!(workout_exercises -> exercises (exercise_id));
diesel::joinable!(workout_sets -> workout_exercises (workout_exercise_id));
diesel::joinable!(user_slots -> workouts (workout_id));

diesel::allow_tables_to_appear_in_same_query!(
    users,
    web_sessions,
    devices,
    exercises,
    workouts,
    workout_exercises,
    workout_sets,
    user_slots,
    recordings,
    sessions,
    session_sets,
);
