#pragma once
#include <pebble.h>

// Heart-rate trace capture for the whole workout (sets + rests). Lightweight:
// 1 byte per sample at a fixed interval, peeked on the session tick (no health
// event subscription — that allocates ~2 KB on the heap, which we can't spare).
// Only active on watches with the sensor (diorite HR, emery/Time 2); a no-op
// elsewhere. The trace is persisted and flushed to the phone like the offline
// session queue, so it survives a phone-in-locker workout.

#define HR_INTERVAL_S 5    // seconds between samples
#define HR_MAX 600         // ~50 min; 1 byte each

// True if this device exposes a heart-rate sensor.
bool hr_available(void);

void hr_init(void);  // load any persisted pending trace (for flush)

// Workout lifecycle. hr_begin raises the HR sample rate and resets the buffer;
// hr_tick appends the current BPM every HR_INTERVAL_S (call once per second);
// hr_end restores the sample rate, persists the trace, and marks it pending.
void hr_begin(const char *workout_name);
void hr_tick(void);
void hr_end(void);

// Flush the pending trace to the phone (sends MSG_HR). Safe to call when idle;
// no-op if nothing pending. Driven like the session queue: HR_ACK clears it.
void hr_flush(void);
void hr_ack(uint32_t client_id);
