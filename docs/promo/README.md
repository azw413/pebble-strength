# Promo / app-submission assets

Authentic screen captures from a physical Pebble Time (basalt, 144×168),
grabbed with `pebble screenshot --cloudpebble`.

- `strength-demo.gif` — animated demo (4× nearest-neighbour upscale + bezel),
  looping the workout flow: Home → Preview → Active timer → Rest → Done.
  Marketing/promo asset.
- `01-home.png` … `05-summary.png` — the same frames at **native 144×168**,
  for the appstore's screenshot slots.

Regenerate the GIF from the native PNGs with Pillow (see the build step in the
project history), or recapture with `pebble screenshot --cloudpebble` while the
watch is on each screen.

## Store assets (generated from the pixel "lifter" mascot)

- `banner-720x320.png` — appstore banner (lifter + STRENGTH wordmark + tagline).
- `icon-144.png`, `icon-48.png` — app icons.

Regenerate with Pillow + `PressStart2P.ttf` (see the build in git history).

## Per-platform screenshots

- **basalt** (Pebble Time, 144×168): `01-home.png` … `05-summary.png` above,
  from a physical watch.
- **chalk** (Pebble Time Round, 180×180), **diorite** (Pebble 2, 144×168 B&W),
  **emery** (Pebble Time 2, 200×228): `emu/<platform>-01-home.png` …
  `<platform>-05-summary.png`, captured from the SDK emulator (same five-screen
  story, driven via the QEMU monitor).
- **aplite** is not captured: the app doesn't target it — the 2400-sample
  recording buffer overflows aplite's 24 KB app RAM (linker: region `APP`
  overflowed by ~12.7 KB).
