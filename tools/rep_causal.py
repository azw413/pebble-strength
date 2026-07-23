#!/usr/bin/env python3
"""Causal rep counter — the *live* counterpart to pushup_count.py.

pushup_count.py is a batch counter: it centres its smoothing windows (peeks at
future samples), segments the whole set, and trims setup+dismount off *both*
ends. The watch can do none of that — it sees samples one at a time and can
never look ahead. This module is the honest version: everything here is a
running state update over a forward pass, so its score is what the watch would
actually get.

It's deliberately a direct image of the C engine we'll ship (rep_counter.c from
a CounterConfig), so tuning here ports as constants:

  band-pass : two causal EMAs. lp = light smooth of the chosen axis (kills
              high-freq noise); base = slow EMA of lp (tracks gravity/drift).
              osc = lp - base is the rep-band signal.
  warmup    : ignore the first warmup_ms (settle the EMAs / get into rep 1).
              Replaces the batch start-trim; the on-watch lead-in already keeps
              most setup out of the recording.
  count     : hysteresis on osc — dip below -h, rise back above +h = one rep,
              with a min_rep_ms refractory gap. No end-trim; a trailing rep is
              accepted live and the on-wrist Up/Down is the backstop.
  threshold : h = max(min_amp, thr_pct% * amp_est), where amp_est is a running
              EMA of swing depth — adapts to the athlete without seeing ahead.
  axis      : fixed (x/y/z/|linear|) or auto = lock the max-variance axis after
              a short selection window (still causal).
"""
import argparse
import struct
import sys
import pathlib
from dataclasses import dataclass

import numpy as np

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from segment import linear_mag  # noqa: E402

RATE = 25


# Defaults = the push-up tune (axis z), fitted causally against the labelled
# corpus: 92% of reps, 5/7 sets exact, vs the batch counter's 98%. Other
# movements start from these until they have their own tuned config.
@dataclass
class CounterConfig:
    axis_mode: int = 3      # 0 auto / 1 x / 2 y / 3 z / 4 |linear|
    lp_ms: int = 500        # fast smoothing time-constant
    hp_ms: int = 3000       # slow baseline time-constant (drift/gravity)
    thr_pct: int = 30       # hysteresis threshold, % of running amplitude
    min_rep_ms: int = 900   # refractory gap between reps
    min_amp: float = 70.0   # noise floor (mG); threshold never dips below this
    warmup_ms: int = 700    # ignore this long at the start
    sel_ms: int = 2500      # auto-axis: variance-accumulation window


def _ema_alpha(tau_ms, rate):
    dt = 1.0 / rate
    tau = tau_ms / 1000.0
    return dt / (tau + dt)


def count_causal(xyz, cfg: CounterConfig, rate=RATE, trace=False):
    """xyz: (N,3) accel in mG. Single forward pass. Returns the rep count."""
    xyz = np.asarray(xyz, float)
    n = len(xyz)
    if n < rate:
        return 0

    # Per-axis candidate signals (mG). |linear| = magnitude (gravity removed by
    # the slow baseline EMA just like any axis).
    axes = [xyz[:, 0], xyz[:, 1], xyz[:, 2], linear_mag(xyz)]

    a_lp = _ema_alpha(cfg.lp_ms, rate)
    a_hp = _ema_alpha(cfg.hp_ms, rate)
    warm = int(cfg.warmup_ms / 1000.0 * rate)
    sel = int(cfg.sel_ms / 1000.0 * rate)
    min_gap = cfg.min_rep_ms / 1000.0 * rate

    # Axis selection (causal): fixed, or accumulate variance and lock after sel.
    if cfg.axis_mode in (1, 2, 3, 4):
        axis = cfg.axis_mode - 1 if cfg.axis_mode <= 3 else 3
        axis_locked = True
    else:
        axis = 3  # provisional until the selection window closes
        axis_locked = False

    # Running band-pass state per axis (so an auto pick has warm EMAs ready).
    lp = [ax[0] for ax in axes]
    base = [ax[0] for ax in axes]
    sq = [0.0, 0.0, 0.0, 0.0]  # variance accumulators over the selection window

    count = 0
    state = "high"
    trough = 0.0
    last_rep = -1e9
    amp_est = cfg.min_amp
    reps = []

    for i in range(n):
        for k in range(4):
            lp[k] += a_lp * (axes[k][i] - lp[k])
            base[k] += a_hp * (lp[k] - base[k])
        if not axis_locked:
            for k in range(4):
                o = lp[k] - base[k]
                sq[k] += o * o
            if i >= sel:
                axis = int(np.argmax(sq[:3]))  # auto restricts to real axes
                axis_locked = True

        if i < warm or not axis_locked:
            continue

        osc = lp[axis] - base[axis]
        h = max(cfg.min_amp, cfg.thr_pct / 100.0 * amp_est)
        if state == "high":
            if osc < -h:
                state = "low"
                trough = osc
        else:  # "low" — track the trough, wait for the rise
            trough = min(trough, osc)
            if osc > h:
                if i - last_rep >= min_gap:
                    count += 1
                    last_rep = i
                    amp_est += 0.35 * (abs(trough) - amp_est)  # adapt to depth
                    reps.append(i)
                state = "high"

    if trace:
        return count, reps, axis
    return count


# ---- Evaluation ----

CLEAN = {10, 11, 13, 14, 15, 17, 18}  # single-buzz firmware, trustworthy labels


def _load(db, mv=4):
    import sqlite3
    con = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    out = []
    for rid, actual, blob in con.execute(
        "SELECT id, actual, samples FROM recordings WHERE movement_id=? AND is_timed=0 ORDER BY id",
        (mv,),
    ):
        n = len(blob) // 6
        xyz = np.array([struct.unpack_from("<hhh", blob, i * 6) for i in range(n)], float)
        out.append((rid, actual, xyz))
    return out


def evaluate(recs, cfg, rate=RATE):
    rows, tot, clean = [], 0, 0
    for rid, actual, xyz in recs:
        c = count_causal(xyz, cfg, rate)
        err = c - actual
        tot += abs(err)
        if rid in CLEAN:
            clean += abs(err)
        rows.append((rid, actual, c, err))
    return rows, tot, clean


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--db", default=str(pathlib.Path(__file__).resolve().parents[1] / "server" / "strength.db"))
    ap.add_argument("--axis", type=int, default=3)
    args = ap.parse_args()
    cfg = CounterConfig(axis_mode=args.axis)
    recs = _load(args.db)
    rows, tot, clean = evaluate(recs, cfg)
    print(f"CounterConfig: {cfg}")
    print(f"{'rec':>4} {'actual':>6} {'causal':>6} {'err':>4}   set")
    for rid, actual, c, err in rows:
        tag = "clean" if rid in CLEAN else "old"
        print(f"{rid:>4} {actual:>6} {c:>6} {err:>+4}   {tag}")
    print(f"\n total abs error: {tot}   |   clean-only: {clean}")


if __name__ == "__main__":
    main()
