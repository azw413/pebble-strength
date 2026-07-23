-- Flesh out the exercise catalog and give the rep counter its own versioned home.

-- New catalog metadata (server-side; the watch needs none of this).
ALTER TABLE exercises ADD COLUMN category TEXT NOT NULL DEFAULT 'other';           -- push/pull/hinge/squat/carry/core/other
ALTER TABLE exercises ADD COLUMN equipment TEXT NOT NULL DEFAULT 'bodyweight';     -- barbell/dumbbell/kettlebell/machine/cable/rings/bodyweight
ALTER TABLE exercises ADD COLUMN loadable BOOLEAN NOT NULL DEFAULT 0;              -- can external weight be added?
ALTER TABLE exercises ADD COLUMN unilateral BOOLEAN NOT NULL DEFAULT 0;           -- performed one side at a time
ALTER TABLE exercises ADD COLUMN description TEXT NOT NULL DEFAULT '';            -- form cue, shown in the web app
ALTER TABLE exercises ADD COLUMN min_reps INTEGER NOT NULL DEFAULT 1;            -- plausibility floor for the counter
ALTER TABLE exercises ADD COLUMN max_reps INTEGER NOT NULL DEFAULT 100;          -- plausibility ceiling for the counter
ALTER TABLE exercises ADD COLUMN default_reps INTEGER NOT NULL DEFAULT 10;       -- prescription default when added to a workout
ALTER TABLE exercises ADD COLUMN default_rest_secs INTEGER NOT NULL DEFAULT 90;  -- prescription default
ALTER TABLE exercises ADD COLUMN owner_user_id INTEGER;                          -- NULL = builtin/global; set for a user's custom exercise

-- The counter params move out of `exercises` into their own versioned table:
-- retuned on a different cadence, downloaded to the watch on their own rail, and
-- rollback-able. The JSON `profile` in shared/exercises.json still feeds the
-- compile-time on-device fallback (movements.h) — this is the tunable copy.
CREATE TABLE counter_configs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  watch_movement_id INTEGER NOT NULL,
  version INTEGER NOT NULL DEFAULT 1,
  active BOOLEAN NOT NULL DEFAULT 1,        -- the one config the watch downloads
  kind INTEGER NOT NULL DEFAULT 0,          -- 0 = parametric (Tier A)
  axis_mode INTEGER NOT NULL DEFAULT 0,     -- 0 auto(max-var) / 1 x / 2 y / 3 z / 4 |linear|
  lp_ms INTEGER NOT NULL DEFAULT 500,       -- low-pass window (rep-band top)
  hp_ms INTEGER NOT NULL DEFAULT 3000,      -- high-pass window (rep-band bottom / drift)
  thr_pct INTEGER NOT NULL DEFAULT 40,      -- hysteresis threshold, % of 95th-pct amplitude
  min_rep_ms INTEGER NOT NULL DEFAULT 900,  -- min spacing between reps
  min_amp INTEGER NOT NULL DEFAULT 150,     -- noise floor (mG); below this -> 0 reps
  warmup_ms INTEGER NOT NULL DEFAULT 0,     -- ignore this long at set start (settle into rep 1)
  confidence REAL NOT NULL DEFAULT 0.0,     -- 0..1 how much to trust this tune (UI leans on Up/Down when low)
  enabled BOOLEAN NOT NULL DEFAULT 1,       -- master switch for auto-counting this movement
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE UNIQUE INDEX idx_counter_configs_mv ON counter_configs (watch_movement_id, version);
CREATE INDEX idx_counter_configs_active ON counter_configs (watch_movement_id, active);

-- Superseded by counter_configs.
ALTER TABLE exercises DROP COLUMN profile_axis;
ALTER TABLE exercises DROP COLUMN profile_min_rep_ms;
ALTER TABLE exercises DROP COLUMN profile_smoothing;
