#include "recorder.h"

#define REC_MAX_SAMPLES 1800  // 72 s at 25 Hz; 10.8 KB per slot
#define REC_RATE_HZ 25
#define CHUNK_BYTES 1500
#define MAX_RETRIES 3
#define NUM_SLOTS 2

enum { MSG_REC_META = 0, MSG_REC_CHUNK = 1, MSG_REC_DONE = 2 };

typedef struct __attribute__((packed)) {
  int16_t x, y, z;
} RecSample;

// Two slots decouple capture from upload: a new set records into the free slot
// while the previous set is still streaming to the phone. Before this the single
// buffer refused every set whose predecessor was still uploading — dropping
// roughly every other set (see recorder.h history).
typedef enum {
  SLOT_FREE,        // available
  SLOT_CAPTURING,   // samples being appended
  SLOT_STAGED,      // captured + labelled metadata, waiting for the sender
  SLOT_SEND_META,   // meta message in flight
  SLOT_SEND_CHUNKS, // chunk in flight
  SLOT_WAIT_LABEL,  // chunks sent, waiting for the corrected count
  SLOT_SEND_DONE,   // final label message in flight
} SlotState;

typedef struct {
  RecSample buf[REC_MAX_SAMPLES];
  uint16_t count;
  bool truncated;
  SlotState state;
  // Staged metadata.
  uint32_t rec_id;
  uint32_t client_set_id;  // watch-unique set id; server idempotency key
  uint8_t movement, set_index;
  bool timed;
  char workout_name[25];
  uint8_t actual;
  bool label_ready;
  uint16_t chunk_index, chunk_total;
} Slot;

static Slot s_slots[NUM_SLOTS];
static int8_t s_cap = -1;  // slot currently capturing, or -1
static int8_t s_snd = -1;  // slot currently sending, or -1
static uint8_t s_retries;

static void send_current(void);

static void slot_reset(Slot *s) {
  s->state = SLOT_FREE;
  s->count = 0;
  s->truncated = false;
  s->label_ready = false;
}

static int8_t find_free_slot(void) {
  for (int8_t i = 0; i < NUM_SLOTS; i++) {
    if (s_slots[i].state == SLOT_FREE) return i;
  }
  return -1;
}

// Kick off the next staged upload if the send pipeline is idle. Preserves
// capture order (slots are scanned low-to-high; only one is ever staged at a
// time given the set/rest cadence).
static void maybe_start_send(void) {
  if (s_snd != -1) return;
  for (int8_t i = 0; i < NUM_SLOTS; i++) {
    if (s_slots[i].state == SLOT_STAGED) {
      s_snd = i;
      s_retries = 0;
      s_slots[i].chunk_index = 0;
      s_slots[i].state = SLOT_SEND_META;
      send_current();
      return;
    }
  }
}

static void outbox_sent(DictionaryIterator *iter, void *ctx) {
  if (s_snd == -1) return;
  Slot *s = &s_slots[s_snd];
  s_retries = 0;
  switch (s->state) {
    case SLOT_SEND_META:
      s->state = SLOT_SEND_CHUNKS;
      s->chunk_index = 0;
      send_current();
      break;
    case SLOT_SEND_CHUNKS:
      s->chunk_index++;
      if (s->chunk_index >= s->chunk_total) {
        s->state = s->label_ready ? SLOT_SEND_DONE : SLOT_WAIT_LABEL;
        if (s->state == SLOT_SEND_DONE) {
          send_current();
        }
      } else {
        send_current();
      }
      break;
    case SLOT_SEND_DONE:
      APP_LOG(APP_LOG_LEVEL_INFO, "recording %lu uploaded (%u samples)",
              (unsigned long)s->rec_id, s->count);
      slot_reset(s);
      s_snd = -1;
      maybe_start_send();
      break;
    default:
      break;
  }
}

static void outbox_failed(DictionaryIterator *iter, AppMessageResult reason, void *ctx) {
  if (s_snd == -1) return;
  Slot *s = &s_slots[s_snd];
  if (s->state == SLOT_WAIT_LABEL) {
    return;  // nothing in flight; waiting on the UI, not the radio
  }
  if (++s_retries > MAX_RETRIES) {
    APP_LOG(APP_LOG_LEVEL_ERROR, "recording transfer failed (reason %d), dropping", reason);
    slot_reset(s);
    s_snd = -1;
    maybe_start_send();
    return;
  }
  send_current();
}

static void send_current(void) {
  if (s_snd == -1) return;
  Slot *s = &s_slots[s_snd];
  DictionaryIterator *iter;
  if (app_message_outbox_begin(&iter) != APP_MSG_OK) {
    if (++s_retries > MAX_RETRIES) {
      slot_reset(s);
      s_snd = -1;
      maybe_start_send();
    }
    return;
  }
  switch (s->state) {
    case SLOT_SEND_META:
      dict_write_uint8(iter, MESSAGE_KEY_MSG_TYPE, MSG_REC_META);
      dict_write_uint32(iter, MESSAGE_KEY_REC_ID, s->rec_id);
      dict_write_uint32(iter, MESSAGE_KEY_CLIENT_ID, s->client_set_id);
      dict_write_uint8(iter, MESSAGE_KEY_MOVEMENT, s->movement);
      dict_write_uint8(iter, MESSAGE_KEY_SET_INDEX, s->set_index);
      dict_write_uint8(iter, MESSAGE_KEY_TIMED, s->timed ? 1 : 0);
      dict_write_uint8(iter, MESSAGE_KEY_RATE, REC_RATE_HZ);
      dict_write_uint16(iter, MESSAGE_KEY_SAMPLE_COUNT, s->count);
      dict_write_uint8(iter, MESSAGE_KEY_TRUNCATED, s->truncated ? 1 : 0);
      dict_write_cstring(iter, MESSAGE_KEY_WORKOUT_NAME, s->workout_name);
      break;
    case SLOT_SEND_CHUNKS: {
      uint32_t total_bytes = (uint32_t)s->count * sizeof(RecSample);
      uint32_t off = (uint32_t)s->chunk_index * CHUNK_BYTES;
      uint16_t len = (total_bytes - off) > CHUNK_BYTES ? CHUNK_BYTES : (uint16_t)(total_bytes - off);
      dict_write_uint8(iter, MESSAGE_KEY_MSG_TYPE, MSG_REC_CHUNK);
      dict_write_uint32(iter, MESSAGE_KEY_REC_ID, s->rec_id);
      dict_write_uint16(iter, MESSAGE_KEY_SEQ, s->chunk_index);
      dict_write_data(iter, MESSAGE_KEY_CHUNK, ((const uint8_t *)s->buf) + off, len);
      break;
    }
    case SLOT_SEND_DONE:
      dict_write_uint8(iter, MESSAGE_KEY_MSG_TYPE, MSG_REC_DONE);
      dict_write_uint32(iter, MESSAGE_KEY_REC_ID, s->rec_id);
      dict_write_uint8(iter, MESSAGE_KEY_ACTUAL, s->actual);
      break;
    default:
      return;
  }
  app_message_outbox_send();
}

void recorder_init(void) {
  for (int8_t i = 0; i < NUM_SLOTS; i++) {
    slot_reset(&s_slots[i]);
  }
  s_cap = -1;
  s_snd = -1;
  app_message_register_outbox_sent(outbox_sent);
  app_message_register_outbox_failed(outbox_failed);
  // AppMessage is opened once in main() (shared with workout-sync inbox).
}

bool recorder_is_capturing(void) {
  return s_cap != -1 && s_slots[s_cap].state == SLOT_CAPTURING;
}

void recorder_begin(void) {
  if (s_cap != -1 && s_slots[s_cap].state == SLOT_CAPTURING) {
    return;  // already capturing this set
  }
  int8_t slot = find_free_slot();
  if (slot == -1) {
    // Both slots busy (one capturing-then-staged, one still uploading): the
    // upload is slower than two set-cycles. Rare; the set goes unrecorded.
    APP_LOG(APP_LOG_LEVEL_WARNING, "recorder full (both slots busy), set not recorded");
    return;
  }
  s_cap = slot;
  Slot *s = &s_slots[slot];
  s->state = SLOT_CAPTURING;
  s->count = 0;
  s->truncated = false;
  s->label_ready = false;
}

void recorder_feed(int16_t x, int16_t y, int16_t z) {
  if (s_cap == -1) return;
  Slot *s = &s_slots[s_cap];
  if (s->state != SLOT_CAPTURING) return;
  if (s->count >= REC_MAX_SAMPLES) {
    s->truncated = true;
    return;
  }
  s->buf[s->count++] = (RecSample){.x = x, .y = y, .z = z};
}

void recorder_stage(uint8_t movement_id, uint8_t set_index, bool timed,
                    const char *workout_name, uint32_t client_set_id) {
  if (s_cap == -1) return;
  Slot *s = &s_slots[s_cap];
  if (s->state != SLOT_CAPTURING) return;
  if (s->count == 0) {
    slot_reset(s);
    s_cap = -1;
    return;
  }
  s->rec_id = (uint32_t)time(NULL);
  s->client_set_id = client_set_id;
  s->movement = movement_id;
  s->set_index = set_index;
  s->timed = timed;
  strncpy(s->workout_name, workout_name, sizeof s->workout_name - 1);
  s->workout_name[sizeof s->workout_name - 1] = '\0';
  s->chunk_total =
      (uint16_t)(((uint32_t)s->count * sizeof(RecSample) + CHUNK_BYTES - 1) / CHUNK_BYTES);
  s->label_ready = false;
  s->state = SLOT_STAGED;
  s_cap = -1;
  maybe_start_send();
}

void recorder_set_label(uint8_t actual) {
  // Applies to the most recently staged set that hasn't been labelled yet.
  // The set/rest cadence guarantees exactly one such slot at label time.
  for (int8_t i = 0; i < NUM_SLOTS; i++) {
    Slot *s = &s_slots[i];
    if (s->state == SLOT_FREE || s->state == SLOT_CAPTURING || s->label_ready) {
      continue;
    }
    s->actual = actual;
    s->label_ready = true;
    if (s->state == SLOT_WAIT_LABEL && i == s_snd) {
      s->state = SLOT_SEND_DONE;
      send_current();
    }
    return;
  }
}

void recorder_abort(void) {
  // Drop only an unstaged capture; in-flight uploads are left to finish.
  if (s_cap != -1 && s_slots[s_cap].state == SLOT_CAPTURING) {
    slot_reset(&s_slots[s_cap]);
    s_cap = -1;
  }
}
