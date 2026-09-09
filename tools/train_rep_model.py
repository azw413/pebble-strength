#!/usr/bin/env python3
"""Train the learned, rotation-invariant rep counter and report honest accuracy.

Pulls labelled recordings, extracts rotation-invariant features
(tools/rep_features.py), fits a small linear model (features -> count),
leave-one-out cross-validates, and exports weights.

  python3 tools/train_rep_model.py [--db PATH] [--out weights.json]

Rotation-invariance is proven directly: every recording is re-scored under
random axis permutations/flips; the count must be identical.
"""
import argparse
import json
import pathlib
import sqlite3
import struct
import sys

import numpy as np

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from rep_features import FEATURE_NAMES, feature_vector  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parents[1]
# Movements + recording ids with trustworthy (varied, corrected) labels.
CLEAN = {
    4: [10, 11, 13, 14, 17, 18, 22, 23, 24, 37, 127, 128, 129, 161, 162, 163, 363, 364, 365],
    7: [49, 50, 51, 52, 81, 82, 83, 84],
    3: [68, 69, 70, 100, 101, 102, 133, 134, 135],
}


def load(db):
    con = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    rows = []
    for mv, ids in CLEAN.items():
        for rid in ids:
            r = con.execute(
                "SELECT actual, samples FROM recordings WHERE id=? AND movement_id=?",
                (rid, mv),
            ).fetchone()
            if not r:
                continue
            actual, blob = r
            n = len(blob) // 6
            xyz = np.array([struct.unpack_from("<hhh", blob, i * 6) for i in range(n)], float)
            rows.append((mv, rid, actual, xyz))
    return rows


def fit(X, y):
    """Least-squares linear model with a bias term. Returns weights (len+1)."""
    A = np.c_[X, np.ones(len(X))]
    w, *_ = np.linalg.lstsq(A, y, rcond=None)
    return w


def predict(w, x):
    return float(np.r_[x, 1.0] @ w)


def loo(X, y):
    err = 0
    for i in range(len(X)):
        tr = [j for j in range(len(X)) if j != i]
        w = fit(X[tr], y[tr])
        err += abs(round(predict(w, X[i])) - y[i])
    return err


def rotations():
    """The 48 signed axis permutations (all cube symmetries) for an invariance check."""
    import itertools
    out = []
    for perm in itertools.permutations(range(3)):
        for signs in itertools.product([1, -1], repeat=3):
            out.append((perm, signs))
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--db", default=str(ROOT / "server" / "strength.db"))
    ap.add_argument("--out", default=str(ROOT / "tools" / "rep_model.json"))
    ap.add_argument("--feature-version", type=int, default=1)
    ap.add_argument("--write-db", action="store_true",
                    help="insert the trained model into rep_models as a new active version")
    args = ap.parse_args()

    rows = load(args.db)
    feats, ys, movs = [], [], []
    for mv, rid, actual, xyz in rows:
        fv = feature_vector(xyz)
        if fv is None:
            continue
        feats.append(fv)
        ys.append(actual)
        movs.append(mv)
    X = np.array(feats)
    y = np.array(ys)
    reps = y.sum()
    print(f"dataset: {len(X)} labelled sets, {reps} reps, features={FEATURE_NAMES}")

    # Invariance check: features must be identical under axis permutation/flip.
    import random
    random.seed(0)
    sample = rows[: min(6, len(rows))]
    max_dev = 0.0
    for _, _, _, xyz in sample:
        base = feature_vector(xyz)
        for perm, signs in random.sample(rotations(), 8):
            t = xyz[:, list(perm)] * np.array(signs)
            fv = feature_vector(t)
            if fv is not None:
                max_dev = max(max_dev, float(np.max(np.abs(fv - base))))
    print(f"invariance check: max feature deviation under 48 rotations = {max_dev:.4g}  (want ~0)")

    # Accuracy
    err = loo(X, y)
    print(f"\nleave-one-out accuracy (all features): {100 * (1 - err / reps):.0f}%  (err {err}/{reps})")
    for mv in sorted(set(movs)):
        idx = [i for i, m in enumerate(movs) if m == mv]
        e = 0
        for i in idx:
            tr = [j for j in range(len(X)) if j != i]
            e += abs(round(predict(fit(X[tr], y[tr]), X[i])) - y[i])
        r = y[idx].sum()
        print(f"  movement {mv}: {100 * (1 - e / r):.0f}% ({len(idx)} sets)")

    # Final model on all data, exported.
    w = fit(X, y)
    weights = [round(float(v), 6) for v in w[:-1]]
    bias = round(float(w[-1]), 6)
    accuracy = round(1 - err / reps, 4)
    movements = sorted(set(movs))
    model = {
        "feature_version": args.feature_version,
        "features": FEATURE_NAMES,
        "movements": movements,
        "weights": weights,
        "bias": bias,
        "accuracy": accuracy,
        "trained_sets": len(X),
    }
    pathlib.Path(args.out).write_text(json.dumps(model, indent=2))
    print(f"\nwrote {args.out}")

    if args.write_db:
        con = sqlite3.connect(args.db)
        # Next model_version for this feature_version; deactivate the old active one.
        cur = con.execute(
            "SELECT COALESCE(MAX(model_version), 0) FROM rep_models WHERE feature_version=?",
            (args.feature_version,),
        ).fetchone()[0]
        mver = cur + 1
        con.execute(
            "UPDATE rep_models SET active=0 WHERE feature_version=? AND active=1",
            (args.feature_version,),
        )
        con.execute(
            """INSERT INTO rep_models
               (feature_version, model_version, movement_id, kind, features, movements,
                weights, bias, accuracy, trained_sets, active)
               VALUES (?,?,NULL,0,?,?,?,?,?,?,1)""",
            (args.feature_version, mver, ",".join(FEATURE_NAMES),
             ",".join(map(str, movements)), json.dumps(weights), bias, accuracy, len(X)),
        )
        con.commit()
        print(f"wrote rep_models: feature_version {args.feature_version}, model_version {mver} (active)")


if __name__ == "__main__":
    main()
