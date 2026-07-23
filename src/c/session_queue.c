#include "session_queue.h"
#include <string.h>

#define PERSIST_SQ_LEN 40
#define PERSIST_SQ_NEXTID 41
#define PERSIST_SQ_BASE 50  // per-slot records: 50 .. 50+SQ_MAX-1

#define SQ_REC_BYTES 38  // packed on-persist record size

static SqSet s_q[SQ_MAX];
static uint8_t s_len;
static uint32_t s_next_id;

static void wr16(uint8_t *p, uint16_t v) {
  p[0] = v & 0xff;
  p[1] = (v >> 8) & 0xff;
}
static void wr32(uint8_t *p, uint32_t v) {
  p[0] = v & 0xff;
  p[1] = (v >> 8) & 0xff;
  p[2] = (v >> 16) & 0xff;
  p[3] = (v >> 24) & 0xff;
}
static uint16_t rd16(const uint8_t *p) {
  return (uint16_t)p[0] | ((uint16_t)p[1] << 8);
}
static uint32_t rd32(const uint8_t *p) {
  return (uint32_t)p[0] | ((uint32_t)p[1] << 8) | ((uint32_t)p[2] << 16) | ((uint32_t)p[3] << 24);
}

// Packed record: [0..3]id [4..7]at [8]mv [9]set [10]timed [11]actual
//                [12..13]work [14..37]name(24)
static void pack(const SqSet *s, uint8_t *r) {
  wr32(r, s->client_set_id);
  wr32(r + 4, s->performed_at);
  r[8] = s->movement_id;
  r[9] = s->set_index;
  r[10] = s->timed ? 1 : 0;
  r[11] = s->actual;
  wr16(r + 12, s->work_secs);
  memcpy(r + 14, s->workout_name, 24);
}
static void unpack(const uint8_t *r, SqSet *s) {
  s->client_set_id = rd32(r);
  s->performed_at = rd32(r + 4);
  s->movement_id = r[8];
  s->set_index = r[9];
  s->timed = r[10] != 0;
  s->actual = r[11];
  s->work_secs = rd16(r + 12);
  memcpy(s->workout_name, r + 14, 24);
  s->workout_name[24] = '\0';
}

static void save(void) {
  uint8_t rec[SQ_REC_BYTES];
  for (uint8_t i = 0; i < s_len; i++) {
    pack(&s_q[i], rec);
    persist_write_data(PERSIST_SQ_BASE + i, rec, SQ_REC_BYTES);
  }
  for (uint8_t i = s_len; i < SQ_MAX; i++) {
    if (persist_exists(PERSIST_SQ_BASE + i)) persist_delete(PERSIST_SQ_BASE + i);
  }
  persist_write_int(PERSIST_SQ_LEN, s_len);
}

void session_queue_init(void) {
  s_len = 0;
  s_next_id = persist_exists(PERSIST_SQ_NEXTID) ? (uint32_t)persist_read_int(PERSIST_SQ_NEXTID) : 1;
  int len = persist_exists(PERSIST_SQ_LEN) ? persist_read_int(PERSIST_SQ_LEN) : 0;
  if (len < 0) len = 0;
  if (len > SQ_MAX) len = SQ_MAX;
  uint8_t rec[SQ_REC_BYTES];
  for (int i = 0; i < len; i++) {
    int key = PERSIST_SQ_BASE + i;
    if (persist_exists(key) && persist_get_size(key) == SQ_REC_BYTES) {
      persist_read_data(key, rec, SQ_REC_BYTES);
      unpack(rec, &s_q[s_len++]);
    }
  }
}

uint32_t session_queue_next_id(void) {
  uint32_t id = s_next_id++;
  persist_write_int(PERSIST_SQ_NEXTID, (int)s_next_id);
  return id;
}

void session_queue_enqueue(const SqSet *set) {
  if (s_len >= SQ_MAX) {
    // Full: drop the oldest so the most recent sets are the ones preserved.
    memmove(&s_q[0], &s_q[1], (SQ_MAX - 1) * sizeof(SqSet));
    s_len = SQ_MAX - 1;
  }
  s_q[s_len++] = *set;
  save();
}

void session_queue_ack(uint32_t client_set_id) {
  for (uint8_t i = 0; i < s_len; i++) {
    if (s_q[i].client_set_id == client_set_id) {
      memmove(&s_q[i], &s_q[i + 1], (s_len - i - 1) * sizeof(SqSet));
      s_len--;
      save();
      return;
    }
  }
}

uint8_t session_queue_count(void) { return s_len; }

bool session_queue_oldest(SqSet *out) {
  if (s_len == 0) return false;
  *out = s_q[0];
  return true;
}
