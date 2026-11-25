-- Per-workout heart-rate trace (devices with the sensor: Pebble 2 HR, Time 2).
-- bpm is a JSON array of BPM samples (0 = no reading) at interval_sec from
-- started_at. avg/peak are precomputed over the real (non-zero) readings for
-- cheap summaries. Attached to a session by its time window at render.
CREATE TABLE session_hr (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER,
  client_id BIGINT NOT NULL,       -- watch-unique; idempotency key with user_id
  workout_name TEXT NOT NULL DEFAULT '',
  started_at BIGINT NOT NULL,      -- unix seconds (watch RTC)
  interval_sec INTEGER NOT NULL DEFAULT 5,
  bpm TEXT NOT NULL DEFAULT '[]',  -- JSON array of samples
  avg_bpm INTEGER,
  peak_bpm INTEGER,
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE UNIQUE INDEX idx_session_hr_client ON session_hr (user_id, client_id);
CREATE INDEX idx_session_hr_started ON session_hr (user_id, started_at);
