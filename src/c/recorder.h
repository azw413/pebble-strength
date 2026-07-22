#pragma once
#include <pebble.h>

// Outbox must hold one chunk message; main opens AppMessage with this size.
#define RECORDER_OUTBOX_SIZE (1500 + 128)

// Set recorder (SPEC.md §6): captures raw 25 Hz accel samples during a set,
// then ships them to the phone over AppMessage in chunks. PebbleKit JS
// reassembles and POSTs to the server as the labelled tuning corpus.
//
// Lifecycle per set:
//   recorder_begin()                    at set start (no-op if still busy)
//   recorder_feed(x, y, z)              per sample
//   recorder_stage(...)                 at set end -> chunks start flowing
//   recorder_set_label(actual)          when the (corrected) count is final
//   recorder_abort()                    discard an unstaged capture

void recorder_init(void);
bool recorder_is_capturing(void);
void recorder_begin(void);
void recorder_feed(int16_t x, int16_t y, int16_t z);
void recorder_stage(uint8_t movement_id, uint8_t set_index, bool timed,
                    const char *workout_name);
void recorder_set_label(uint8_t actual);
void recorder_abort(void);
