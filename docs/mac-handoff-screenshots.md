# Continuation prompt — per-platform emulator screenshots (run on the Mac)

Paste the block below into Claude Code on the Mac (after `git pull`). The Jetson
can't run the Pebble emulator — `pypkjs` needs `stpyv8` (V8), which has no
installable wheel for aarch64 + Python 3.13 and won't build. The Mac runs it
fine, so per-platform submission screenshots are a Mac job.

---

I'm continuing work on **pebble-strength** (this repo). The app is live and
multi-user at **pebblestrength.app**; watch↔server sync works both directions.
For the appstore submission we already have, in `docs/promo/`:

- `strength-demo.gif` + `01-home.png … 05-summary.png` — the **Pebble Time
  (basalt)** flow, captured from a physical watch.
- `banner-720x320.png`, `icon-144.png`, `icon-48.png` — store banner + icons
  (the pixel "lifter" mascot).

**Task: capture emulator screenshots for the other Pebble platforms** so the
submission has a set per device. Target platforms (from `package.json`
`targetPlatforms`, plus aplite for completeness):

- **chalk** — Pebble Time Round, 180×180, colour, round
- **diorite** — Pebble 2, 144×168, black & white
- **emery** — Pebble Time 2, 200×228, colour
- **aplite** — Pebble Classic/Steel, 144×168, black & white (optional)

**Setup (if not already):** `pebble sdk install latest`; on Apple Silicon you
may need Rosetta 2 + `brew install libpng node`. Verify with `pebble build` at
the repo root (produces `build/pebble-strength.pbw`).

**For each platform:**
1. `pebble install --emulator <platform>` — launches the emulator (it shows the
   **embedded** workouts: Day A–D + Push ups; the emulator has no device token,
   so live sync is expected to be absent — that's fine for screenshots).
2. Drive the app with the emulator window's keys (Up/Down arrows, Right/Return =
   Select, Left = Back) to reach each screen, capturing with
   `pebble screenshot --emulator <platform> docs/promo/emu/<platform>-<screen>.png`:
   - **home** — the workout list
   - **preview** — select Day A (the exercise list)
   - **active** — start it; the big work timer + reps (let it run a few seconds)
   - **rest** — press Select to finish a set → the rest countdown
   - **summary** — Back → End workout → the "Done!" screen
3. Optional: build a per-platform GIF the same way as `strength-demo.gif` (see
   the Pillow build in the git history: nearest-neighbour upscale + bezel).

Match the five-screen story of the existing basalt set. Note chalk is **round**
(180×180) so its framing differs — capture and eyeball it. Save everything under
`docs/promo/emu/` and commit.
