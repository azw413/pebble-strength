#!/usr/bin/env python3
"""Rep-analysis pipeline, stages 1-2: activity segmentation + rep identification.

This is the offline groundwork for retuning the on-device rep counter and, later,
symbolic-regression of a small on-device rep detector (see the roadmap in the repo
discussion). It works on the raw 25 Hz triaxial accelerometer recordings the watch
uploads.

Stages here:
  1. segment_activity() -- trim the preamble (getting into position) and the
     dismount, leaving the working span. Energy-envelope onset/offset.
  2. estimate_cadence() -- autocorrelation of the active signal to find the rep
     period, hence a rep-count estimate independent of the current counter.

Both are plain functions so later stages (feature extraction, symbolic regression)
can import them. `python3 tools/segment.py [--plot out.png]` prints a summary
table over all rep recordings in the dev DB and optionally writes a plot.

Usage:
  python3 tools/segment.py
  python3 tools/segment.py --plot /tmp/segment.png
  python3 tools/segment.py --db /path/to/strength.db
"""
import argparse
import struct
import pathlib

import numpy as np

ROOT = pathlib.Path(__file__).resolve().parents[1]
DEFAULT_DB = ROOT / "server" / "strength.db"

RATE = 25  # Hz; the watch records at 25 Hz


def linear_mag(xyz):
    """Gravity-removed acceleration magnitude, matching rep_counter.c's EMA
    (alpha = 1/16) so this analysis reflects what the device actually sees."""
    g = np.empty_like(xyz)
    g[0] = xyz[0]
    a = 1.0 / 16.0
    for i in range(1, len(xyz)):
        g[i] = g[i - 1] + (xyz[i] - g[i - 1]) * a
    return np.linalg.norm(xyz - g, axis=1)


def _smooth(x, win):
    if win <= 1:
        return x
    k = np.ones(win) / win
    return np.convolve(x, k, mode="same")


def segment_activity(mag, rate=RATE, smooth_s=0.6, on_frac=0.22, pad_s=0.3):
    """Return (i0, i1) sample indices of the working span.

    Smooth the magnitude into an energy envelope, threshold relative to a robust
    baseline (percentiles, so one big transient can't dominate), and take the
    span from the first to the last active sample. First-to-last (rather than the
    longest contiguous run) is deliberate: a periodic set dips below threshold
    *between* reps, and we must not split it into one-rep fragments.
    """
    n = len(mag)
    env = _smooth(mag, max(1, int(smooth_s * rate)))
    base = np.percentile(env, 20)
    peak = np.percentile(env, 95)
    if peak <= base:
        return 0, n
    thr = base + on_frac * (peak - base)
    idx = np.where(env > thr)[0]
    if len(idx) == 0:
        return 0, n
    pad = int(pad_s * rate)
    return max(0, idx[0] - pad), min(n, idx[-1] + 1 + pad)


def estimate_cadence(sig, rate=RATE, min_period_s=1.0, max_period_s=8.0, smooth_s=0.3):
    """Autocorrelation cadence: return (period_s, reps_estimate, strength).

    Operates on the low-passed *envelope* (reps show up as slow oscillation of
    the energy, not in the raw high-frequency magnitude) with a rep-plausible
    period floor. reps_estimate = active_duration / period. `strength` is the
    normalised autocorrelation peak (0..1); low means "not really periodic".
    """
    x = _smooth(sig, max(1, int(smooth_s * rate)))
    x = x - x.mean()
    if len(x) < rate or np.allclose(x, 0):
        return None, None, 0.0
    ac = np.correlate(x, x, mode="full")[len(x) - 1:]
    ac = ac / ac[0]
    lo = int(min_period_s * rate)
    hi = min(len(ac) - 1, int(max_period_s * rate))
    if hi <= lo:
        return None, None, 0.0
    lag = lo + int(np.argmax(ac[lo:hi]))
    period = lag / rate
    reps = len(sig) / rate / period
    return period, reps, float(ac[lag])


def load_rep_recordings(db_path):
    import sqlite3
    con = sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)
    rows = con.execute(
        """SELECT id, movement_id, exercise_name, set_index, actual, sample_rate, samples
           FROM recordings WHERE is_timed=0 ORDER BY id"""
    ).fetchall()
    out = []
    for rid, mv, name, si, actual, rate, blob in rows:
        n = len(blob) // 6
        xyz = np.array([struct.unpack_from("<hhh", blob, i * 6) for i in range(n)], float)
        out.append(dict(id=rid, mv=mv, name=name, set=si + 1, actual=actual,
                        rate=rate or RATE, xyz=xyz))
    return out


def analyse(rec):
    mag = linear_mag(rec["xyz"])
    rate = rec["rate"]
    i0, i1 = segment_activity(mag, rate)
    active = mag[i0:i1]
    period, reps, strength = estimate_cadence(active, rate)
    return dict(mag=mag, i0=i0, i1=i1,
                trimmed_s=(len(mag) - (i1 - i0)) / rate,
                active_s=(i1 - i0) / rate,
                period=period, reps=reps, strength=strength)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--db", default=str(DEFAULT_DB))
    ap.add_argument("--plot", default=None, help="write an analysis PNG here")
    args = ap.parse_args()

    recs = load_rep_recordings(args.db)
    print(f"{'rec':>3} {'exercise':<12} {'set':>3} {'actual':>6} "
          f"{'active_s':>8} {'trim_s':>7} {'period':>7} {'cad_reps':>8} {'periodic':>8}")
    results = []
    for r in recs:
        a = analyse(r)
        results.append((r, a))
        per = f"{a['period']:.2f}" if a["period"] else "  -"
        reps = f"{a['reps']:.1f}" if a["reps"] else "  -"
        print(f"{r['id']:>3} {r['name'][:12]:<12} {r['set']:>3} {r['actual']:>6} "
              f"{a['active_s']:>8.1f} {a['trimmed_s']:>7.1f} {per:>7} {reps:>8} "
              f"{a['strength']:>8.2f}")

    if args.plot:
        import matplotlib
        matplotlib.use("Agg")
        import matplotlib.pyplot as plt
        n = len(results)
        fig, axes = plt.subplots(n, 1, figsize=(11, 2.1 * n))
        for ax, (r, a) in zip(np.atleast_1d(axes), results):
            t = np.arange(len(a["mag"])) / r["rate"]
            ax.plot(t, a["mag"], lw=0.7, color="#333")
            ax.axvspan(a["i0"] / r["rate"], a["i1"] / r["rate"], color="#e5484d", alpha=0.10)
            ax.axvline(a["i0"] / r["rate"], color="#e5484d", lw=1)
            ax.axvline(a["i1"] / r["rate"], color="#e5484d", lw=1)
            cad = f"cadence {a['reps']:.1f} reps @ {a['period']:.2f}s (periodicity {a['strength']:.2f})" \
                if a["period"] else "no clear cadence"
            ax.set_title(f"rec{r['id']} {r['name']} set{r['set']}: actual={r['actual']} | "
                         f"active {a['active_s']:.1f}s, trimmed {a['trimmed_s']:.1f}s | {cad}",
                         fontsize=9, loc="left")
            ax.set_ylabel("|lin a| mG"); ax.margins(x=0.01)
        np.atleast_1d(axes)[-1].set_xlabel("seconds")
        fig.suptitle("Stage 1-2: activity window (red) + autocorrelation cadence", y=1.0)
        fig.tight_layout()
        fig.savefig(args.plot, dpi=110, bbox_inches="tight")
        print(f"\nwrote {args.plot}")


if __name__ == "__main__":
    main()
