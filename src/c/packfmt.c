#include "packfmt.h"

bool packfmt_unpack(const uint8_t *data, size_t len, PackedWorkout *out) {
  if (len < 28) {
    return false;
  }
  if (data[24] != PACK_VERSION) {
    APP_LOG(APP_LOG_LEVEL_ERROR, "workout format version %d, expected %d", data[24], PACK_VERSION);
    return false;
  }
  uint8_t count = data[25];
  if (count == 0 || count > PACK_MAX_EXERCISES) {
    return false;
  }
  memcpy(out->name, data, 24);
  out->name[24] = '\0';
  out->exercise_count = count;

  size_t off = 28;
  for (uint8_t i = 0; i < count; i++) {
    if (off + 6 > len) {
      return false;
    }
    PackExercise *e = &out->exercises[i];
    e->movement_id = data[off];
    e->flags = data[off + 1];
    e->weight_q = (uint16_t)data[off + 2] | ((uint16_t)data[off + 3] << 8);
    e->set_count = data[off + 4];
    // data[off + 5] is customNameIdx — unused until custom names land
    if (e->set_count == 0 || e->set_count > PACK_MAX_SETS) {
      return false;
    }
    off += 6;
    if (off + 2u * e->set_count > len) {
      return false;
    }
    for (uint8_t s = 0; s < e->set_count; s++) {
      e->sets[s].target = data[off];
      e->sets[s].rest_5s = data[off + 1];
      off += 2;
    }
  }
  return true;
}
