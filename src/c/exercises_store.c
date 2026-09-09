#include "exercises_store.h"
#include "movements.h"
#include <string.h>

// Persist: the packed catalog split into <=256-byte chunks (persist value cap).
#define PERSIST_EX_LEN 60
#define PERSIST_EX_BASE 61   // chunk i at PERSIST_EX_BASE + i
#define EX_CHUNK 256
#define EX_MAX_CHUNKS ((EX_MAX_BYTES + EX_CHUNK - 1) / EX_CHUNK)

static uint8_t s_blob[EX_MAX_BYTES];
static uint16_t s_len;

// Sync staging.
static uint8_t s_stage[EX_MAX_BYTES];
static uint16_t s_stage_len;
static uint8_t s_stage_total, s_stage_seen;

static void persist_catalog(void) {
  uint16_t off = 0;
  uint8_t chunks = 0;
  while (off < s_len) {
    uint16_t n = (s_len - off) > EX_CHUNK ? EX_CHUNK : (s_len - off);
    persist_write_data(PERSIST_EX_BASE + chunks, s_blob + off, n);
    off += n;
    chunks++;
  }
  persist_write_int(PERSIST_EX_LEN, s_len);
}

void exercises_init(void) {
  s_len = 0;
  if (!persist_exists(PERSIST_EX_LEN)) return;
  int len = persist_read_int(PERSIST_EX_LEN);
  if (len <= 0 || len > EX_MAX_BYTES) return;
  uint16_t off = 0;
  uint8_t chunk = 0;
  while (off < (uint16_t)len && chunk < EX_MAX_CHUNKS) {
    int key = PERSIST_EX_BASE + chunk;
    if (!persist_exists(key)) return;  // corrupt/partial -> ignore, use built-ins
    int got = persist_read_data(key, s_blob + off, EX_CHUNK);
    if (got <= 0) return;
    off += got;
    chunk++;
  }
  s_len = (uint16_t)len;
}

const char *movement_name(uint8_t movement_id) {
  // Downloaded catalog: scan the packed [id][len][name] records.
  static char buf[25];
  uint16_t i = 0;
  while (i + 2 <= s_len) {
    uint8_t id = s_blob[i];
    uint8_t nlen = s_blob[i + 1];
    uint16_t rec_end = i + 2 + nlen;
    if (rec_end > s_len) break;  // malformed
    if (id == movement_id) {
      uint8_t n = nlen < sizeof(buf) - 1 ? nlen : sizeof(buf) - 1;
      memcpy(buf, s_blob + i + 2, n);
      buf[n] = '\0';
      return buf;
    }
    i = rec_end;
  }
  // Fall back to the compiled table.
  return movement_name_builtin(movement_id);
}

void exercises_sync_begin(uint8_t total) {
  s_stage_len = 0;
  s_stage_total = total;
  s_stage_seen = 0;
}

void exercises_sync_set(uint8_t index, const uint8_t *data, uint16_t len) {
  (void)index;
  if (s_stage_len + len > EX_MAX_BYTES) return;  // overflow guard
  memcpy(s_stage + s_stage_len, data, len);
  s_stage_len += len;
  s_stage_seen++;
}

bool exercises_sync_commit(void) {
  if (s_stage_total == 0 || s_stage_seen != s_stage_total) return false;  // partial
  memcpy(s_blob, s_stage, s_stage_len);
  s_len = s_stage_len;
  persist_catalog();
  return true;
}
