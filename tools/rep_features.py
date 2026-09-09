#!/usr/bin/env python3
"""Rotation-invariant, set-level features for the learned rep counter
(docs/design/rep-counter-learned.md), feature_version 1.

Written in INTEGER arithmetic that the on-watch C port mirrors bit-for-bit
(src/c/rep_model.c) — moving averages via prefix sums, integer percentiles,
integer sqrt — so training and device agree exactly. All features are unchanged
under any rotation/flip of the watch frame (verified), so a model trained on
them counts reps regardless of how the device is worn.

Features (integer units): dur = active-window length in SAMPLES; magstd =
integer std (mG) of the band-passed |accel| over the window; pk = invariant
peak count of that magnitude.
"""
import math

RATE = 25
FEATURE_NAMES = ["dur", "magstd", "pk"]
# Bound the analysis length so the on-watch scratch fits the smallest platform
# (diorite). Longer sets are DECIMATED (subsampled), not truncated, so a long
# set keeps all its reps; window sizes scale down and dur scales back up.
MAXN = 600


def _isqrt(v):
    return math.isqrt(int(v)) if v > 0 else 0


def _smooth_same(a, w):
    """Integer moving average matching np.convolve(a, ones(w)/w, 'same'):
    centered window, edges divide by w (not by the smaller overlap)."""
    n = len(a)
    if w <= 1:
        return list(a)
    pre = [0] * (n + 1)
    for i in range(n):
        pre[i + 1] = pre[i] + a[i]
    off = (w - 1) // 2
    out = [0] * n
    for i in range(n):
        nn = i + off
        lo = max(0, nn - w + 1)
        hi = min(n - 1, nn)
        out[i] = (pre[hi + 1] - pre[lo]) // w
    return out


def _percentile(sorted_vals, q):
    """Linear-interpolated percentile (matches numpy default), integer output."""
    n = len(sorted_vals)
    if n == 1:
        return sorted_vals[0]
    pos = q * (n - 1) / 100.0
    lo = int(pos)
    frac = pos - lo
    if lo + 1 >= n:
        return sorted_vals[lo]
    return int(round(sorted_vals[lo] + frac * (sorted_vals[lo + 1] - sorted_vals[lo])))


def _grav_linmag(x, y, z):
    """Gravity-removed |accel| per sample (causal EMA alpha 1/16), integer mG."""
    n = len(x)
    gx, gy, gz = x[0], y[0], z[0]
    out = [0] * n
    for i in range(n):
        gx += (x[i] - gx) >> 4 if (x[i] - gx) >= 0 else -((gx - x[i]) >> 4)
        gy += (y[i] - gy) >> 4 if (y[i] - gy) >= 0 else -((gy - y[i]) >> 4)
        gz += (z[i] - gz) >> 4 if (z[i] - gz) >= 0 else -((gz - z[i]) >> 4)
        dx, dy, dz = x[i] - gx, y[i] - gy, z[i] - gz
        out[i] = _isqrt(dx * dx + dy * dy + dz * dz)
    return out


def _inv_mag(x, y, z, rate=RATE):
    """Band-passed |accel| magnitude (rotation-invariant), integer mG."""
    lpw, basew = int(0.4 * rate), int(3.0 * rate)
    mag = [0] * len(x)
    osc = []
    for ax in (x, y, z):
        lp = _smooth_same(ax, lpw)
        base = _smooth_same(lp, basew)
        osc.append([lp[i] - base[i] for i in range(len(ax))])
    ox, oy, oz = osc
    for i in range(len(x)):
        mag[i] = _isqrt(ox[i] * ox[i] + oy[i] * oy[i] + oz[i] * oz[i])
    return mag


def _segment(linmag, rate=RATE):
    env = _smooth_same(linmag, int(0.6 * rate))
    s = sorted(env)
    lo, hi = _percentile(s, 10), _percentile(s, 90)
    thr = lo + 22 * (hi - lo) // 100
    active = [i for i, v in enumerate(env) if v > thr]
    if len(active) < rate:
        return 0, len(linmag)
    pad = int(0.3 * rate)
    return max(0, active[0] - pad), min(len(linmag), active[-1] + pad)


def features(xyz, rate=RATE):
    """Integer feature dict (feature_version 1), or None if too short."""
    x = [int(v) for v in xyz[:, 0]]
    y = [int(v) for v in xyz[:, 1]]
    z = [int(v) for v in xyz[:, 2]]
    n = len(x)
    if n < rate:
        return None

    # Decimate long sets to fit MAXN; window sizes shrink by `step`, dur scales up.
    step = 1 if n <= MAXN else (n + MAXN - 1) // MAXN
    if step > 1:
        x, y, z = x[::step], y[::step], z[::step]
    rw = max(1, rate // step)  # window base (samples) in the decimated signal

    mag = _inv_mag(x, y, z, rw)
    i0, i1 = _segment(_grav_linmag(x, y, z), rw)
    seg = mag[i0:i1]
    if len(seg) < rw:
        return None
    dur = (i1 - i0) * step

    # integer std (mG) of the segment magnitude
    m = sum(seg) // len(seg)
    var = sum((v - m) * (v - m) for v in seg) // len(seg)
    magstd = _isqrt(var)

    # invariant peak count: cross above the 60th pct, then below half of it
    thr = _percentile(sorted(seg), 60)
    low = thr // 2
    pk, above = 0, False
    for v in seg:
        if not above and v > thr:
            above = True
        elif above and v < low:
            above = False
            pk += 1

    return {"dur": dur, "magstd": magstd, "pk": pk}


def feature_vector(xyz, rate=RATE):
    import numpy as np
    f = features(xyz, rate)
    return None if f is None else np.array([f[n] for n in FEATURE_NAMES], float)
