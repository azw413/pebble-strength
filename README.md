# Strength

A Garmin-style strength training app for Pebble: program workouts on your
phone, run them from your wrist with accelerometer-based rep counting, rest
timers, and session history.

**See [SPEC.md](SPEC.md) for the full specification** — features, data model,
sync protocol, rep-counting algorithm, and milestones.

Targets: `basalt` (Time), `chalk` (Time Round), `diorite` (Pebble 2),
`emery` (Time 2 / Core Time 2).

## Development

Toolchain: [`pebble-tool`](https://developer.repebble.com/sdk/) (installed via
`uv tool install pebble-tool`) with SDK 4.17.

Until real sync lands (M3), the watch binary embeds the dev server's workouts.
The loop after editing workouts at http://localhost:8080:

```sh
python3 tools/gen_embedded.py         # refresh src/c/embedded_workouts.h from the server
python3 tools/gen_movements.py        # only after editing shared/exercises.json
pebble build && pebble install --emulator basalt
```

### Recording & rep-counter tuning (M2)

Every set run on the watch records raw 25 Hz accelerometer data and uploads it
(via the phone JS) to the server, labelled with the corrected rep count —
browse and export at http://localhost:8080/recordings. For a physical watch,
set `SERVER` in `src/pkjs/index.js` to this machine's LAN address first.

```sh
python3 tools/replay.py --selftest          # synthetic-waveform pipeline check
python3 tools/replay.py recording_7.csv     # replay a capture offline (tune constants here
                                            # and mirror changes into src/c/rep_counter.c)
pebble emu-accel custom reps.txt --emulator basalt   # inject "x,y,z" mG lines into the emulator
```

```sh
pebble build                          # build all target platforms → build/strength.pbw
pebble install --emulator basalt      # run in the emulator (also: chalk/diorite/emery)
pebble logs --emulator basalt         # tail app logs
pebble screenshot --emulator basalt   # capture the screen
pebble kill                           # stop stuck emulators (fixes "Connection refused")
pebble emu-app-config --emulator basalt   # open the config page against the emulator
```

### Server (website + device API)

```sh
cd server
cp .env.example .env        # then fill in Google OAuth creds, or leave DEV_LOGIN=1
cargo run                   # http://localhost:8080, DB + migrations + seed automatic
cargo test                  # packed-format tests
```

With `DEV_LOGIN=1` the landing page has a local-only dev login — no Google
setup needed for development — and the dev account is seeded with the four
"Rings & Strength" sample workouts (Days A–D) in watch slots 1–4. For real Google sign-in, create an OAuth client
(see `.env.example`) and set `GOOGLE_CLIENT_ID` / `GOOGLE_CLIENT_SECRET`.

### Installing on a physical watch

1. Install the Pebble mobile app from <https://repebble.com/app> and pair the watch.
2. Enable **Dev Connect** in the app's settings and sign in with GitHub.
3. Then:

```sh
pebble login
pebble install --cloudpebble
```

## Project layout

```
src/c/           Watchapp: UI, workout engine, rep counter, persist store
src/pkjs/        PebbleKit JS (phone side): sync relay to the server
server/          Rust axum app: Google sign-in, workout builder, device API,
                 binary packer (arrives in M3 — see SPEC.md §8)
resources/       Images, fonts
package.json     Project metadata (UUID, platforms, message keys)
wscript          Build rules — usually no need to edit
SPEC.md          Full app specification
```

## Documentation

SDK docs and API reference: <https://developer.repebble.com>
