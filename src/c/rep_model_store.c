#include "rep_model_store.h"
#include <string.h>

#define PERSIST_RM_BLOB 70
#define RM_MAX_BYTES 96

static RepModel s_model;
static uint8_t s_blob[RM_MAX_BYTES];

static uint16_t rd16(const uint8_t *p) { return (uint16_t)p[0] | ((uint16_t)p[1] << 8); }
static int32_t rd32(const uint8_t *p) {
  return (int32_t)((uint32_t)p[0] | ((uint32_t)p[1] << 8) | ((uint32_t)p[2] << 16) | ((uint32_t)p[3] << 24));
}

static bool parse(const uint8_t *b, uint16_t len) {
  if (len < 4) return false;
  uint8_t fver = b[0];
  uint16_t mver = rd16(b + 1);
  uint8_t nfeat = b[3];
  if (fver != REP_FEAT_VERSION || nfeat != REP_NFEAT) return false;  // unsupported
  uint16_t need = 4 + (uint16_t)nfeat * 4 + 4 + 1;
  if (len < need) return false;
  const uint8_t *p = b + 4;
  RepModel m;
  memset(&m, 0, sizeof m);
  m.feature_version = fver;
  m.model_version = mver;
  for (int i = 0; i < nfeat; i++) { m.weight_q16[i] = rd32(p); p += 4; }
  m.bias_q16 = rd32(p); p += 4;
  uint8_t mc = *p++;
  if (need + mc > len) return false;
  if (mc > sizeof m.movements) mc = sizeof m.movements;
  memcpy(m.movements, p, mc);
  m.movement_count = mc;
  m.present = true;
  s_model = m;
  return true;
}

void rep_model_store_init(void) {
  s_model.present = false;
  if (persist_exists(PERSIST_RM_BLOB)) {
    int size = persist_get_size(PERSIST_RM_BLOB);
    if (size > 0 && size <= RM_MAX_BYTES) {
      persist_read_data(PERSIST_RM_BLOB, s_blob, size);
      parse(s_blob, (uint16_t)size);
    }
  }
}

const RepModel *rep_model_current(void) { return &s_model; }

bool rep_model_sync_set(const uint8_t *blob, uint16_t len) {
  if (len == 0 || len > RM_MAX_BYTES) return false;
  if (!parse(blob, len)) return false;
  memcpy(s_blob, blob, len);
  persist_write_data(PERSIST_RM_BLOB, s_blob, len);
  return true;
}
