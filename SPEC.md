# Strength — a Garmin-style strength training app for Pebble

## 1. Overview

Strength lets you program workouts on the Strength website (Garmin
Connect-style) and sync them to the watch through the phone app. The watch then
guides you through each exercise and set — counting reps automatically with the
accelerometer, timing rest periods and timed holds — and uploads each finished
session to your browsable/exportable history on the site, where charts track
volume, reps, and intensity over time. Workouts can be made public and forked
by other users.

**Target platforms:** `basalt` (Pebble Time / Time Steel), `chalk` (Time Round),
`diorite` (Pebble 2, B&W), `emery` (Time 2 / Core Time 2).
Not targeted: `aplite` (original Pebble — 24 KB app heap is too tight).
The SDK also offers `flint`/`gabbro` (2025 Core Devices hardware); adding them
later is a one-line `targetPlatforms` change plus a layout pass.

**Design principles**

- The watch is a lean *execution* device; all workout *authoring* happens on
  the website, and the phone app is only a sync relay (like Garmin Connect →
  device sync).
- Fully usable mid-workout with three buttons and no phone in reach.
- Auto rep counting is an assist, never a gate: every count is correctable with
  Up/Down at any moment, and everything works if the counter is wrong.
- Fixed-point integer math only on the watch (no float in the hot path).

## 2. Feature set (v1)

| Area | v1 scope |
|---|---|
| Workout programming | Website: unlimited saved workouts, each up to 16 exercises, with up to 5 assigned to watch sync slots; **per-set targets** (e.g. push-ups 12/10/8) with per-set rest; sets are rep-based or **timed holds** (e.g. L-sit 10 s); optional weight per exercise |
| Accounts & sync | Google sign-in on the website; watch linked once via a pairing code; workouts download / sessions upload through the phone app |
| Exercise library | ~30 built-in movements seeded into the server DB with body area, muscle groups worked, and a rep-counting profile; the watch's table is generated from the same seed; "Custom" movement with generic profile |
| Workout sharing | Workouts have a title, description, and public flag; public workouts browsable on an Explore page and forkable into your own slots |
| Analytics | Website charts: volume, reps, sets, and intensity (work ÷ (work + rest)) over time; per-exercise progression |
| Guided execution | Exercise → set → rest flow with big rep counter, set x/y, target reps, weight on screen; timed holds count down with vibe cues |
| Rep counting | Accelerometer peak detection at 25 Hz, active only during a set; manual ±1 correction |
| Rest timer | Per-exercise rest duration, countdown with vibration at 10 s and 0 s, skippable |
| Session recording | Per-set actual reps + weight; summary screen (duration, total reps, total volume) |
| History | Sessions uploaded to the server; browsable + CSV export on the website; last 3 summaries viewable on watch |
| Settings | Units kg/lb, vibration on/off, auto-count on/off |

**Explicitly out of scope for v1** (candidates for v2): automatic exercise
*recognition* (Garmin's auto-detect — we count reps for the exercise you
programmed), automatic set-end detection via motion idle, heart rate (Pebble 2),
timeline pins, on-watch workout editing, supersets/circuits, user-defined
exercises, per-user tuned counting profiles (the schema leaves room for both).

## 3. Architecture

Three pieces in one repo — the watch executes, the server is the source of
truth, and the phone app is the pipe between them:

```
┌─────────────────┐  AppMessage  ┌────────────────┐  HTTPS, Bearer  ┌────────────────────┐
│  Watch app (C)  │◄────────────►│  PebbleKit JS  │  device token   │ Server (Rust/axum) │
│  UI, execution, │              │  (phone side)  │◄───────────────►│ web UI · device API│
│  rep counter,   │              │  sync relay    │                 │ SQLite · binary    │
│  persist store  │              └───────┬────────┘                 │ packer             │
└─────────────────┘                      │ settings opens           └─────────▲──────────┘
                                         ▼ /link webview                      │ Google sign-in
                                   pairing-code page ─────────────────────────┘ (any browser)
```

- **Watch app (C, SDK 4.x).** Owns the synced workout store (persist API), the
  execution state machine, the rep-counting engine, and all UI. Standalone once
  workouts are synced — no phone needed during a session.
- **PebbleKit JS** (bundled `src/pkjs/index.js`, runs inside the phone app).
  A thin sync relay: holds the device token, downloads packed workouts from the
  server and streams them to the watch over AppMessage, uploads finished
  sessions. No app logic and no format knowledge beyond chunking.
- **Server (Rust + axum, `server/` crate in this repo).** Google sign-in, the
  workout-builder web UI, session history, and the device API (§8). The only
  component besides the watch that knows the packed binary format: it packs
  workouts in Rust, mirrored against the C structs and round-trip tested, so
  format changes land in one commit. SQLite storage; deploys as a single
  binary. In development it runs on localhost and `pebble emu-app-config`
  points the emulator's settings page at it.

## 4. Data model

### 4.1 Movement library (compiled into the watch app)

Each movement: `id (u8)`, display name, body-area group, and a counting profile
(see §6): dominant axis hint, minimum rep period, smoothing preset. Initial set
(~30): bench press, incline press, overhead press, push-up, dip, lateral raise,
biceps curl, hammer curl, triceps extension/pushdown, row (barbell/dumbbell),
lat pulldown, pull-up, shrug, deadlift, Romanian deadlift, squat, front squat,
goblet squat, lunge, leg press, leg curl, leg extension, calf raise, hip
thrust, kettlebell swing, crunch, plank (timed, not counted), Russian twist,
face pull, **custom** (generic profile).

One shared JSON seed file is the canonical source: the server's `exercises`
table (§8.4) is seeded from it — including muscle groups and counting-profile
parameters as *data* — and the watch's compiled-in table is generated from it
at build time, so movement IDs always agree.

### 4.2 Workout (packed binary, one persist key each — ≤ 256 B)

Packed by the server (Rust), unpacked by the watch (C); both sides mirror the
same layout and are round-trip tested against shared fixtures. Targets are
**per set**, so pyramids (12/10/8) and mixed rep/timed work are natural.

```
WorkoutHeader: name[24] · version u8 · exerciseCount u8 · reserved u16  = 28 B
ExerciseRec:   movementId u8 · flags u8 (bit0: timed — set targets are
               seconds; bit1: AMRAP) · weight u16 (0.25 kg units,
               0 = bodyweight) · setCount u8 · customNameIdx u8         =  6 B
SetRec:        target u8 (reps, or hold seconds when timed) ·
               rest u8 (5 s units → 0–1275 s)                           =  2 B
```

A typical workout (6 exercises × 4 sets) packs to ≈ 112 B; the website
enforces a 228 B cap at save time, which no sane workout approaches. Custom
exercise names live in a separate string-pool key. Weight is stored
metric-only; lb is a display conversion.

### 4.3 Session record (pending-sync ring on watch, then phone-side JSON)

```
SessionHeader:  startTime u32 · workoutSlot u8 · durationSecs u16 ·
                setCountTotal u8                                        =  8 B
SetRec:         exerciseIdx u8 · setIdx u8 · actualReps u8 · autoReps u8 ·
                weight u16 · startOffset u16 (secs since session start) ·
                duration u16 (secs, set start → set end)                = 10 B
```

`startOffset`/`duration` exist for the intensity analytics (§8.6): work time is
the set duration (hold time for timed sets), and actual rest is derived from
the gap to the next set's start — capturing skipped or overrun rest screens,
not just the programmed values. One 256 B persist key holds a session of up to
24 recorded sets (a 6-exercise × 4-set workout exactly); longer sessions spill
into a second key.

`autoReps` (what the counter said) is kept alongside `actualReps` (after user
correction) — this is the ground-truth data for tuning the algorithm. For timed
sets the same fields carry target vs actually-held seconds. The watch keeps up
to 4 unsynced sessions; on each app launch with the phone reachable, pending
sessions upload (watch → phone JS → `POST /api/device/sessions`) and are then
deleted from the watch. History lives in the server DB, browsable on the
website.

### 4.4 Persist budget (4 KB total, 256 B/key)

| Keys | Use | Budget |
|---|---|---|
| 1–5 | Workout slots | 5 × ≤ 228 B |
| 6 | Custom-name string pool | 256 B |
| 7–10 | Pending session ring (1–2 keys/session, 4 sessions typical) | ~1 KB |
| 11 | Settings + sync state | ~32 B |

Total ≈ 2.5 KB worst case — well inside the limit; sessions must sync and
clear before the ring fills (the watch warns at 3 pending).

## 5. Sync protocol (AppMessage)

Inbox/outbox sized 2048 B. All multi-item transfers are sequenced and ACKed;
either side retries a NACKed or timed-out chunk (3 attempts).

| Key | Direction | Meaning |
|---|---|---|
| `MSG_TYPE` u8 | both | 0 = workouts-push, 1 = session-upload, 2 = ack, 3 = nack, 4 = request-sessions, 5 = settings |
| `SEQ` / `TOTAL` u8 | both | chunk index / count |
| `SLOT` u8 | phone→watch | workout slot being written (one workout per message — 156 B payload fits easily) |
| `PAYLOAD` bytes | both | packed Workout / Session blob |
| `SETTINGS` u8 | phone→watch | bitfield: units, vibes, auto-count |

Flow A (download): on app launch, on `webviewclosed` (settings page dismissed),
or on a manual "Sync" menu action → JS fetches `GET /api/device/workouts`,
skips slots whose server-side hash matches the last acked sync, pushes changed
slots → watch persists each and ACKs → JS records the new hashes.
Flow B (upload): JS sends `request-sessions` → watch streams pending sessions →
JS ACKs each one only after a successful `POST /api/device/sessions` → watch
frees the slot. Offline is fine: sessions just wait in the ring.

## 6. Rep-counting engine

Runs only while a set is active (subscribe on set start, unsubscribe on set
end/rest — this is also the battery strategy).

- **Input:** `accel_data_service_subscribe` at **25 Hz**, batches of 25
  (one callback/second), using samples where `did_vibrate == false`.
- **Pipeline** (all int32 fixed-point):
  1. Gravity removal: per-axis EMA (α ≈ 1/16) subtracted from the raw signal —
     leaves linear acceleration.
  2. Signal selection: magnitude of linear acceleration by default; a
     movement profile may weight a dominant axis.
  3. Smoothing: 5-tap moving average (≈ 3 Hz cutoff at 25 Hz).
  4. Peak detection with **adaptive threshold + hysteresis**: threshold floats
     at k × running envelope (EMA of recent peak amplitudes), so light lateral
     raises and heavy deadlifts both register. A rep = signal crossing up
     through threshold, peaking, and crossing back below the lower hysteresis
     band.
  5. **Refractory period** from the movement profile (default 900 ms, e.g.
     1200 ms for deadlift, 700 ms for curls): crossings inside it are the same
     rep. Crossings are also ignored for the first ~1.5 s of a set (settling
     after pressing Start).
- **Feedback:** short vibe pulse on each counted rep (toggleable) so you get
  count confidence without looking.
- **Correction:** Up/Down = ±1 at any time during set or rest (rest edits the
  just-finished set).
- **Accuracy target:** ±1 rep per set on the common barbell/dumbbell movements;
  measured from the `autoReps` vs `actualReps` telemetry (§4.3).
- **Recording mode → training corpus:** a "Record set" mode on the watch
  captures the raw 25 Hz samples during a set and ships them through the phone
  JS to `POST /api/device/recordings`, labelled with the exercise and the
  actual rep count (or hold seconds) entered afterwards. The server keeps
  every recording (§8.4), so the counting pipeline can be replayed offline
  against labelled ground truth — and, longer term, the corpus feeds
  automatic movement *classification* (v2): recognising which exercise you're
  doing from the signal alone.

## 7. Watch UI

Standard `MenuLayer`-based navigation; layouts use `PBL_IF_ROUND_ELSE` for
chalk and `PBL_IF_COLOR_ELSE` for diorite.

1. **Home** — menu: workout list (name + exercise count), History, Settings.
2. **Workout preview** — exercise list with sets × reps @ weight; Select on
   "Start" begins the session.
3. **Active set** *(the main screen)* — exercise name, "Set 2 of 4", target
   reps + weight, and a huge live rep count.
   - Up / Down: correct count ±1
   - Select: finish set → rest screen
   - Long Select: menu (skip exercise, restart set, end workout)
   - Back: pause overlay (resume / end & save / discard)

   For **timed sets** the big number is a hold countdown instead: Select arms
   the hold with a 3-2-1 vibe lead-in, the target seconds count down, a long
   buzz marks zero, and the app auto-advances to rest. Select during the hold
   ends it early and records the elapsed seconds as the actual.
4. **Rest** — big countdown, next up ("Set 3 of 4 — Bench 8 @ 60 kg"), actual
   reps of the finished set (editable). Select skips. Vibe at 10 s and 0 s;
   auto-advances to the next Active-set screen.
5. **Summary** — duration, total reps, total volume, per-exercise lines;
   session queued for sync.
6. **History** — last 3 synced-or-pending summaries.
7. **Settings** — units, vibes, auto-count.

## 8. Website & server

**Stack:** Rust + axum, **Diesel ORM on SQLite** (embedded migrations; the
schema avoids SQLite-isms so a later move to Postgres is a mechanical
re-migration), server-rendered pages (askama templates) with a little vanilla
JS for the workout builder and uPlot for charts. One static binary + one DB
file; deployable on any small VPS or Fly.io. SQLite calls are blocking — they
run on an r2d2 pool via `spawn_blocking`, which is plenty at this scale.

### 8.1 Authentication

Two separate credentials, because the Pebble phone app's settings webview and
the PebbleKit JS runtime do **not** share cookies:

- **Human ↔ website:** "Sign in with Google" (OIDC authorization-code flow);
  the Google account's verified email is the user identity, a session cookie
  does the rest. Works from any browser.
- **Watch ↔ device API:** a random 128-bit **device token**, generated by
  PebbleKit JS on first run, stored in phone localStorage, and sent as
  `Authorization: Bearer …` on every API call. Useless until linked to an
  account; revocable from the website's Devices page.

**Linking (pairing-code flow).** An email address is public information, so
typing one into the settings page cannot authenticate the device; and Google
blocks OAuth inside embedded webviews (`disallowed_useragent`), so the webview
can't reliably sign in either. Instead, the TV-activation dance:

1. Open the app's settings in the Pebble phone app → PebbleKit JS opens
   `https://…/link#<device-token>` in the webview.
2. The page registers the token and displays a short code — **BFK-392** —
   plus a QR of the pairing URL.
3. In any real browser: sign in with Google, enter the code (or scan the QR).
   The server binds device token ↔ account.
4. The webview polls, flips to ✓, and closes back into the phone app
   (`pebblejs://close`), which immediately runs a first sync.

### 8.2 Device API (Bearer device-token)

| Endpoint | Purpose |
|---|---|
| `GET /api/device/workouts` | All slots, packed per §4.2, base64-in-JSON, with format version + per-slot hash so JS can skip unchanged slots |
| `POST /api/device/sessions` | Packed session blobs (§4.3); server unpacks, stores, ACKs |
| `GET /api/device/status` | Linked? Account email, slot hashes, server time |
| `POST /api/device/recordings` *(M2)* | Raw labelled accelerometer capture of a set — the tuning + classification corpus (§6) |

Packed payloads are base64-wrapped in JSON — binary XHR in the PebbleKit JS
runtime is historically unreliable, and payloads are ~100 B each anyway.

### 8.3 Web UI

- **Workouts:** all your workouts with watch-slot badges (5 slots); editor with name, ordered exercise
  rows (movement picker grouped by body area, per-set target reps *or* hold
  seconds, per-set rest, optional weight), duplicate/delete/reorder. Validates
  the §4.2 pack-size budget at save time so the watch never sees an oversized
  workout.
- **History:** analytics charts (§8.6) above session cards (date, workout,
  duration, volume, per-set table incl. auto vs corrected reps), CSV export.
- **Explore:** public workouts (§8.5) with title, description, owner, and
  exercise summary; one-click fork into your own slots.
- **Devices:** linked Pebbles with last-sync time; revoke button.

### 8.4 Database (Diesel + SQLite)

```
users             id · google_sub · email · display_name · created_at
devices           id · user_id → users · token_hash · label · last_sync_at
pending_links     code · token_hash · expires_at
exercises         id · watch_movement_id (u8) · name · body_area ·
                  primary_muscles · secondary_muscles ·
                  profile_axis · profile_min_rep_ms · profile_smoothing ·
                  is_builtin
workouts          id · owner_id → users · title · description · is_public ·
                  forked_from → workouts? · created_at · updated_at
workout_exercises id · workout_id → workouts · position · exercise_id →
                  exercises · weight_kg · is_timed · is_amrap
workout_sets      id · workout_exercise_id · position · target · rest_secs
user_slots        user_id → users · slot (1–5) · workout_id → workouts
sessions          id · user_id → users · device_id → devices · workout_id? ·
                  workout_title · started_at · duration_secs
session_sets      id · session_id → sessions · position · exercise_id? ·
                  exercise_name · set_idx · target · actual · auto ·
                  weight_kg · start_offset_s · duration_s
recordings (M2)   id · user_id → users · device_id → devices · exercise_id? ·
                  recorded_at · sample_rate_hz · samples BLOB (x,y,z i16
                  triplets) · labeled_reps · is_timed · notes
```

Notes: device tokens are stored **hashed** (they're bearer credentials);
sessions snapshot the workout title and exercise names so history survives
edits and deletions; `user_slots` maps a user's workouts onto the watch's 8
persist slots; the `exercises` table carries muscle groups and the
accelerometer counting profile as data, seeded from the shared JSON source
(§4.1) — which leaves room for user-defined exercises and per-user tuned
profiles later without schema surgery.

### 8.5 Sharing

Workouts are private by default. Setting the public flag lists a workout on
**Explore**; "Add to my workouts" **forks a copy** into a free slot (recorded
via `forked_from`) — a fork never changes because its origin was edited or
deleted. The watch's 24-byte name is derived from the title; description is
website-only.

### 8.6 Analytics

Server-computed aggregates rendered with uPlot on the History page:

- **Volume over time** — Σ actual reps × weight per session and per week for
  weighted exercises; bodyweight work charted as rep totals alongside.
- **Reps & sets over time** — weekly totals, filterable by exercise.
- **Intensity over time** — work ÷ (work + rest) per session, where work =
  per-set duration (hold seconds for timed sets) and actual rest comes from
  the per-set start offsets (§4.3), so skipped rest raises intensity.
- **Per-exercise progression** — best set (weight × reps, or longest hold)
  trend.

## 9. Constraints & non-functional

- **Memory:** 64 KB heap (basalt/chalk/diorite), 128 KB (emery). Static
  buffers for the accel pipeline (< 1 KB). No dynamic allocation during a set.
- **Battery:** accelerometer only during active sets; no backlight forcing;
  target ≪ 10% drain for a 60-minute session.
- **Persist:** hard 4 KB / 256 B-per-key budget per §4.4 — enforced by the
  website's save-time validation, not discovered at runtime.
- **Robustness:** watch never depends on the phone mid-session; app relaunch
  after a crash offers to resume the in-progress session (state checkpointed to
  persist at each set boundary).

## 10. Milestones

- **M0 — Environment & scaffold.** SDK installed, project builds, runs in
  basalt emulator, git repo. *(done)*
- **M1 — Execution flow, manual counting.** Embedded workouts (from the dev
  server, incl. timed holds); full Home → Preview → Active → Rest → Summary
  flow; rep entry via buttons; rest/hold timers + vibes; pause menu. *(done —
  C unpacker for §4.2 shipped with it)*
- **M2 — Auto rep counting.** Accel pipeline + recording harness with upload
  to the server as a labelled corpus; capture real sets on the physical
  Pebble; tune profiles offline against the recordings; ship counter with
  vibe feedback and correction. *(shipped — counter live on-watch, every set
  auto-uploads, tools/replay.py mirrors the pipeline; profile tuning against
  real wrist data ongoing)*
- **M3 — Server & sync.** `server/` axum crate: Diesel/SQLite schema +
  migrations, Google sign-in, workout builder, pairing-code device linking,
  Rust binary packer + device API; PebbleKit JS relay; watch persist store;
  multi-workout support.
- **M4 — History & polish.** Session upload, website history + CSV, watch
  history, settings, chalk/diorite layout passes, resume-after-crash.
- **M5 — Sharing & analytics.** Public flag + Explore page + forking;
  aggregate queries and the §8.6 charts.
