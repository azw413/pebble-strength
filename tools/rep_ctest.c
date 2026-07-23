// Off-device harness for the rep-counter engine. Reads "x y z" (mG) per line
// from stdin, runs src/c/rep_counter.c with the config from argv, prints the
// final rep count. Used by tools/verify_rep_counter.py to prove the C engine
// matches tools/rep_causal.py before it ships to the watch.
//
//   cc -O2 -o rep_ctest tools/rep_ctest.c src/c/rep_counter.c -Isrc/c
//   args: axis_mode lp_ms hp_ms thr_pct min_rep_ms min_amp warmup_ms
#include <stdio.h>
#include <stdlib.h>
#include "rep_counter.h"

int main(int argc, char **argv) {
  CounterConfig cfg = {0};
  cfg.kind = 0;
  cfg.axis_mode = argc > 1 ? (uint8_t)atoi(argv[1]) : 3;
  cfg.lp_ms = argc > 2 ? (uint16_t)atoi(argv[2]) : 500;
  cfg.hp_ms = argc > 3 ? (uint16_t)atoi(argv[3]) : 3000;
  cfg.thr_pct = argc > 4 ? (uint8_t)atoi(argv[4]) : 30;
  cfg.min_rep_ms = argc > 5 ? (uint16_t)atoi(argv[5]) : 900;
  cfg.min_amp = argc > 6 ? (uint16_t)atoi(argv[6]) : 70;
  cfg.warmup_ms = argc > 7 ? (uint16_t)atoi(argv[7]) : 700;

  RepCounter rc;
  rep_counter_init(&rc, &cfg, 25);

  int x, y, z;
  uint32_t t = 0;
  char line[128];
  while (fgets(line, sizeof line, stdin)) {
    if (sscanf(line, "%d %d %d", &x, &y, &z) == 3) {
      rep_counter_feed(&rc, (int16_t)x, (int16_t)y, (int16_t)z, t);
      t += 40;
    }
  }
  printf("%u\n", rc.count);
  return 0;
}
