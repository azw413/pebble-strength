#include "rep_counter.h"
#include <string.h>
#include <math.h>

// Principal eigenvector of the accumulated 3x3 covariance (power iteration).
// Sign-normalised (largest component positive) so it's deterministic and matches
// the Python reference. Runs once per set when the selection window closes.
static void pca_solve(RepCounter *rc) {
  float m00 = (float)rc->cov[0], m11 = (float)rc->cov[1], m22 = (float)rc->cov[2];
  float m01 = (float)rc->cov[3], m02 = (float)rc->cov[4], m12 = (float)rc->cov[5];
  float u0 = 1.0f, u1 = 1.0f, u2 = 1.0f;
  for (int it = 0; it < 16; it++) {
    float v0 = m00 * u0 + m01 * u1 + m02 * u2;
    float v1 = m01 * u0 + m11 * u1 + m12 * u2;
    float v2 = m02 * u0 + m12 * u1 + m22 * u2;
    float nrm = sqrtf(v0 * v0 + v1 * v1 + v2 * v2);
    if (nrm < 1e-6f) break;
    u0 = v0 / nrm; u1 = v1 / nrm; u2 = v2 / nrm;
  }
  // Deterministic sign: make the largest-magnitude component positive.
  float a0 = u0 < 0 ? -u0 : u0, a1 = u1 < 0 ? -u1 : u1, a2 = u2 < 0 ? -u2 : u2;
  float dom = (a0 >= a1 && a0 >= a2) ? u0 : (a1 >= a2) ? u1 : u2;
  if (dom < 0) { u0 = -u0; u1 = -u1; u2 = -u2; }
  rc->pca_u[0] = u0; rc->pca_u[1] = u1; rc->pca_u[2] = u2;
}

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

void rep_counter_init(RepCounter *rc, const CounterConfig *cfg, uint16_t rate) {
  memset(rc, 0, sizeof *rc);
  if (rate == 0) rate = 25;
  uint32_t dt_ms = 1000u / rate;

  // First-order low-pass coefficient a = dt/(tau+dt), in Q16. Matches
  // rep_causal._ema_alpha to within integer rounding.
  uint16_t lp = cfg->lp_ms ? cfg->lp_ms : 500;
  uint16_t hp = cfg->hp_ms ? cfg->hp_ms : 3000;
  rc->alpha_lp_q16 = (65536u * dt_ms) / (lp + dt_ms);
  rc->alpha_hp_q16 = (65536u * dt_ms) / (hp + dt_ms);

  rc->axis_mode = cfg->axis_mode;
  if (cfg->axis_mode >= 1 && cfg->axis_mode <= 4) {
    rc->axis = (cfg->axis_mode <= 3) ? (uint8_t)(cfg->axis_mode - 1) : 3;
    rc->axis_locked = true;
  } else {
    rc->axis = 3;  // provisional until the selection window closes
    rc->axis_locked = false;
  }

  rc->min_amp_q8 = (int32_t)(cfg->min_amp ? cfg->min_amp : 150) << 8;
  rc->thr_pct = cfg->thr_pct ? cfg->thr_pct : 40;
  rc->min_rep_samples = (uint32_t)(cfg->min_rep_ms ? cfg->min_rep_ms : 900) * rate / 1000u;
  rc->warmup_samples = (uint32_t)cfg->warmup_ms * rate / 1000u;
  rc->sel_samples = (uint32_t)REP_SEL_MS * rate / 1000u;
  rc->amp_est_q8 = rc->min_amp_q8;
}

bool rep_counter_feed(RepCounter *rc, int16_t x, int16_t y, int16_t z, uint32_t ms) {
  (void)ms;  // timing is by sample index, not wall clock

  // Gravity: per-axis EMA (alpha 1/16) -> gravity-removed |linear| candidate,
  // matching tools/segment.linear_mag.
  if (!rc->primed) {
    rc->gx = x;
    rc->gy = y;
    rc->gz = z;
  } else {
    rc->gx += (x - rc->gx) / 16;
    rc->gy += (y - rc->gy) / 16;
    rc->gz += (z - rc->gz) / 16;
  }
  int32_t lx = x - rc->gx, ly = y - rc->gy, lz = z - rc->gz;
  int32_t mag = (int32_t)isqrt32((uint32_t)(lx * lx + ly * ly + lz * lz));

  int32_t cand_q8[4];
  cand_q8[0] = (int32_t)x << 8;
  cand_q8[1] = (int32_t)y << 8;
  cand_q8[2] = (int32_t)z << 8;
  cand_q8[3] = mag << 8;

  if (!rc->primed) {
    for (int k = 0; k < 4; k++) {
      rc->lp_q8[k] = cand_q8[k];
      rc->base_q8[k] = cand_q8[k];
    }
    rc->primed = true;
  }

  // Causal band-pass: lp = light smooth of the axis; base = slow EMA of lp.
  for (int k = 0; k < 4; k++) {
    rc->lp_q8[k] += (int32_t)(((int64_t)(cand_q8[k] - rc->lp_q8[k]) * rc->alpha_lp_q16) >> 16);
    rc->base_q8[k] += (int32_t)(((int64_t)(rc->lp_q8[k] - rc->base_q8[k]) * rc->alpha_hp_q16) >> 16);
  }

  uint32_t i = rc->n++;

  // Band-passed rep signal (Q8) for x,y,z — used by auto-axis and PCA.
  int32_t ox = rc->lp_q8[0] - rc->base_q8[0];
  int32_t oy = rc->lp_q8[1] - rc->base_q8[1];
  int32_t oz = rc->lp_q8[2] - rc->base_q8[2];

  int32_t osc;
  if (rc->axis_mode == 5) {
    // Rotation-invariant PCA: over the selection window accumulate the
    // covariance of (ox,oy,oz), then lock the principal direction (the rep
    // direction). Projecting onto it is invariant to watch orientation.
    if (i < rc->warmup_samples) return false;
    if (!rc->axis_locked) {
      int64_t mx = ox >> 8, my = oy >> 8, mz = oz >> 8;  // to mG
      rc->cov[0] += mx * mx; rc->cov[1] += my * my; rc->cov[2] += mz * mz;
      rc->cov[3] += mx * my; rc->cov[4] += mx * mz; rc->cov[5] += my * mz;
      if (i >= rc->warmup_samples + rc->sel_samples) {
        pca_solve(rc);
        rc->axis_locked = true;
      }
      return false;
    }
    float s = (float)(ox >> 8) * rc->pca_u[0] + (float)(oy >> 8) * rc->pca_u[1] +
              (float)(oz >> 8) * rc->pca_u[2];
    osc = (int32_t)(s * 256.0f);  // back to Q8 for the shared hysteresis
  } else {
    // Auto axis (mode 0): accumulate per-axis variance, then lock the strongest.
    if (!rc->axis_locked) {
      rc->sq[0] += (int64_t)ox * ox;
      rc->sq[1] += (int64_t)oy * oy;
      rc->sq[2] += (int64_t)oz * oz;
      if (i >= rc->sel_samples) {
        uint8_t best = 0;
        if (rc->sq[1] > rc->sq[best]) best = 1;
        if (rc->sq[2] > rc->sq[best]) best = 2;
        rc->axis = best;
        rc->axis_locked = true;
      }
    }
    if (i < rc->warmup_samples || !rc->axis_locked) return false;
    osc = rc->lp_q8[rc->axis] - rc->base_q8[rc->axis];
  }

  int32_t h = rc->min_amp_q8;
  int32_t adj = (int32_t)(((int64_t)rc->amp_est_q8 * rc->thr_pct) / 100);
  if (adj > h) h = adj;

  if (!rc->in_low) {
    if (osc < -h) {
      rc->in_low = true;
      rc->trough_q8 = osc;
    }
  } else {
    if (osc < rc->trough_q8) rc->trough_q8 = osc;
    if (osc > h) {
      rc->in_low = false;
      bool ok = !rc->have_rep || (i - rc->last_rep_n >= rc->min_rep_samples);
      if (ok) {
        rc->count++;
        rc->last_rep_n = i;
        rc->have_rep = true;
        // Adapt the amplitude estimate toward this swing's depth (~0.35).
        int32_t at = rc->trough_q8 < 0 ? -rc->trough_q8 : rc->trough_q8;
        rc->amp_est_q8 += (int32_t)(((int64_t)(at - rc->amp_est_q8) * 90) >> 8);
        return true;
      }
    }
  }
  return false;
}
