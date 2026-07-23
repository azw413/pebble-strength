ALTER TABLE exercises ADD COLUMN profile_axis TEXT NOT NULL DEFAULT 'mag';
ALTER TABLE exercises ADD COLUMN profile_min_rep_ms INTEGER NOT NULL DEFAULT 900;
ALTER TABLE exercises ADD COLUMN profile_smoothing INTEGER NOT NULL DEFAULT 5;

DROP TABLE counter_configs;

ALTER TABLE exercises DROP COLUMN owner_user_id;
ALTER TABLE exercises DROP COLUMN default_rest_secs;
ALTER TABLE exercises DROP COLUMN default_reps;
ALTER TABLE exercises DROP COLUMN max_reps;
ALTER TABLE exercises DROP COLUMN min_reps;
ALTER TABLE exercises DROP COLUMN description;
ALTER TABLE exercises DROP COLUMN unilateral;
ALTER TABLE exercises DROP COLUMN loadable;
ALTER TABLE exercises DROP COLUMN equipment;
ALTER TABLE exercises DROP COLUMN category;
