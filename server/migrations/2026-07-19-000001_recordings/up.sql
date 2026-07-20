CREATE TABLE recordings (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  movement_id INTEGER NOT NULL,
  exercise_name TEXT NOT NULL DEFAULT '',
  workout_name TEXT NOT NULL DEFAULT '',
  set_index INTEGER NOT NULL DEFAULT 0,
  actual INTEGER NOT NULL,
  is_timed BOOLEAN NOT NULL DEFAULT 0,
  sample_rate INTEGER NOT NULL DEFAULT 25,
  sample_count INTEGER NOT NULL,
  truncated BOOLEAN NOT NULL DEFAULT 0,
  samples BLOB NOT NULL,
  recorded_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_recordings_user ON recordings(user_id, recorded_at);
