// Off-device harness for the learned-counter feature extractor. Reads "x y z"
// (mG) per line from stdin, runs src/c/rep_model.c, prints the 3 features so
// tools/verify_rep_model.py can confirm the C matches the Python reference.
//   cc -O2 -o rep_model_ctest tools/rep_model_ctest.c src/c/rep_model.c -Isrc/c
#include <stdio.h>
#include <stdlib.h>
#include "rep_model.h"

int main(void) {
  static int16_t xyz[1800 * 3];
  int n = 0, x, y, z;
  char line[128];
  while (n < 1800 && fgets(line, sizeof line, stdin)) {
    if (sscanf(line, "%d %d %d", &x, &y, &z) == 3) {
      xyz[n * 3] = (int16_t)x; xyz[n * 3 + 1] = (int16_t)y; xyz[n * 3 + 2] = (int16_t)z;
      n++;
    }
  }
  int32_t feat[REP_NFEAT];
  if (rep_features(xyz, (uint16_t)n, 25, feat))
    printf("%d %d %d\n", feat[0], feat[1], feat[2]);
  else
    printf("none\n");
  return 0;
}
