#pragma once
#include <pebble.h>

// Downloaded exercise-name catalog. Makes the exercise list data-driven: a new
// exercise reaches the watch over the sync rail (like workouts and counter
// configs) instead of needing a firmware reinstall. movement_name() prefers a
// downloaded name and falls back to the compiled table (movements.h) when the
// catalog hasn't synced yet, so names always resolve.
//
// Wire/blob record (variable length, packed by pkjs packExercises()):
//   [movement_id u8][name_len u8][name bytes...]  repeated.

#define EX_MAX_BYTES 1800  // packed catalog cap (~ up to ~65 exercises)

void exercises_init(void);

// The movement's display name: downloaded catalog first, else the compiled
// built-in, else "Unknown".
const char *movement_name(uint8_t movement_id);

// Sync (phone -> watch): begin with the blob count, set each blob by index,
// then commit. A partial sync (missing blobs) is discarded on commit.
void exercises_sync_begin(uint8_t total);
void exercises_sync_set(uint8_t index, const uint8_t *data, uint16_t len);
bool exercises_sync_commit(void);
