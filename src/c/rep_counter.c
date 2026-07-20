#include "rep_counter.h"

static uint32_t isqrt32(uint32_t v) {
  uint32_t r = 0, b = 1u << 30;
  while (b > v) {
    b >>= 2;
  }
  while (b) {
    if (v >= r + b) {
      v -= r + b;
      r = (r >> 1) + b;
    } else {
      r >>= 1;
    }
    b >>= 2;
  }
  return r;
}

void rep_counter_init(RepCounter *rc, uint16_t min_rep_ms, uint8_t smoothing) {
  memset(rc, 0, sizeof *rc);
  rc->min_rep_ms = min_rep_ms ? min_rep_ms : 900;
  rc->ma_len = smoothing;
  if (rc->ma_len < 1) rc->ma_len = 1;
  if (rc->ma_len > 8) rc->ma_len = 8;
  rc->env = REP_ENV_INIT;
}

bool rep_counter_feed(RepCounter *rc, int16_t x, int16_t y, int16_t z, uint32_t ms) {
  if (!rc->primed) {
    rc->gx = x;
    rc->gy = y;
    rc->gz = z;
    rc->start_ms = ms;
    rc->primed = true;
    return false;
  }

  // Gravity: per-axis EMA, alpha = 1/16.
  rc->gx += (x - rc->gx) / 16;
  rc->gy += (y - rc->gy) / 16;
  rc->gz += (z - rc->gz) / 16;

  // Linear-acceleration magnitude.
  int32_t lx = x - rc->gx, ly = y - rc->gy, lz = z - rc->gz;
  int32_t mag = (int32_t)isqrt32((uint32_t)(lx * lx + ly * ly + lz * lz));

  // Moving average.
  rc->ma_sum -= rc->ma_buf[rc->ma_idx];
  rc->ma_buf[rc->ma_idx] = mag;
  rc->ma_sum += mag;
  rc->ma_idx = (rc->ma_idx + 1) % rc->ma_len;
  if (rc->ma_fill < rc->ma_len) {
    rc->ma_fill++;
    return false;
  }
  int32_t sm = rc->ma_sum / rc->ma_len;

  // Adaptive threshold with hysteresis.
  int32_t thr = rc->env / 2;
  if (thr < REP_MIN_THRESHOLD) thr = REP_MIN_THRESHOLD;
  int32_t low = thr / 2;

  if (!rc->above) {
    if (sm > thr && (ms - rc->start_ms) > REP_SETTLE_MS &&
        (rc->last_rep_ms == 0 || (ms - rc->last_rep_ms) > rc->min_rep_ms)) {
      rc->above = true;
      rc->peak_max = sm;
    }
  } else {
    if (sm > rc->peak_max) {
      rc->peak_max = sm;
    }
    if (sm < low) {
      rc->above = false;
      rc->last_rep_ms = ms;
      rc->count++;
      rc->env += (rc->peak_max - rc->env) / 4;
      if (rc->env < REP_ENV_MIN) rc->env = REP_ENV_MIN;
      if (rc->env > REP_ENV_MAX) rc->env = REP_ENV_MAX;
      return true;
    }
  }
  return false;
}
