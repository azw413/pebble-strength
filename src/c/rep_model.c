#include "rep_model.h"
#include <string.h>
#include <stdlib.h>

#define MAXN 600   // bounded analysis length (fits smallest platform RAM); longer sets decimate

// Scratch is allocated on demand (~6 KB) only for the brief once-per-set feature
// computation, then freed — so it doesn't permanently reduce the app heap on the
// smaller platforms. Pointers into a single malloc'd block.
static int16_t *s_a, *s_b, *s_lin;
static int32_t *s_mag;

static uint32_t isqrt32(uint32_t v) {
  uint32_t r = 0, b = 1u << 30;
  while (b > v) b >>= 2;
  while (b) {
    if (v >= r + b) { v -= r + b; r = (r >> 1) + b; }
    else r >>= 1;
    b >>= 2;
  }
  return r;
}

// Integer moving average matching np.convolve(a, ones(w)/w, 'same').
static void smooth_same(const int16_t *src, int16_t *dst, uint16_t n, uint16_t w) {
  if (w <= 1) { memcpy(dst, src, (size_t)n * sizeof(int16_t)); return; }
  int off = (w - 1) / 2;
  int64_t sum = 0;
  int lo_prev = 0, hi_prev = -1;
  for (int i = 0; i < n; i++) {
    int nn = i + off;
    int lo = nn - w + 1; if (lo < 0) lo = 0;
    int hi = nn; if (hi > n - 1) hi = n - 1;
    while (hi_prev < hi) sum += src[++hi_prev];
    while (lo_prev < lo) sum -= src[lo_prev++];
    dst[i] = (int16_t)(sum / w);
  }
}

static int cmp_i16(const void *a, const void *b) {
  int16_t x = *(const int16_t *)a, y = *(const int16_t *)b;
  return (x > y) - (x < y);
}

static int32_t pctile(const int16_t *sorted, int n, int q) {
  if (n == 1) return sorted[0];
  int num = q * (n - 1);
  int lo = num / 100;
  int frac_num = num - lo * 100;
  if (lo + 1 >= n) return sorted[lo];
  return sorted[lo] + (frac_num * (sorted[lo + 1] - sorted[lo]) + 50) / 100;
}

// Feature extraction inner body — scratch already allocated.
static bool rep_features_inner(const int16_t *xyz, uint16_t n, uint16_t rate,
                               int32_t feat[REP_NFEAT]) {
  // Decimate long sets to fit MAXN; window sizes shrink by step, dur scales up.
  int step = (n <= MAXN) ? 1 : (n + MAXN - 1) / MAXN;
  int m = (n + step - 1) / step;  // decimated sample count
  if (m > MAXN) m = MAXN;
  int rw = rate / step; if (rw < 1) rw = 1;

  // Gravity-removed |accel| per (decimated) sample -> s_lin.
  int32_t gx = xyz[0], gy = xyz[1], gz = xyz[2];
  for (int i = 0; i < m; i++) {
    int j = i * step;
    int32_t x = xyz[j * 3], y = xyz[j * 3 + 1], z = xyz[j * 3 + 2];
    gx += (x - gx) >> 4; gy += (y - gy) >> 4; gz += (z - gz) >> 4;
    int32_t dx = x - gx, dy = y - gy, dz = z - gz;
    s_lin[i] = (int16_t)isqrt32((uint32_t)(dx * dx + dy * dy + dz * dz));
  }

  // Band-passed |accel| magnitude -> s_mag (accumulate osc^2 across axes).
  uint16_t lpw = (uint16_t)(0.4 * rw), basew = (uint16_t)(3.0 * rw);
  for (int i = 0; i < m; i++) s_mag[i] = 0;
  for (int k = 0; k < 3; k++) {
    for (int i = 0; i < m; i++) s_a[i] = xyz[(i * step) * 3 + k];
    smooth_same(s_a, s_b, m, lpw);    // lp -> s_b
    smooth_same(s_b, s_a, m, basew);  // base -> s_a
    for (int i = 0; i < m; i++) {
      int32_t osc = s_b[i] - s_a[i];
      s_mag[i] += osc * osc;
    }
  }
  for (int i = 0; i < m; i++) s_mag[i] = (int32_t)isqrt32((uint32_t)s_mag[i]);

  // Segmentation on s_lin: env = smooth(lin, 0.6s); thr from percentiles.
  smooth_same(s_lin, s_a, m, (uint16_t)(0.6 * rw));  // env -> s_a
  memcpy(s_b, s_a, (size_t)m * sizeof(int16_t));
  qsort(s_b, m, sizeof(int16_t), cmp_i16);
  int32_t p10 = pctile(s_b, m, 10), p90 = pctile(s_b, m, 90);
  int32_t thr = p10 + 22 * (p90 - p10) / 100;
  int first = -1, last = -1;
  for (int i = 0; i < m; i++) if (s_a[i] > thr) { if (first < 0) first = i; last = i; }
  int pad = (int)(0.3 * rw);
  int i0, i1;
  if (first < 0 || last - first + 1 < rw) { i0 = 0; i1 = m; }
  else { i0 = first - pad; if (i0 < 0) i0 = 0; i1 = last + pad; if (i1 > m) i1 = m; }
  int len = i1 - i0;
  if (len < rw) return false;

  // magstd: integer std (mG) of s_mag[i0:i1]
  int64_t sum = 0;
  for (int i = i0; i < i1; i++) sum += s_mag[i];
  int32_t mean = (int32_t)(sum / len);
  int64_t var = 0;
  for (int i = i0; i < i1; i++) { int32_t d = s_mag[i] - mean; var += (int64_t)d * d; }
  int32_t magstd = (int32_t)isqrt32((uint32_t)(var / len));

  // pk: cross above the 60th pct of the segment, then below half of it
  for (int i = 0; i < len; i++) s_b[i] = (int16_t)s_mag[i0 + i];
  qsort(s_b, len, sizeof(int16_t), cmp_i16);
  int32_t p60 = pctile(s_b, len, 60), low = p60 / 2;
  int pk = 0; bool above = false;
  for (int i = i0; i < i1; i++) {
    int32_t v = s_mag[i];
    if (!above) { if (v > p60) above = true; }
    else if (v < low) { above = false; pk++; }
  }

  feat[0] = len * step;  // dur in original samples
  feat[1] = magstd;
  feat[2] = pk;
  return true;
}

bool rep_features(const int16_t *xyz, uint16_t n, uint16_t rate, int32_t feat[REP_NFEAT]) {
  if (n < rate) return false;
  // One block for all scratch (~6 KB), freed on return.
  uint8_t *block = malloc((size_t)MAXN * (3 * sizeof(int16_t) + sizeof(int32_t)));
  if (!block) return false;  // low memory -> caller falls back to the live count
  s_a = (int16_t *)block;
  s_b = s_a + MAXN;
  s_lin = s_b + MAXN;
  s_mag = (int32_t *)(s_lin + MAXN);
  bool ok = rep_features_inner(xyz, n, rate, feat);
  free(block);
  return ok;
}

int rep_model_predict(const RepModel *m, const int32_t feat[REP_NFEAT]) {
  int64_t acc = m->bias_q16;
  for (int i = 0; i < REP_NFEAT; i++) acc += (int64_t)m->weight_q16[i] * feat[i];
  int32_t v = (int32_t)((acc + (1 << 15)) >> 16);
  return v < 0 ? 0 : v;
}

bool rep_model_has(const RepModel *m, uint8_t movement_id) {
  if (!m->present) return false;
  for (int i = 0; i < m->movement_count; i++)
    if (m->movements[i] == movement_id) return true;
  return false;
}
