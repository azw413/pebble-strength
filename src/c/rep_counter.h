#pragma once
#include <pebble.h>

// Accelerometer rep counter (SPEC.md §6). Fixed-point integer pipeline:
// gravity EMA removal -> linear-accel magnitude -> moving average ->
// adaptive-threshold peak detection with hysteresis + refractory period.
// Mirrored by tools/replay.py — keep the constants in sync.

#define REP_SETTLE_MS 1500     // ignore crossings this long after set start
#define REP_MIN_THRESHOLD 100  // mG floor for the adaptive threshold
#define REP_ENV_INIT 400       // initial peak-envelope estimate (mG)
#define REP_ENV_MIN 200
#define REP_ENV_MAX 4000

typedef struct {
  int32_t gx, gy, gz;      // gravity estimate per axis (EMA, alpha = 1/16)
  int32_t ma_buf[8];       // moving-average window
  int32_t ma_sum;
  uint8_t ma_len, ma_idx, ma_fill;
  int32_t env;             // EMA of recent peak amplitudes
  bool above;              // currently above threshold (inside a candidate rep)
  int32_t peak_max;
  uint32_t start_ms, last_rep_ms;
  uint16_t min_rep_ms;     // refractory period from the movement profile
  uint16_t count;
  bool primed;
} RepCounter;

void rep_counter_init(RepCounter *rc, uint16_t min_rep_ms, uint8_t smoothing);

// Feed one 25 Hz sample (mG). Returns true when a rep was just counted.
bool rep_counter_feed(RepCounter *rc, int16_t x, int16_t y, int16_t z, uint32_t ms);
