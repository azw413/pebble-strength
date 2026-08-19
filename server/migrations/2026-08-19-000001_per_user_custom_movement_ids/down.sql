-- Back to one global movement-id sequence. Same foreign_keys caveat as up.sql.
PRAGMA foreign_keys = OFF;

-- Custom ids now repeat across users, so renumber them onto one global
-- sequence before restoring the UNIQUE constraint, or the rebuild fails.
-- Done with the per-user index dropped, so a lowered id can't collide with a
-- row the same statement hasn't rewritten yet.
DROP INDEX idx_exercises_custom_movement;
DROP INDEX idx_exercises_builtin_movement;

WITH renumbered AS (
  SELECT id,
         127 + ROW_NUMBER() OVER (ORDER BY owner_user_id, watch_movement_id)
           AS new_movement_id
  FROM exercises
  WHERE owner_user_id IS NOT NULL
)
UPDATE exercises
   SET watch_movement_id = (SELECT new_movement_id FROM renumbered WHERE renumbered.id = exercises.id)
 WHERE owner_user_id IS NOT NULL;

CREATE TABLE exercises_old (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  watch_movement_id INTEGER NOT NULL UNIQUE,
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

INSERT INTO exercises_old
  SELECT id, watch_movement_id, name, body_area, primary_muscles,
         secondary_muscles, default_timed, is_builtin, load_factor, category,
         equipment, loadable, unilateral, description, min_reps, max_reps,
         default_reps, default_rest_secs, owner_user_id
  FROM exercises;

DROP TABLE exercises;
ALTER TABLE exercises_old RENAME TO exercises;

PRAGMA foreign_keys = ON;
