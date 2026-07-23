-- Stable per-set id from the watch, so an offline-queued session-set upserts
-- idempotently against the live accel-upload path (no duplicate sets).
ALTER TABLE session_sets ADD COLUMN client_set_id BIGINT;
CREATE INDEX idx_session_sets_client ON session_sets (client_set_id);
