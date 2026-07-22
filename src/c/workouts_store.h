#pragma once
#include <pebble.h>

// Workout store (SPEC.md §7, M3). Holds the watch's workouts in memory backed by
// persistent storage, so downloaded workouts survive restarts and are available
// offline. Until the first successful sync it serves the workouts embedded in
// the binary; a sync replaces them with the server's assigned slots.

#define WK_MAX 5
#define WK_MAX_BYTES 232  // PACK_CAP (228) + a little slack

void workouts_init(void);
uint8_t workouts_count(void);
// Packed bytes for workout i, or NULL. *len_out set to the length.
const uint8_t *workouts_get(uint8_t i, uint16_t *len_out);

// Sync (server -> watch): begin with the total count, set each by index, then
// commit. A partial sync (missing indices) is discarded on commit.
void workouts_sync_begin(uint8_t total);
void workouts_sync_set(uint8_t i, const uint8_t *data, uint16_t len);
// Returns true if the store changed (menu should reload).
bool workouts_sync_commit(void);
