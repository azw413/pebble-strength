-- Learned rep-counter models (docs/design/rep-counter-learned.md). The "v2"
-- counter: rotation-invariant features -> a small model. Downloaded by
-- learned-model-capable apps via /api/device/rep-model; older apps ignore it
-- and keep using the fixed-axis configs ("v1"), so it's fully backwards-compatible.
--
-- feature_version defines WHICH features (and their order) the weights expect;
-- the watch runs a model only if it supports that feature_version, else falls
-- back to fixed-axis. model_version bumps on every retrain. movement_id NULL =
-- a global model applied to any rep movement it was trained for.
CREATE TABLE rep_models (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  feature_version INTEGER NOT NULL,
  model_version INTEGER NOT NULL,
  movement_id INTEGER,                 -- NULL = global
  kind INTEGER NOT NULL DEFAULT 0,     -- 0 = linear
  features TEXT NOT NULL,              -- comma-joined names (transparency; contract is feature_version)
  movements TEXT NOT NULL DEFAULT '',  -- comma-joined movement ids the model was trained/validated for
  weights TEXT NOT NULL,               -- JSON array of feature weights
  bias REAL NOT NULL DEFAULT 0,
  accuracy REAL,                       -- LOO accuracy at training time (0..1)
  trained_sets INTEGER,
  active BOOLEAN NOT NULL DEFAULT 1,   -- the one served per feature_version
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_rep_models_active ON rep_models (feature_version, active);
CREATE UNIQUE INDEX idx_rep_models_ver ON rep_models (feature_version, model_version, movement_id);
