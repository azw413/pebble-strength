-- Crash breadcrumbs reported by the watch on the boot after a crash (Pebble has
-- no remote crash telemetry). code = what it was doing (see crumb.h), ctx =
-- code-specific (e.g. movement_id), heap = free bytes at the time.
CREATE TABLE crash_reports (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER,
  code INTEGER NOT NULL,
  ctx BIGINT,
  heap BIGINT,
  app_version TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
