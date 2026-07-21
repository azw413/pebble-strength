-- Dashboard support: per-exercise bodyweight load factor and a bodyweight log.

-- Fraction of bodyweight that loads the muscles for a rep-based calisthenics
-- movement (pull-up ~1.0, push-up ~0.65). 0 for weighted lifts and timed holds.
-- Effective load = bodyweight * load_factor + added weight.
ALTER TABLE exercises ADD COLUMN load_factor REAL NOT NULL DEFAULT 0;

-- Bodyweight over time; a session uses the entry with the nearest date.
CREATE TABLE bodyweights (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  measured_on DATE NOT NULL,
  weight_kg REAL NOT NULL,
  created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_bodyweights_user ON bodyweights(user_id, measured_on);
