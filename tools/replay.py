#!/usr/bin/env python3
"""Replay a recording through the exact on-watch rep-counting pipeline.

Mirrors src/c/rep_counter.c — keep constants and integer semantics in sync.

Usage:
  python3 tools/replay.py recording_7.csv            # count reps in a capture
  python3 tools/replay.py recording_7.csv --min-rep-ms 700
  python3 tools/replay.py --selftest                 # synthetic waveform check

CSV format (as served by /recordings/{id}/csv): t_ms,x,y,z in mG.
"""
import argparse
import csv
import math
import sys

SETTLE_MS = 1500
MIN_THRESHOLD = 100
ENV_INIT = 400
ENV_MIN = 200
ENV_MAX = 4000


def cdiv(a, b):
    """C-style integer division (truncates toward zero)."""
    q = abs(a) // b
    return q if a >= 0 else -q


class RepCounter:
    def __init__(self, min_rep_ms=900, smoothing=5):
        self.min_rep_ms = min_rep_ms or 900
        self.ma_len = max(1, min(8, smoothing))
        self.gx = self.gy = self.gz = 0
        self.ma_buf = [0] * self.ma_len
        self.ma_sum = 0
        self.ma_idx = 0
        self.ma_fill = 0
        self.env = ENV_INIT
        self.above = False
        self.peak_max = 0
        self.start_ms = 0
        self.last_rep_ms = 0
        self.count = 0
        self.primed = False
        self.rep_times = []

    def feed(self, x, y, z, ms):
        if not self.primed:
            self.gx, self.gy, self.gz = x, y, z
            self.start_ms = ms
            self.primed = True
            return False

        self.gx += cdiv(x - self.gx, 16)
        self.gy += cdiv(y - self.gy, 16)
        self.gz += cdiv(z - self.gz, 16)

        lx, ly, lz = x - self.gx, y - self.gy, z - self.gz
        mag = math.isqrt(lx * lx + ly * ly + lz * lz)

        self.ma_sum -= self.ma_buf[self.ma_idx]
        self.ma_buf[self.ma_idx] = mag
        self.ma_sum += mag
        self.ma_idx = (self.ma_idx + 1) % self.ma_len
        if self.ma_fill < self.ma_len:
            self.ma_fill += 1
            return False
        sm = self.ma_sum // self.ma_len

        thr = max(self.env // 2, MIN_THRESHOLD)
        low = thr // 2

        if not self.above:
            if (sm > thr and (ms - self.start_ms) > SETTLE_MS
                    and (self.last_rep_ms == 0 or (ms - self.last_rep_ms) > self.min_rep_ms)):
                self.above = True
                self.peak_max = sm
        else:
            self.peak_max = max(self.peak_max, sm)
            if sm < low:
                self.above = False
                self.last_rep_ms = ms
                self.count += 1
                self.env += cdiv(self.peak_max - self.env, 4)
                self.env = max(ENV_MIN, min(ENV_MAX, self.env))
                self.rep_times.append(ms)
                return True
        return False


def replay_csv(path, min_rep_ms, smoothing):
    rc = RepCounter(min_rep_ms, smoothing)
    with open(path) as f:
        for row in csv.DictReader(f):
            rc.feed(int(row["x"]), int(row["y"]), int(row["z"]), int(float(row["t_ms"])))
    return rc


def selftest():
    import random
    random.seed(42)
    rate = 25
    duration_s = 30
    n_reps = 10
    rc = RepCounter(min_rep_ms=900, smoothing=5)
    # Gravity on -z, ±20 mG noise, and 10 half-sine bursts (600 mG on x)
    # of 0.6 s starting every 2 s from t = 4 s — a caricature of curls.
    burst_starts = [4.0 + 2.0 * k for k in range(n_reps)]
    for i in range(rate * duration_s):
        t = i / rate
        x = random.randint(-20, 20)
        y = random.randint(-20, 20)
        z = -1000 + random.randint(-20, 20)
        for b in burst_starts:
            if b <= t < b + 0.6:
                x += int(600 * math.sin(math.pi * (t - b) / 0.6))
        rc.feed(x, y, z, int(t * 1000))
    status = "OK" if rc.count == n_reps else "FAIL"
    print(f"selftest: {rc.count}/{n_reps} synthetic reps counted — {status}")
    return rc.count == n_reps


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("csv", nargs="?", help="recording CSV (from /recordings/{id}/csv)")
    ap.add_argument("--min-rep-ms", type=int, default=900)
    ap.add_argument("--smoothing", type=int, default=5)
    ap.add_argument("--selftest", action="store_true")
    args = ap.parse_args()

    if args.selftest:
        sys.exit(0 if selftest() else 1)
    if not args.csv:
        ap.error("give a CSV file or --selftest")
    rc = replay_csv(args.csv, args.min_rep_ms, args.smoothing)
    print(f"{args.csv}: {rc.count} reps counted")
    if rc.rep_times:
        print("rep times (s):", ", ".join(f"{t / 1000:.1f}" for t in rc.rep_times))


if __name__ == "__main__":
    main()
