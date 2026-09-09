#!/usr/bin/env python3
"""Rotation-invariant, set-level features for the learned rep counter
(docs/design/rep-counter-learned.md).

Every feature is unchanged under any rotation/flip of the watch frame, so a model
trained on them counts reps regardless of how the device is worn. This is the
shared spec between offline training (train_rep_model.py) and the on-watch C port
— keep them in sync.
"""
import numpy as np

RATE = 25
FEATURE_NAMES = ["dur", "acest", "fftest", "magstd", "pk"]


def _smooth(x, win):
    return x if win <= 1 else np.convolve(x, np.ones(win) / win, mode="same")


def _linear_mag(xyz, rate=RATE):
    """Gravity-removed |accel| (EMA alpha 1/16), matching tools/segment."""
    g = np.empty_like(xyz)
    g[0] = xyz[0]
    a = 1.0 / 16.0
    for i in range(1, len(xyz)):
        g[i] = g[i - 1] + (xyz[i] - g[i - 1]) * a
    return np.linalg.norm(xyz - g, axis=1)


def _segment(mag, rate=RATE, on_frac=0.22, pad_s=0.3):
    """Active window [i0, i1) from an energy envelope (percentile-relative)."""
    env = _smooth(np.abs(mag), int(0.6 * rate))
    lo, hi = np.percentile(env, 10), np.percentile(env, 90)
    thr = lo + on_frac * (hi - lo)
    active = np.where(env > thr)[0]
    if len(active) < rate:
        return 0, len(mag)
    pad = int(pad_s * rate)
    return max(0, active[0] - pad), min(len(mag), active[-1] + pad)


def _inv_mag(xyz, rate=RATE):
    """Band-passed |accel| magnitude — rotation-invariant rep signal."""
    lp = np.array([_smooth(xyz[:, k], int(0.4 * rate)) for k in range(3)]).T
    base = np.array([_smooth(lp[:, k], int(3.0 * rate)) for k in range(3)]).T
    return np.linalg.norm(lp - base, axis=1)


def features(xyz, rate=RATE):
    """Return the rotation-invariant feature dict, or None if too short."""
    xyz = np.asarray(xyz, float)
    mag = _inv_mag(xyz, rate)
    i0, i1 = _segment(_linear_mag(xyz, rate), rate)
    seg = mag[i0:i1]
    dur = (i1 - i0) / rate
    if len(seg) < rate:
        return None
    s = seg - seg.mean()

    # autocorrelation-based rep period -> duration/period estimate
    ac = np.correlate(s, s, "full")[len(s) - 1:]
    ac = ac / (ac[0] + 1e-9)
    period = 0.0
    for lag in range(int(0.5 * rate), min(int(4 * rate), len(ac) - 1)):
        if ac[lag] > ac[lag - 1] and ac[lag] > ac[lag + 1] and ac[lag] > 0.1:
            period = lag / rate
            break

    # spectral peak frequency in the rep band
    f = np.fft.rfftfreq(len(s), 1 / rate)
    P = np.abs(np.fft.rfft(s)) ** 2
    band = (f > 0.15) & (f < 1.5)
    fpk = f[band][np.argmax(P[band])] if band.any() else 0.0

    # invariant peak count of the magnitude (overcounts, but informative)
    thr = np.percentile(seg, 60)
    pk, above = 0, False
    for v in seg:
        if not above and v > thr:
            above = True
        elif above and v < thr * 0.5:
            above = False
            pk += 1

    return {
        "dur": dur,
        "acest": dur / period if period else 0.0,
        "fftest": fpk * dur,
        "magstd": float(seg.std()),
        "pk": float(pk),
    }


def feature_vector(xyz, rate=RATE):
    f = features(xyz, rate)
    return None if f is None else np.array([f[n] for n in FEATURE_NAMES])
