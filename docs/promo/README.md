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
