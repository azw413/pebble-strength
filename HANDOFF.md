# Continuation prompt

Paste the block below into Claude Code after cloning this repo on a new machine.

---

I'm continuing work on **pebble-strength** (this repo) — a Garmin-style strength
training app for Pebble watches. Orient yourself first: read `SPEC.md` (full
specification, data formats, milestone status) and `README.md` (dev commands).

**Current state:** M0–M2 are done and verified in the emulator.
- Watch app (C, Pebble SDK 4.17): guided workout execution (per-set rep targets,
  timed holds), accelerometer rep counting, and raw 25 Hz set recording.
- Server (`server/`, Rust axum + Diesel/SQLite): workout builder UI, packed
  binary device API, and a labelled accel-recording store with CSV export.
  Runs with `DEV_LOGIN=1` (dev-mode login, tokenless device API, seeds four
  sample workouts). No real auth for now — that's intentional.
- PebbleKit JS (`src/pkjs/index.js`) relays set recordings to the server.
- The watch binary embeds workouts via `tools/gen_embedded.py` (real AppMessage
  sync is milestone M3, not built yet).

**Set up this machine first** (check what's already installed):
- Rust toolchain; then `cd server && DEV_LOGIN=1 cargo run` (DB migrates and
  seeds itself; UI at http://localhost:8080 via the "Dev login" button).
- Pebble SDK: `uv tool install --python 3.13 pebble-tool`, then
  `pebble sdk install latest`. macOS also needs `brew install libpng node`
  and Rosetta 2 on Apple Silicon. Verify with `pebble build` at the repo root.

**Task 1 — run the server on a fixed IP and port.**
`server/src/main.rs` currently binds `127.0.0.1:{PORT}` — make the bind address
configurable (e.g. `BIND_ADDR` env var, default `127.0.0.1`) and run it on
`0.0.0.0` with a fixed port so the phone can reach it over the LAN. Keep
`DEV_LOGIN=1`. Then point the phone relay at it: set `SERVER` in
`src/pkjs/index.js` to `http://<fixed-ip>:<port>` and rebuild the watch app.
Verify from another device: `curl http://<fixed-ip>:<port>/api/device/workouts`
should return the four sample workouts.

**Task 2 — install on my physical Pebble Time (basalt).**
Pebble mobile app from https://repebble.com/app, pair the watch, enable
**Dev Connect** in the app settings (sign in with GitHub), then
`pebble login` and `pebble install --cloudpebble`. Then I'll do a real set on
the watch: confirm the recording appears at `http://<fixed-ip>:<port>/recordings`,
download its CSV, and run `python3 tools/replay.py <csv>` to compare the
counter against what I actually did. That real-wrist data drives tuning of
`src/c/rep_counter.c` — its constants are mirrored in `tools/replay.py`
(tune in Python first; keep both in sync).

**Known gotchas:**
- Emulator's first boot sometimes races → `Connection refused`; run
  `pebble kill` and retry.
- After editing `messageKeys` in `package.json`, run `pebble clean` before
  building or the generated keys go stale.
- `tools/gen_embedded.py` needs the dev server running (it fetches the packed
  workouts from it). `tools/gen_movements.py` regenerates the movement table
  after editing `shared/exercises.json`.
- PebbleKit JS delivers AppMessage byte arrays as typed arrays — already
  handled in `index.js`, don't "simplify" it back to `concat`.

**After that:** tune the rep counter from the real recordings, then M3
(real workout sync over AppMessage, replacing the embedded workouts — the C
unpacker `src/c/packfmt.c` and the server device API are both ready for it).
