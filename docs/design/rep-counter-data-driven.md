# Data-driven rep counting

## Goal

Make the on-watch rep counter **per-exercise and updatable without reflashing**:
the watch runs one generic counting *engine*; each exercise's tuning (params, or
a tiny model's weights) is **data downloaded with the workouts**. As the upload
corpus grows we retune server-side, and every watch picks up the improvement on
its next sync — no app-store release.

## Why now

- We already stream a labelled corpus up (raw 25 Hz accel + corrected counts).
- M3 already downloads packed workouts to the watch and persists them
  (`workouts_store`). Counter config rides the same rail.
- `tools/pushup_count.py` shows a *parametric* counter can be tuned from data and
  score within ~1 rep on clean sets — proof the parametric tier is worth shipping
  before any ML.

## The three model tiers (ship in order)

**Tier A — parametric config (do first).** The engine is the push-up algorithm,
generalised: band-pass each axis to the rep band, pick an axis, hysteresis-count
down-up swings, amplitude-gate the zero case. Each exercise supplies a small
struct:

```
CounterConfig v0 (~12 bytes):
  type        u8   // 0 = parametric
  axis_mode   u8   // 0 auto(max-var) · 1 x · 2 y · 3 z · 4 |linear a|
  lp_ms       u8   // low-pass window (rep-band top)
  hp_ds       u8   // high-pass window in 0.1s units (rep-band bottom / drift)
  thr_pct     u8   // hysteresis threshold, % of the 95th-pct amplitude
  min_rep_ms  u16  // min spacing between reps
  min_amp     u16  // noise floor (mG); below this -> 0 reps
  warmup_ms   u16  // ignore this long at set start (getting into position)
```

Tiny, interpretable, trivially fast, and directly fittable from the corpus
(exactly what `pushup_count.py` produces — export its constants per exercise).

**Tier B — tiny learned model on hand features (bridge).** When Tier A plateaus
for a movement, keep the same feature front-end (band-passed axes, short-window
energy, dominant-axis, autocorr period) and replace the hand threshold with a
small learned classifier — logistic regression or a 1-hidden-layer MLP (≤~32
weights) emitting a rep-event score. Trains with far less data than an RNN, still
a handful of MACs per sample. Weights quantised to int8, downloaded per exercise.

**Tier C — tiny quantised recurrent net ("qrnn").** A small GRU/1D-conv (e.g. 3
inputs → 8 hidden, int8) over the accel stream emitting P(rep boundary). Weights
~a few hundred–few thousand int8, downloaded per exercise (or one model
conditioned on an exercise-id embedding). Compute is fine on a Cortex-M at 25 Hz
(a few hundred int8 MACs/sample). **Blocker is data, not the watch**: an RNN
needs hundreds of sets with *per-rep* labels; we have single digits per exercise
today. Tier C is the destination, not the starting point.

## The hard constraint the offline tools must respect: it runs *live*

`pushup_count.py` trims setup **and** dismount off both ends — a batch luxury.
On the watch the counter runs in real time and **cannot see the future**:

- **Start** (getting into position) → handle with `warmup_ms` (ignore the first
  N ms) instead of a trim.
- **End** (the last rep's follow-through / reaching to tap Select) → the recording
  ends at Select, so a trailing motion can still be mis-counted live. Mitigate
  with a short trailing debounce and rely on the on-wrist Up/Down correction.

So the *training/tuning target* must be the **causal** counter (warmup + debounce,
no end-trim), evaluated the way it will actually run — otherwise offline scores
overstate on-watch accuracy. Add a `--causal` mode to the tools and tune against
that.

## Plumbing (shared by all tiers)

1. **Corpus → fit** (offline, `tools/`): per exercise, fit Tier-A params (or
   train B/C) from its recordings; emit a `CounterConfig` blob. Version it.
2. **Store**: keep each exercise's active `CounterConfig` server-side (a column
   on `exercises`, or a small `counter_configs` table keyed by movement + version).
3. **Download**: extend the device API so each downloaded exercise carries its
   `CounterConfig` — either appended to the packed workout per exercise, or a
   parallel `GET /api/device/counters` the watch fetches alongside workouts. A
   parallel endpoint is cleaner (configs change on a different cadence than
   workouts and are shared across users).
4. **Watch**: persist configs (like `workouts_store`); `rep_counter` becomes a
   generic engine that takes a `CounterConfig`. Unknown/absent config → fall back
   to today's built-in constants (`movements.h`), so it degrades safely offline.
5. **Version + safety**: config carries a `type`/version; the engine ignores
   configs it can't run. Ship a size cap so a config always fits one AppMessage.

## Recommended path

1. Refactor `rep_counter.c` to run from a `CounterConfig` (Tier A engine), with
   the current `movements.h` constants as the built-in fallback.
2. Add `--causal` to the offline tools; port the push-up params as the first
   real config and confirm the causal score on-watch.
3. Wire the config into the device download + `workouts_store`-style persistence.
4. Grow the corpus (every logged set helps), retune per exercise, and only reach
   for Tier B/C on movements where parametric genuinely can't cope (e.g. subtle
   chin-ups) once there's enough labelled data to justify it.

The end state matches the ask exactly: **download the counting "data" with the
workouts.** It just starts as a dozen bytes of params and grows into a few KB of
quantised weights when the data earns it.
