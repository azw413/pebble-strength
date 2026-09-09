# Orientation-independent rep counting (learned model)

## Problem

The fixed-axis counter is accurate (~90%) when the watch is worn as tuned, but
counting on a hardcoded axis is fragile to **how the device is worn**:

- **Sign flips** (upside down / screen-down) — *robust already*: the symmetric
  hysteresis counts a full down-up oscillation regardless of direction (verified
  ~95% under negated axes).
- **Axis remaps** (different wrist, rotated posture) — *breaks it*: the rep
  motion moves to a different axis and the fixed-axis counter watches the wrong
  one (2–50%). This is also why the 'Seated' workout failed (posture rotated the
  frame ~90°).

Every attempt to pick the axis *live* — PCA (max-variance), autocorrelation
(rhythm), auto (max-var) — underperforms, because off-axis wobble has energy in
the same rep band as the reps. No frequency filter separates them (the wobble is
higher-frequency on the off-axis, but the fixed axis is already the clean one, so
filtering can't help axis *selection*). Conclusion: you can't reliably recover
the hand-picked axis from the signal alone.

## Approach: features that don't depend on orientation, + a small model

Don't pick an axis at all. Compute **rotation-invariant features** of the set and
let a small model — trained on the real `(recording → corrected count)` pairs we
already collect — map them to the count. Invariance is *by construction*: the
features are unchanged under any rotation/flip of the watch frame, so the counter
works on any wrist/posture without per-user calibration.

### Rotation-invariant features (set-level)

All computed on the captured accel buffer at set end (the recorder already holds
the whole set), over the active window (segmentation on |linear accel|):

- **duration** of the active window
- **rep period** from the autocorrelation peak of the band-passed |accel|
  magnitude → `duration / period`
- **spectral peak frequency** × duration
- **magnitude oscillation energy** (std of band-passed |accel|)
- **invariant peak count** of band-passed |accel| (overcounts, but informative)

Magnitude and tilt-angle signals are exactly invariant (verified: axis-swapped
inputs produce identical feature values).

### Model

A tiny linear regressor (~6 weights) — or a 1-hidden-layer MLP if needed —
mapping features → count. Per-movement or one shared model. Small enough to
quantise to int and evaluate once per set on-watch.

## Evidence (data we have: 36 labelled sets, push-up/curl/OHP)

Leave-one-out cross-validation (so it generalises):

| Model | Accuracy |
| --- | --- |
| duration only | 89% |
| peak-count only | 69% |
| **all invariant features (linear)** | **93%** |
| per-movement (shared model) | push-up 91%, curl 95%, OHP 95% |

Matches fixed-axis accuracy while being orientation-independent. No single
feature suffices; the model combines them.

### Known limitations

1. **Duration-leaning** — 89% from duration alone (steady tempo → duration ≈
   count). Rhythm features add tempo-awareness (→93%) but irregular/paused sets
   are the weak spot; fixed-axis handles those better.
2. **Small, low-diversity data** — 36 sets, only ~2 real wear orientations.
   Invariance is guaranteed by construction, but breadth (more movements, genuine
   different-wrist data) is needed to be rock-solid.
3. **Set-level, not streaming** — no accurate *live* count during the set; the
   authoritative count appears at set end (fits the confirm-on-rest-screen flow).

## Pipeline

1. **Feature extractor** (`tools/rep_features.py`) — pure, rotation-invariant,
   shared spec between offline training and the on-watch port.
2. **Train** (`tools/train_rep_model.py`) — pull labelled recordings, extract
   features, fit the model, report LOO accuracy, export weights (JSON).
3. **Store** — model weights server-side (extend `counter_configs`, or a
   `rep_models` table); versioned.
4. **Download** — weights ride the existing counter-config rail to the watch.
5. **On-watch** — at set end, compute the features in C on the captured buffer,
   run the model (a dot product), pre-fill the rest screen. Falls back to the
   fixed-axis counter when no model is present (safe default).

## Build order (prove-as-we-go)

1. `rep_features.py` + `train_rep_model.py`: validate LOO accuracy on current
   data, export weights. **← as far as the data proves.**
2. On-watch C feature extractor + model eval, validated against the Python
   reference (like `verify_rep_counter.py`).
3. Rail + storage for weights; wire into the session UI as the set-end count.
4. Grow the corpus (esp. genuinely different wear orientations), retune, ship
   weights over the rail.
