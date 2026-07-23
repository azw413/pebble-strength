#pragma once
#include <pebble.h>
#include "rep_counter.h"

// Downloaded per-movement counter overrides. A small persist-backed table
// keyed by movement_id; a sync from the phone replaces it wholesale. Absent any
// download, counter_config_get falls back to the compiled defaults (counters.h),
// so the counter always works offline and on first run.

#define CN_STORE_MAX 18     // records that fit one 256-byte persist value (18*14=252)
#define CN_RECORD_BYTES 14  // packed wire record; must match pkjs packCounters()

void counters_init(void);

// Fill *out with the movement's config: a downloaded override if present, else
// the compiled-in default.
void counter_config_get(uint8_t movement_id, CounterConfig *out);

// Sync (phone -> watch): replace all overrides from `count` packed records in
// `blob` (14 bytes each). Persists. Returns true if applied.
bool counters_sync_set(const uint8_t *blob, uint16_t len, uint8_t count);
