#pragma once
#include <pebble.h>

// Packed workout format — the binary contract with the server (SPEC.md §4.2).
// Mirrors server/src/pack.rs exactly. Little-endian.

#define PACK_VERSION 1
#define PACK_MAX_EXERCISES 16
#define PACK_MAX_SETS 10
#define PACK_FLAG_TIMED (1 << 0)
#define PACK_FLAG_AMRAP (1 << 1)

typedef struct {
  uint8_t target;   // reps, or hold seconds when the exercise is timed
  uint8_t rest_5s;  // rest in 5-second units
} PackSet;

typedef struct {
  uint8_t movement_id;
  uint8_t flags;
  uint16_t weight_q;  // 0.25 kg units, 0 = bodyweight
  uint8_t set_count;
  PackSet sets[PACK_MAX_SETS];
} PackExercise;

typedef struct {
  char name[25];  // 24 bytes + NUL
  uint8_t exercise_count;
  PackExercise exercises[PACK_MAX_EXERCISES];
} PackedWorkout;

bool packfmt_unpack(const uint8_t *data, size_t len, PackedWorkout *out);

static inline uint16_t packfmt_rest_secs(PackSet s) {
  return (uint16_t)s.rest_5s * 5;
}
