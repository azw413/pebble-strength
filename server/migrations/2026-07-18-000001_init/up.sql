CREATE TABLE users (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  google_sub TEXT NOT NULL UNIQUE,
  email TEXT NOT NULL,
  display_name TEXT NOT NULL DEFAULT '',
  created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE web_sessions (
  token_hash TEXT NOT NULL PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  expires_at TIMESTAMP NOT NULL
);

CREATE TABLE devices (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  token_hash TEXT NOT NULL UNIQUE,
  label TEXT NOT NULL DEFAULT 'Pebble',
  last_sync_at TIMESTAMP,
  created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE exercises (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  watch_movement_id INTEGER NOT NULL UNIQUE,
  name TEXT NOT NULL,
  body_area TEXT NOT NULL,
  primary_muscles TEXT NOT NULL DEFAULT '',
  secondary_muscles TEXT NOT NULL DEFAULT '',
  default_timed BOOLEAN NOT NULL DEFAULT 0,
  profile_axis TEXT NOT NULL DEFAULT 'mag',
  profile_min_rep_ms INTEGER NOT NULL DEFAULT 900,
  profile_smoothing INTEGER NOT NULL DEFAULT 5,
  is_builtin BOOLEAN NOT NULL DEFAULT 1
);

CREATE TABLE workouts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  owner_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  title TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  is_public BOOLEAN NOT NULL DEFAULT 0,
  forked_from INTEGER REFERENCES workouts(id) ON DELETE SET NULL,
  created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE workout_exercises (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  workout_id INTEGER NOT NULL REFERENCES workouts(id) ON DELETE CASCADE,
  position INTEGER NOT NULL,
  exercise_id INTEGER NOT NULL REFERENCES exercises(id),
  weight_kg FLOAT NOT NULL DEFAULT 0,
  is_timed BOOLEAN NOT NULL DEFAULT 0,
  is_amrap BOOLEAN NOT NULL DEFAULT 0
);

CREATE TABLE workout_sets (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  workout_exercise_id INTEGER NOT NULL REFERENCES workout_exercises(id) ON DELETE CASCADE,
  position INTEGER NOT NULL,
  target INTEGER NOT NULL,
  rest_secs INTEGER NOT NULL
);

CREATE TABLE user_slots (
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  slot INTEGER NOT NULL CHECK (slot BETWEEN 1 AND 5),
  workout_id INTEGER NOT NULL REFERENCES workouts(id) ON DELETE CASCADE,
  PRIMARY KEY (user_id, slot)
);

CREATE INDEX idx_workout_exercises_workout ON workout_exercises(workout_id);
CREATE INDEX idx_workout_sets_we ON workout_sets(workout_exercise_id);
CREATE INDEX idx_workouts_owner ON workouts(owner_id);
