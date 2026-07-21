-- Logged workout sessions: what was actually performed, as opposed to the
-- workout *definition* in workouts/workout_exercises/workout_sets. A session is
-- the workout's structure filled in with actual reps/holds, weight, and work
-- time. Sets are auto-logged from watch recording uploads and grouped into a
-- session by user + workout name + upload-time proximity (see sessions.rs).

CREATE TABLE sessions (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  workout_name TEXT NOT NULL DEFAULT '',
  performed_on TIMESTAMP NOT NULL,
  notes TEXT NOT NULL DEFAULT '',
  created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_sessions_user ON sessions(user_id, performed_on);

CREATE TABLE session_sets (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  position INTEGER NOT NULL,          -- order within the session
  movement_id INTEGER NOT NULL,
  exercise_name TEXT NOT NULL DEFAULT '',
  is_timed BOOLEAN NOT NULL DEFAULT 0,
  actual INTEGER NOT NULL DEFAULT 0,  -- reps, or hold seconds when is_timed
  weight_kg REAL,                     -- user-entered on the web (watch has none)
  work_secs INTEGER,                  -- time under load; from the accel capture
  recording_id INTEGER REFERENCES recordings(id) ON DELETE SET NULL,
  performed_at TIMESTAMP NOT NULL
);
CREATE INDEX idx_session_sets_session ON session_sets(session_id, position);
