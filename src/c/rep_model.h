#pragma once
#include <stdint.h>
#include <stdbool.h>

// Learned rep counter ("v2", docs/design/rep-counter-learned.md): rotation-
// invariant features of the whole set -> a tiny linear model -> the count.
// Runs once at set end on the captured accel buffer. Orientation-independent by
// construction, so it works on any wrist/flip/posture. Pure integer (no float),
// a bit-faithful mirror of tools/rep_features.py (feature_version 1).

#define REP_FEAT_VERSION 1
#define REP_NFEAT 3  // dur (samples), magstd (mG), pk (count)

// A downloaded model: linear weights (Q16 fixed point) + bias, and the set of
// movements it was trained/validated for.
typedef struct {
  bool present;
  uint8_t feature_version;
  uint16_t model_version;
  int32_t weight_q16[REP_NFEAT];
  int32_t bias_q16;
  uint8_t movements[24];  // movement ids this model applies to
  uint8_t movement_count;
} RepModel;

// Compute the rotation-invariant features from an interleaved x,y,z sample
// buffer (mG). Returns true and fills feat[REP_NFEAT] on success; false if the
// set is too short / no active window.
bool rep_features(const int16_t *xyz, uint16_t n, uint16_t rate, int32_t feat[REP_NFEAT]);

// Predict the rep count: round(sum(w*feat) + bias). Clamped to >= 0.
int rep_model_predict(const RepModel *m, const int32_t feat[REP_NFEAT]);

// True if the model applies to this movement.
bool rep_model_has(const RepModel *m, uint8_t movement_id);
