#!/usr/bin/env python3
"""Prove the on-device C engine (src/c/rep_counter.c) matches the Python
reference (rep_causal.py) on the real recordings, before it ships.

For each recording of a movement, run both counters with that movement's tuned
CounterConfig (from shared/exercises.json) and compare to the human label.

  python3 tools/verify_rep_counter.py
"""
import json
import pathlib
import sqlite3
import struct
import subprocess
import sys
import tempfile

import numpy as np

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
from rep_causal import CounterConfig, count_causal  # noqa: E402

AXIS = {"auto": 0, "x": 1, "y": 2, "z": 3, "mag": 4, "linear": 4}
DB = ROOT / "server" / "strength.db"


def build_harness():
    exe = pathlib.Path(tempfile.gettempdir()) / "rep_ctest"
    subprocess.run(
        ["cc", "-O2", "-o", str(exe), str(ROOT / "tools" / "rep_ctest.c"),
         str(ROOT / "src" / "c" / "rep_counter.c"), "-I", str(ROOT / "src" / "c")],
        check=True,
    )
    return exe


def cfg_for(profile):
    return CounterConfig(
        axis_mode=AXIS.get(profile.get("axis", "mag"), 0),
        lp_ms=profile.get("lp_ms", 500),
        hp_ms=profile.get("hp_ms", 3000),
        thr_pct=profile.get("thr_pct", 40),
        min_rep_ms=profile.get("min_rep_ms", 900),
        min_amp=profile.get("min_amp", 150),
        warmup_ms=profile.get("warmup_ms", 0),
    )


def c_count(exe, cfg, xyz):
    lines = "".join(f"{int(a)} {int(b)} {int(c)}\n" for a, b, c in xyz)
    out = subprocess.run([str(exe), str(cfg.axis_mode), str(cfg.lp_ms), str(cfg.hp_ms),
                          str(cfg.thr_pct), str(cfg.min_rep_ms), str(int(cfg.min_amp)),
                          str(cfg.warmup_ms)], input=lines, capture_output=True, text=True)
    return int(out.stdout.strip())


def main():
    exe = build_harness()
    catalog = json.load(open(ROOT / "shared" / "exercises.json"))["exercises"]
    con = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)

    total_c_vs_py = 0
    total_c_err = total_py_err = total_reps = 0
    for ex in catalog:
        if "profile" not in ex:
            continue
        mv = ex["id"]
        recs = con.execute(
            "SELECT id, actual, samples FROM recordings WHERE movement_id=? AND is_timed=0 ORDER BY id",
            (mv,),
        ).fetchall()
        if not recs:
            continue
        cfg = cfg_for(ex["profile"])
        print(f"\n== {ex['name']} (mv {mv}, axis {ex['profile'].get('axis')}) ==")
        print(f"{'rec':>4} {'actual':>6} {'python':>6} {'C':>4} {'match':>6}")
        for rid, actual, blob in recs:
            n = len(blob) // 6
            xyz = np.array([struct.unpack_from("<hhh", blob, i * 6) for i in range(n)], float)
            py = count_causal(xyz, cfg)
            c = c_count(exe, cfg, xyz)
            match = "ok" if py == c else "DIFF"
            total_c_vs_py += abs(py - c)
            total_c_err += abs(c - actual)
            total_py_err += abs(py - actual)
            total_reps += actual
            print(f"{rid:>4} {actual:>6} {py:>6} {c:>4} {match:>6}")

    print(f"\n--- summary over {total_reps} labelled reps ---")
    print(f"C vs Python disagreement: {total_c_vs_py} reps   <- want 0")
    print(f"Python abs error: {total_py_err}   |   C abs error: {total_c_err}")


if __name__ == "__main__":
    main()
