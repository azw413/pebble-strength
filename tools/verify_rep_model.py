#!/usr/bin/env python3
"""Prove the on-device learned-counter feature extractor (src/c/rep_model.c)
matches the Python reference (tools/rep_features.py), and that the model count
matches — before it ships. Compares features rec-by-rec and reports the count
delta under the trained model.

  python3 tools/verify_rep_model.py
"""
import json
import pathlib
import sqlite3
import struct
import subprocess
import sys

import numpy as np

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
from rep_features import features, FEATURE_NAMES  # noqa: E402

CLEAN = {
    4: [10, 11, 13, 14, 17, 18, 22, 23, 24, 37, 127, 128, 129, 161, 162, 163, 363, 364, 365],
    7: [49, 50, 51, 52, 81, 82, 83, 84],
    3: [68, 69, 70, 100, 101, 102, 133, 134, 135],
}


def build():
    exe = pathlib.Path("/tmp/rep_model_ctest")
    subprocess.run(
        ["cc", "-O2", "-o", str(exe), str(ROOT / "tools" / "rep_model_ctest.c"),
         str(ROOT / "src" / "c" / "rep_model.c"), "-I", str(ROOT / "src" / "c")],
        check=True,
    )
    return exe


def c_features(exe, xyz):
    lines = "".join(f"{int(a)} {int(b)} {int(c)}\n" for a, b, c in xyz)
    out = subprocess.run([str(exe)], input=lines, capture_output=True, text=True).stdout.strip()
    if out == "none":
        return None
    return [int(v) for v in out.split()]


def main():
    exe = build()
    model = json.loads((ROOT / "tools" / "rep_model.json").read_text())
    w = np.array(model["weights"]); b = model["bias"]
    con = sqlite3.connect(f"file:{ROOT / 'server' / 'strength.db'}?mode=ro", uri=True)

    feat_mismatch = 0
    count_delta = 0
    total_reps = 0
    n_sets = 0
    print(f"{'rec':>4} {'py-feat':>16} {'c-feat':>16} {'py#':>4} {'c#':>4} {'act':>4}")
    for mv, ids in CLEAN.items():
        for rid in ids:
            row = con.execute("SELECT actual, samples FROM recordings WHERE id=?", (rid,)).fetchone()
            if not row:
                continue
            actual, blob = row
            n = len(blob) // 6
            xyz = np.array([struct.unpack_from("<hhh", blob, i * 6) for i in range(n)], float)
            pf = features(xyz)
            cf = c_features(exe, xyz)
            if pf is None or cf is None:
                continue
            pv = [pf[k] for k in FEATURE_NAMES]
            if pv != cf:
                feat_mismatch += 1
            pcount = round(float(np.array(pv) @ w + b))
            ccount = round(float(np.array(cf) @ w + b))
            count_delta += abs(pcount - ccount)
            total_reps += actual
            n_sets += 1
            flag = "" if pv == cf else "  <-DIFF"
            print(f"{rid:>4} {str(pv):>16} {str(cf):>16} {pcount:>4} {ccount:>4} {actual:>4}{flag}")

    print(f"\n{n_sets} sets, {total_reps} reps")
    print(f"feature mismatches (C vs Python): {feat_mismatch}   <- want 0")
    print(f"model-count delta (C vs Python):  {count_delta} reps   <- want 0")


if __name__ == "__main__":
    main()
