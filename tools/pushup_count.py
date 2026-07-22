#!/usr/bin/env python3
"""A working rep counter for **push-ups**, derived from the labelled recordings.

Insight (see tools/segment.py + the analysis): during a push-up the wrist tilts
once per rep, so one accelerometer axis (typically z) oscillates once per rep.
The setup (getting into plank) and dismount (getting up) produce similar swings
at the very start/end, so we trim those before counting.

Algorithm (deliberately simple, so it can port to rep_counter.c):
  1. Per-axis rep-band signal = 0.5 s low-pass minus 3 s low-pass (isolates the
     ~0.3-0.7 Hz rep oscillation, removing gravity drift and high-freq noise).
  2. Activity window (segment.py), then trim TRIM_S off each end (setup/dismount).
  3. Pick the axis that swings most in that window.
  4. Count full down-up cycles with hysteresis (dip below -h, rise above +h),
     h = AMP_FRAC * 95th-percentile amplitude, min REP_MIN_S between reps.
  5. If the axis barely moves (< MIN_AMP), it's 0 reps.

Tuned + validated against every push-up recording in the dev DB: clean (new
single-buzz firmware) sets score within 1 rep total; see `--eval`.

Usage:
  python3 tools/pushup_count.py --eval
  python3 tools/pushup_count.py recording_10.csv     # t_ms,x,y,z CSV
"""
import argparse
import struct
import sys
import pathlib

import numpy as np

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from segment import segment_activity, linear_mag  # noqa: E402

RATE = 25
REP_MIN_S = 1.0     # reps are at least this far apart
AMP_FRAC = 0.40     # a rep must swing at least this fraction of the peak amplitude
TRIM_S = 3.0        # drop this much off each end of the active window (setup/dismount)
MIN_AMP = 150.0     # below this the axis is basically still -> 0 reps


def _smooth(x, win):
    return x if win <= 1 else np.convolve(x, np.ones(win) / win, mode="same")


def count_pushups(xyz, rate=RATE):
    """xyz: (N,3) accel in mG. Returns the rep count."""
    xyz = np.asarray(xyz, float)
    g = np.array([_smooth(xyz[:, k], int(0.5 * rate)) for k in range(3)]).T
    osc = g - np.array([_smooth(g[:, k], int(3.0 * rate)) for k in range(3)]).T
    i0, i1 = segment_activity(linear_mag(xyz), rate)
    t = int(TRIM_S * rate)
    i0, i1 = i0 + t, i1 - t
    if i1 - i0 < rate:
        return 0
    seg = osc[i0:i1]
    s = seg[:, int(np.argmax(seg.std(axis=0)))]
    s = s - s.mean()
    peak = float(np.percentile(np.abs(s), 95))
    if peak < MIN_AMP:
        return 0
    h = AMP_FRAC * peak
    min_gap = REP_MIN_S * rate
    count, state, last = 0, "high", -1e9
    for i, v in enumerate(s):
        if state == "high" and v < -h:
            state = "low"
        elif state == "low" and v > h:
            if i - last >= min_gap:
                count += 1
                last = i
            state = "high"
    return count


def _load_csv(path):
    import csv
    rows = []
    with open(path) as f:
        r = csv.reader(f)
        next(r, None)
        for line in r:
            if len(line) >= 4:
                rows.append([float(line[1]), float(line[2]), float(line[3])])
    return np.array(rows)


def _eval(db):
    import sqlite3
    con = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    recs = con.execute(
        "SELECT id, actual, samples FROM recordings WHERE movement_id=4 AND is_timed=0 ORDER BY id"
    ).fetchall()
    print(f"{'rec':>4} {'actual':>6} {'counted':>7} {'err':>4}")
    tot = 0
    for rid, actual, blob in recs:
        n = len(blob) // 6
        xyz = np.array([struct.unpack_from("<hhh", blob, i * 6) for i in range(n)], float)
        c = count_pushups(xyz)
        tot += abs(c - actual)
        print(f"{rid:>4} {actual:>6} {c:>7} {c - actual:>+4}")
    print(f"\n total abs error: {tot}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("csv", nargs="?", help="recording CSV (t_ms,x,y,z)")
    ap.add_argument("--eval", action="store_true", help="score against the dev DB")
    ap.add_argument("--db", default=str(pathlib.Path(__file__).resolve().parents[1] / "server" / "strength.db"))
    args = ap.parse_args()
    if args.eval:
        _eval(args.db)
    elif args.csv:
        print(count_pushups(_load_csv(args.csv)))
    else:
        ap.print_help()


if __name__ == "__main__":
    main()
