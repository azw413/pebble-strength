#include "recorder.h"

#define REC_MAX_SAMPLES 2400  // 96 s at 25 Hz; 14.4 KB buffer
#define REC_RATE_HZ 25
#define CHUNK_BYTES 1500
#define MAX_RETRIES 3

enum { MSG_REC_META = 0, MSG_REC_CHUNK = 1, MSG_REC_DONE = 2 };

typedef struct __attribute__((packed)) {
  int16_t x, y, z;
} RecSample;

typedef enum {
  REC_IDLE,        // buffer free
  REC_CAPTURING,   // samples being appended
  REC_SEND_META,   // staged: meta message in flight
  REC_SEND_CHUNKS, // chunk i in flight
  REC_WAIT_LABEL,  // chunks sent, waiting for the corrected count
  REC_SEND_DONE,   // final label message in flight
} RecState;

static RecSample s_buf[REC_MAX_SAMPLES];
static uint16_t s_count;
static bool s_truncated;
static RecState s_state = REC_IDLE;

// Staged metadata.
static uint32_t s_rec_id;
static uint8_t s_movement, s_set_index;
static bool s_timed;
static char s_workout_name[25];
static uint8_t s_actual;
static bool s_label_ready;
static uint16_t s_chunk_index, s_chunk_total;
static uint8_t s_retries;

static void send_current(void);

static void reset(void) {
  s_state = REC_IDLE;
  s_count = 0;
  s_truncated = false;
  s_label_ready = false;
}

static void outbox_sent(DictionaryIterator *iter, void *ctx) {
  s_retries = 0;
  switch (s_state) {
    case REC_SEND_META:
      s_state = REC_SEND_CHUNKS;
      s_chunk_index = 0;
      send_current();
      break;
    case REC_SEND_CHUNKS:
      s_chunk_index++;
      if (s_chunk_index >= s_chunk_total) {
        s_state = s_label_ready ? REC_SEND_DONE : REC_WAIT_LABEL;
        if (s_state == REC_SEND_DONE) {
          send_current();
        }
      } else {
        send_current();
      }
      break;
    case REC_SEND_DONE:
      APP_LOG(APP_LOG_LEVEL_INFO, "recording %lu uploaded (%u samples)",
              (unsigned long)s_rec_id, s_count);
      reset();
      break;
    default:
      break;
  }
}

static void outbox_failed(DictionaryIterator *iter, AppMessageResult reason, void *ctx) {
  if (s_state == REC_IDLE || s_state == REC_CAPTURING || s_state == REC_WAIT_LABEL) {
    return;
  }
  if (++s_retries > MAX_RETRIES) {
    APP_LOG(APP_LOG_LEVEL_ERROR, "recording transfer failed (reason %d), dropping", reason);
    reset();
    return;
  }
  send_current();
}

static void send_current(void) {
  DictionaryIterator *iter;
  if (app_message_outbox_begin(&iter) != APP_MSG_OK) {
    if (++s_retries > MAX_RETRIES) {
      reset();
    }
    return;
  }
  switch (s_state) {
    case REC_SEND_META:
      dict_write_uint8(iter, MESSAGE_KEY_MSG_TYPE, MSG_REC_META);
      dict_write_uint32(iter, MESSAGE_KEY_REC_ID, s_rec_id);
      dict_write_uint8(iter, MESSAGE_KEY_MOVEMENT, s_movement);
      dict_write_uint8(iter, MESSAGE_KEY_SET_INDEX, s_set_index);
      dict_write_uint8(iter, MESSAGE_KEY_TIMED, s_timed ? 1 : 0);
      dict_write_uint8(iter, MESSAGE_KEY_RATE, REC_RATE_HZ);
      dict_write_uint16(iter, MESSAGE_KEY_SAMPLE_COUNT, s_count);
      dict_write_uint8(iter, MESSAGE_KEY_TRUNCATED, s_truncated ? 1 : 0);
      dict_write_cstring(iter, MESSAGE_KEY_WORKOUT_NAME, s_workout_name);
      break;
    case REC_SEND_CHUNKS: {
      uint32_t total_bytes = (uint32_t)s_count * sizeof(RecSample);
      uint32_t off = (uint32_t)s_chunk_index * CHUNK_BYTES;
      uint16_t len = (total_bytes - off) > CHUNK_BYTES ? CHUNK_BYTES : (uint16_t)(total_bytes - off);
      dict_write_uint8(iter, MESSAGE_KEY_MSG_TYPE, MSG_REC_CHUNK);
      dict_write_uint32(iter, MESSAGE_KEY_REC_ID, s_rec_id);
      dict_write_uint16(iter, MESSAGE_KEY_SEQ, s_chunk_index);
      dict_write_data(iter, MESSAGE_KEY_CHUNK, ((const uint8_t *)s_buf) + off, len);
      break;
    }
    case REC_SEND_DONE:
      dict_write_uint8(iter, MESSAGE_KEY_MSG_TYPE, MSG_REC_DONE);
      dict_write_uint32(iter, MESSAGE_KEY_REC_ID, s_rec_id);
      dict_write_uint8(iter, MESSAGE_KEY_ACTUAL, s_actual);
      break;
    default:
      return;
  }
  app_message_outbox_send();
}

void recorder_init(void) {
  app_message_register_outbox_sent(outbox_sent);
  app_message_register_outbox_failed(outbox_failed);
  app_message_open(256, CHUNK_BYTES + 128);
}

bool recorder_is_capturing(void) {
  return s_state == REC_CAPTURING;
}

void recorder_begin(void) {
  if (s_state != REC_IDLE) {
    APP_LOG(APP_LOG_LEVEL_WARNING, "recorder busy (state %d), set not recorded", s_state);
    return;
  }
  s_state = REC_CAPTURING;
  s_count = 0;
  s_truncated = false;
  s_label_ready = false;
}

void recorder_feed(int16_t x, int16_t y, int16_t z) {
  if (s_state != REC_CAPTURING) {
    return;
  }
  if (s_count >= REC_MAX_SAMPLES) {
    s_truncated = true;
    return;
  }
  s_buf[s_count++] = (RecSample){.x = x, .y = y, .z = z};
}

void recorder_stage(uint8_t movement_id, uint8_t set_index, bool timed,
                    const char *workout_name) {
  if (s_state != REC_CAPTURING) {
    return;
  }
  if (s_count == 0) {
    reset();
    return;
  }
  s_rec_id = (uint32_t)time(NULL);
  s_movement = movement_id;
  s_set_index = set_index;
  s_timed = timed;
  strncpy(s_workout_name, workout_name, sizeof s_workout_name - 1);
  s_workout_name[sizeof s_workout_name - 1] = '\0';
  s_chunk_total =
      (uint16_t)(((uint32_t)s_count * sizeof(RecSample) + CHUNK_BYTES - 1) / CHUNK_BYTES);
  s_retries = 0;
  s_state = REC_SEND_META;
  send_current();
}

void recorder_set_label(uint8_t actual) {
  s_actual = actual;
  s_label_ready = true;
  if (s_state == REC_WAIT_LABEL) {
    s_state = REC_SEND_DONE;
    send_current();
  }
}

void recorder_abort(void) {
  if (s_state == REC_CAPTURING) {
    reset();
  }
}
