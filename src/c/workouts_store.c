#include "workouts_store.h"
#include "embedded_workouts.h"

// Persistent-storage keys. WK_COUNT holds how many synced workouts exist (0 or
// absent => fall back to embedded); WK_BASE+i holds each workout's packed bytes.
#define PERSIST_WK_COUNT 1
#define PERSIST_WK_BASE 10

typedef struct {
  uint16_t len;
  uint8_t data[WK_MAX_BYTES];
} Stored;

static Stored s_store[WK_MAX];
static uint8_t s_count;

static Stored s_pending[WK_MAX];
static uint8_t s_pending_total;
static uint8_t s_pending_got;
static bool s_pending_active;

static void load_embedded(void) {
  s_count = EMBEDDED_WORKOUT_COUNT < WK_MAX ? EMBEDDED_WORKOUT_COUNT : WK_MAX;
  for (uint8_t i = 0; i < s_count; i++) {
    uint16_t len = EMBEDDED_WORKOUTS[i].len;
    if (len > WK_MAX_BYTES) len = WK_MAX_BYTES;
    memcpy(s_store[i].data, EMBEDDED_WORKOUTS[i].data, len);
    s_store[i].len = len;
  }
}

void workouts_init(void) {
  int count = persist_exists(PERSIST_WK_COUNT) ? persist_read_int(PERSIST_WK_COUNT) : 0;
  if (count > 0 && count <= WK_MAX) {
    uint8_t ok = 0;
    for (int i = 0; i < count; i++) {
      int key = PERSIST_WK_BASE + i;
      int size = persist_exists(key) ? persist_get_size(key) : 0;
      if (size <= 0 || size > WK_MAX_BYTES) {
        break;
      }
      persist_read_data(key, s_store[i].data, size);
      s_store[i].len = (uint16_t)size;
      ok++;
    }
    if (ok == count) {
      s_count = count;
      return;
    }
  }
  load_embedded();
}

uint8_t workouts_count(void) {
  return s_count;
}

const uint8_t *workouts_get(uint8_t i, uint16_t *len_out) {
  if (i >= s_count) {
    if (len_out) *len_out = 0;
    return NULL;
  }
  if (len_out) *len_out = s_store[i].len;
  return s_store[i].data;
}

void workouts_sync_begin(uint8_t total) {
  s_pending_total = total > WK_MAX ? WK_MAX : total;
  s_pending_got = 0;
  s_pending_active = true;
}

void workouts_sync_set(uint8_t i, const uint8_t *data, uint16_t len) {
  if (!s_pending_active || i >= WK_MAX || len == 0 || len > WK_MAX_BYTES) {
    return;
  }
  memcpy(s_pending[i].data, data, len);
  s_pending[i].len = len;
  s_pending_got++;
}

bool workouts_sync_commit(void) {
  bool ok = s_pending_active && s_pending_total > 0 && s_pending_got >= s_pending_total;
  if (ok) {
    for (uint8_t i = 0; i < s_pending_total; i++) {
      s_store[i] = s_pending[i];
      persist_write_data(PERSIST_WK_BASE + i, s_store[i].data, s_store[i].len);
    }
    s_count = s_pending_total;
    persist_write_int(PERSIST_WK_COUNT, s_count);
    APP_LOG(APP_LOG_LEVEL_INFO, "workouts synced: %d", s_count);
  }
  s_pending_active = false;
  return ok;
}
