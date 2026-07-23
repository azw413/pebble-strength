#include "counters_store.h"
#include "counters.h"
#include <string.h>

#define PERSIST_CN_COUNT 30
#define PERSIST_CN_BLOB 31

static uint8_t s_count;
static uint8_t s_blob[CN_STORE_MAX * CN_RECORD_BYTES];

static uint16_t rd16(const uint8_t *p) {
  return (uint16_t)p[0] | ((uint16_t)p[1] << 8);
}

// Packed record layout (little-endian), must match pkjs packCounters():
// [0] movement_id [1] kind [2] axis_mode [3] thr_pct
// [4..5] lp_ms [6..7] hp_ms [8..9] min_rep_ms [10..11] min_amp [12..13] warmup_ms
static void parse_record(const uint8_t *r, CounterConfig *out) {
  out->kind = r[1];
  out->axis_mode = r[2];
  out->thr_pct = r[3];
  out->lp_ms = rd16(r + 4);
  out->hp_ms = rd16(r + 6);
  out->min_rep_ms = rd16(r + 8);
  out->min_amp = rd16(r + 10);
  out->warmup_ms = rd16(r + 12);
}

void counters_init(void) {
  s_count = 0;
  if (persist_exists(PERSIST_CN_COUNT) && persist_exists(PERSIST_CN_BLOB)) {
    int c = persist_read_int(PERSIST_CN_COUNT);
    int size = persist_get_size(PERSIST_CN_BLOB);
    if (c > 0 && c <= CN_STORE_MAX && size == c * CN_RECORD_BYTES) {
      persist_read_data(PERSIST_CN_BLOB, s_blob, size);
      s_count = (uint8_t)c;
    }
  }
}

void counter_config_get(uint8_t movement_id, CounterConfig *out) {
  for (uint8_t i = 0; i < s_count; i++) {
    const uint8_t *r = &s_blob[i * CN_RECORD_BYTES];
    if (r[0] == movement_id) {
      parse_record(r, out);
      return;
    }
  }
  *out = *counter_config_default(movement_id);
}

bool counters_sync_set(const uint8_t *blob, uint16_t len, uint8_t count) {
  if (count == 0) return false;
  uint8_t n = count > CN_STORE_MAX ? CN_STORE_MAX : count;  // clamp; keep the first N
  uint16_t need = (uint16_t)n * CN_RECORD_BYTES;
  if (len < need) return false;
  memcpy(s_blob, blob, need);
  s_count = n;
  persist_write_int(PERSIST_CN_COUNT, s_count);
  persist_write_data(PERSIST_CN_BLOB, s_blob, need);
  return true;
}
