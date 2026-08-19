-- Custom movement ids become per-user. The packed format carries the id as a
-- u8 (SPEC §4.2), so the custom range 128–255 is only 128 slots; making it
-- global would let one account exhaust a pool everyone else shares — and since
-- custom exercises are invisible to other users, running out would look
-- inexplicable. Each user now gets their own full 128 slots.
--
-- Built-ins keep global uniqueness: they come from shared/exercises.json and
-- must agree with the table compiled into the watch.

-- The old constraint is an inline UNIQUE, which SQLite implements as an
-- implicit index that can't be dropped, so the table has to be rebuilt.
-- workout_exercises.exercise_id references it and db.rs runs with
-- foreign_keys=ON, which breaks the rebuild two ways: DROP TABLE does an
-- implicit FK-checked DELETE, and ALTER TABLE RENAME rewrites REFERENCES
-- clauses to follow the renamed table. Both are disabled by turning
-- foreign_keys off — which SQLite ignores inside a transaction, hence
-- metadata.toml's run_in_transaction = false.
PRAGMA foreign_keys = OFF;

CREATE TABLE exercises_new (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  watch_movement_id INTEGER NOT NULL,
  name TEXT NOT NULL,
  body_area TEXT NOT NULL,
  primary_muscles TEXT NOT NULL DEFAULT '',
  secondary_muscles TEXT NOT NULL DEFAULT '',
  default_timed BOOLEAN NOT NULL DEFAULT 0,
  is_builtin BOOLEAN NOT NULL DEFAULT 1,
  load_factor REAL NOT NULL DEFAULT 0,
  category TEXT NOT NULL DEFAULT 'other',
  equipment TEXT NOT NULL DEFAULT 'bodyweight',
  loadable BOOLEAN NOT NULL DEFAULT 0,
  unilateral BOOLEAN NOT NULL DEFAULT 0,
  description TEXT NOT NULL DEFAULT '',
  min_reps INTEGER NOT NULL DEFAULT 1,
  max_reps INTEGER NOT NULL DEFAULT 100,
  default_reps INTEGER NOT NULL DEFAULT 10,
  default_rest_secs INTEGER NOT NULL DEFAULT 90,
  owner_user_id INTEGER
);

-- id values are preserved, so workout_exercises' references stay valid.
INSERT INTO exercises_new
  SELECT id, watch_movement_id, name, body_area, primary_muscles,
         secondary_muscles, default_timed, is_builtin, load_factor, category,
         equipment, loadable, unilateral, description, min_reps, max_reps,
         default_reps, default_rest_secs, owner_user_id
  FROM exercises;

DROP TABLE exercises;
ALTER TABLE exercises_new RENAME TO exercises;

-- Existing custom rows were numbered from one global sequence. Repack each
-- user's from 128 so everyone starts with a dense, full range. This runs
-- before the indexes below exist: SQLite checks uniqueness row by row within
-- an UPDATE, and a repack that lowers ids can collide mid-statement.
WITH renumbered AS (
  SELECT id,
         127 + ROW_NUMBER() OVER (PARTITION BY owner_user_id ORDER BY watch_movement_id)
           AS new_movement_id
  FROM exercises
  WHERE owner_user_id IS NOT NULL
)
UPDATE exercises
   SET watch_movement_id = (SELECT new_movement_id FROM renumbered WHERE renumbered.id = exercises.id)
 WHERE owner_user_id IS NOT NULL;

-- Built-ins: one row per movement id, matching the watch's compiled table.
-- Partial, so it ignores custom rows; seed.rs matches against this.
CREATE UNIQUE INDEX idx_exercises_builtin_movement
  ON exercises (watch_movement_id) WHERE owner_user_id IS NULL;

-- Custom: unique within an owner, so two users can both hold id 128.
CREATE UNIQUE INDEX idx_exercises_custom_movement
  ON exercises (owner_user_id, watch_movement_id) WHERE owner_user_id IS NOT NULL;

-- counter_configs is keyed by watch_movement_id alone, with no owner, so a
-- per-user id can no longer address a row there without collisions. Custom
-- movements never had a usable config anyway: they seeded at confidence 0.0
-- and /api/device/counters only ships confidence > 0, so the watch always fell
-- back to its compiled-in Custom(0) profile. Drop them; tuning a custom
-- movement will need an owner column here.
DELETE FROM counter_configs WHERE watch_movement_id >= 128;

PRAGMA foreign_keys = ON;
