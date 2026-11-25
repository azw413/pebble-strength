#include "hr.h"
#include "session_queue.h"
#include <string.h>

#define MSG_HR 4  // watch -> phone: a workout's HR trace (see strength.c MSG_TYPE)

// Persist layout: one meta key + the bpm bytes split across <=256-byte chunks.
#define PERSIST_HR_META 90
#define PERSIST_HR_BASE 91
#define HR_CHUNK 256
#define HR_NCHUNKS ((HR_MAX + HR_CHUNK - 1) / HR_CHUNK)

typedef struct {
  uint32_t client_id;
  uint32_t started_at;
  uint16_t count;
  char workout_name[25];
} HrMeta;

static uint8_t s_bpm[HR_MAX];
static HrMeta s_meta;
static bool s_capturing;
static bool s_pending;   // a trace is captured + persisted, awaiting server ack
static uint16_t s_count; // samples captured so far this workout
static uint8_t s_tick;   // 0..HR_INTERVAL_S-1

bool hr_available(void) {
  time_t now = time(NULL);
  HealthServiceAccessibilityMask m =
      health_service_metric_accessible(HealthMetricHeartRateBPM, now - 1, now);
  return (m & HealthServiceAccessibilityMaskAvailable) != 0;
}

static void persist_trace(void) {
  persist_write_data(PERSIST_HR_META, &s_meta, sizeof s_meta);
  uint16_t off = 0;
  uint8_t c = 0;
  while (off < s_meta.count && c < HR_NCHUNKS) {
    uint16_t n = (s_meta.count - off) > HR_CHUNK ? HR_CHUNK : (s_meta.count - off);
    persist_write_data(PERSIST_HR_BASE + c, s_bpm + off, n);
    off += n;
    c++;
  }
}

static void clear_trace(void) {
  s_pending = false;
  if (persist_exists(PERSIST_HR_META)) persist_delete(PERSIST_HR_META);
  for (int c = 0; c < HR_NCHUNKS; c++)
    if (persist_exists(PERSIST_HR_BASE + c)) persist_delete(PERSIST_HR_BASE + c);
}

void hr_init(void) {
  s_capturing = s_pending = false;
  if (!persist_exists(PERSIST_HR_META)) return;
  persist_read_data(PERSIST_HR_META, &s_meta, sizeof s_meta);
  if (s_meta.count == 0 || s_meta.count > HR_MAX) { clear_trace(); return; }
  uint16_t off = 0;
  uint8_t c = 0;
  while (off < s_meta.count && c < HR_NCHUNKS) {
    if (!persist_exists(PERSIST_HR_BASE + c)) { clear_trace(); return; }
    int got = persist_read_data(PERSIST_HR_BASE + c, s_bpm + off, HR_CHUNK);
    if (got <= 0) { clear_trace(); return; }
    off += got;
    c++;
  }
  s_pending = true;  // a trace from a previous run awaits flushing
}

void hr_begin(const char *workout_name) {
  if (!hr_available()) { s_capturing = false; return; }
  s_capturing = true;
  s_count = 0;
  s_tick = 0;
  strncpy(s_meta.workout_name, workout_name, sizeof s_meta.workout_name - 1);
  s_meta.workout_name[sizeof s_meta.workout_name - 1] = '\0';
  health_service_set_heart_rate_sample_period(HR_INTERVAL_S);
}

void hr_tick(void) {
  if (!s_capturing) return;
  if (++s_tick < HR_INTERVAL_S) return;
  s_tick = 0;
  if (s_count >= HR_MAX) return;  // buffer full -> stop appending (keep what we have)
  HealthValue v = health_service_peek_current_value(HealthMetricHeartRateBPM);
  s_bpm[s_count++] = (v > 0 && v < 255) ? (uint8_t)v : 0;  // 0 = no reading
}

void hr_end(void) {
  if (!s_capturing) return;
  s_capturing = false;
  health_service_set_heart_rate_sample_period(0);  // restore default (battery)
  if (s_count == 0) return;
  // Keep the trace only if it has at least one real reading.
  bool any = false;
  for (uint16_t i = 0; i < s_count; i++)
    if (s_bpm[i]) { any = true; break; }
  if (!any) return;
  s_meta.client_id = session_queue_next_id();
  s_meta.started_at = (uint32_t)time(NULL) - (uint32_t)s_count * HR_INTERVAL_S;
  s_meta.count = s_count;  // workout_name was set in hr_begin
  persist_trace();
  s_pending = true;
}

void hr_flush(void) {
  if (!s_pending) return;
  DictionaryIterator *iter;
  if (app_message_outbox_begin(&iter) != APP_MSG_OK) return;  // retry on next pull
  dict_write_uint8(iter, MESSAGE_KEY_MSG_TYPE, MSG_HR);
  dict_write_uint32(iter, MESSAGE_KEY_CLIENT_ID, s_meta.client_id);
  dict_write_uint32(iter, MESSAGE_KEY_HR_START, s_meta.started_at);
  dict_write_uint8(iter, MESSAGE_KEY_HR_INTERVAL, HR_INTERVAL_S);
  dict_write_uint16(iter, MESSAGE_KEY_HR_COUNT, s_meta.count);
  dict_write_cstring(iter, MESSAGE_KEY_WORKOUT_NAME, s_meta.workout_name);
  dict_write_data(iter, MESSAGE_KEY_HR_DATA, s_bpm, s_meta.count);
  app_message_outbox_send();
}

void hr_ack(uint32_t client_id) {
  if (s_pending && client_id == s_meta.client_id) clear_trace();
}
