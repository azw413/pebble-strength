#pragma once
#include <stdint.h>
#include <stdbool.h>

// Data-driven rep counter: a generic causal engine driven by a per-movement
// CounterConfig (SPEC §6, docs/design/rep-counter-data-driven.md). It is a
// direct fixed-point image of tools/rep_causal.py — keep the two in sync.
//
// Deliberately depends only on stdint/stdbool (no pebble.h) so the pure
// algorithm can be unit-tested off-device against the labelled corpus
// (tools/rep_ctest.c + tools/verify_rep_counter.py).
//
// Pipeline: two causal EMAs per candidate axis form a rep-band signal
// (osc = fast-smooth - slow-baseline); after a warmup, hysteresis counts
// down-up swings with a refractory gap; the threshold scales with a running
// amplitude estimate so it adapts to how hard the athlete works.

#define REP_SEL_MS 2500  // auto-axis: variance-accumulation window

// Fitted params for one movement. Matches the server `counter_configs` row;
// on-device these arrive compiled-in (counters.h) or, later, downloaded.
typedef struct {
  uint8_t kind;         // 0 = parametric (the only kind implemented)
  uint8_t axis_mode;    // 0 auto(max-var) / 1 x / 2 y / 3 z / 4 |linear|
  uint16_t lp_ms;       // fast smoothing time-constant (rep-band top)
  uint16_t hp_ms;       // slow baseline time-constant (rep-band bottom / drift)
  uint8_t thr_pct;      // hysteresis threshold, % of running amplitude
  uint16_t min_rep_ms;  // refractory gap between reps
  uint16_t min_amp;     // noise floor (mG); threshold never dips below this
  uint16_t warmup_ms;   // ignore this long at set start (settle / get into rep 1)
} CounterConfig;

typedef struct {
  // Derived from the config at init.
  uint32_t alpha_lp_q16, alpha_hp_q16;  // EMA coefficients, Q16
  uint8_t axis_mode, axis;
  bool axis_locked;
  int32_t min_amp_q8;
  uint8_t thr_pct;
  uint32_t min_rep_samples, warmup_samples, sel_samples;

  // Running state. Signals held in Q8 fixed point (mG << 8) for EMA precision.
  int32_t lp_q8[4], base_q8[4];  // band-pass state for x, y, z, |linear|
  int32_t gx, gy, gz;            // gravity EMA (alpha 1/16) for the |linear| axis
  int64_t sq[3];                 // auto-axis variance accumulators
  int32_t amp_est_q8;            // running swing-amplitude estimate
  bool in_low;                   // inside a down-swing, waiting for the rise
  int32_t trough_q8;             // deepest point of the current down-swing
  uint32_t n;                    // samples seen
  uint32_t last_rep_n;
  bool have_rep;
  uint16_t count;
  bool primed;
} RepCounter;

// rate = sample rate in Hz (25 on device). Config nulls fall back to sane
// defaults so a partially-specified config still runs.
void rep_counter_init(RepCounter *rc, const CounterConfig *cfg, uint16_t rate);

// Feed one sample (mG). Returns true when a rep was just counted. `ms` is
// accepted for call-site compatibility but timing is by sample index.
bool rep_counter_feed(RepCounter *rc, int16_t x, int16_t y, int16_t z, uint32_t ms);
