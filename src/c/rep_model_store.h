#pragma once
#include <pebble.h>
#include "rep_model.h"

// Downloaded learned-counter model, persist-backed (survives restarts). Synced
// from the phone via {RM_DATA} — one packed record. Absent any model, the
// session UI falls back to the fixed-axis counter, so v2 is purely additive.

void rep_model_store_init(void);
const RepModel *rep_model_current(void);

// Sync (phone -> watch): parse + persist one packed model record. Returns true
// if applied. Wire (LE): [feature_version u8][model_version u16][nfeat u8]
// [nfeat x weight_q16 i32][bias_q16 i32][movement_count u8][movements...].
bool rep_model_sync_set(const uint8_t *blob, uint16_t len);
