#include <pebble.h>
#include "packfmt.h"
#include "recorder.h"
#include "ui_preview.h"
#include "workouts_store.h"
#include "counters_store.h"
#include "session_queue.h"

#define MSG_SESSION_SET 3  // watch -> phone: one queued set summary to POST

// Home: list of the watch's workouts. Served from persistent storage (or the
// embedded defaults until the first sync), and refreshed over AppMessage from
// the phone/server on launch — see workouts_store.c and src/pkjs/index.js (M3).

static Window *s_window;
static MenuLayer *s_menu;
static PackedWorkout s_workout;  // unpacked on selection, shown by preview

static uint16_t get_num_rows(MenuLayer *menu, uint16_t section, void *ctx) {
  return workouts_count();
}

static void draw_row(GContext *ctx, const Layer *cell, MenuIndex *index, void *context) {
  uint16_t len;
  const uint8_t *data = workouts_get(index->row, &len);
  if (!data || len < 26) {
    return;
  }
  char name[25];
  char sub[24];
  memcpy(name, data, 24);
  name[24] = '\0';
  snprintf(sub, sizeof sub, "%d exercises", data[25]);
  menu_cell_basic_draw(ctx, cell, name, sub, NULL);
}

static void select_click(MenuLayer *menu, MenuIndex *index, void *ctx) {
  uint16_t len;
  const uint8_t *data = workouts_get(index->row, &len);
  if (data && packfmt_unpack(data, len, &s_workout)) {
    preview_window_push(&s_workout);
  } else {
    APP_LOG(APP_LOG_LEVEL_ERROR, "failed to unpack workout %d", index->row);
  }
}

static bool s_flushing;          // an offline-queue flush is in progress
static TextLayer *s_sync_layer;  // transient "Syncing..." banner on the home screen

static void sync_hide(void *ctx) {
  if (s_sync_layer) layer_set_hidden(text_layer_get_layer(s_sync_layer), true);
}

static void sync_indicator(bool active) {
  if (!s_sync_layer) return;
  if (active) {
    text_layer_set_text(s_sync_layer, "Syncing...");
    layer_set_hidden(text_layer_get_layer(s_sync_layer), false);
  } else {
    text_layer_set_text(s_sync_layer, "Synced");
    app_timer_register(1500, sync_hide, NULL);  // clear shortly after
  }
}

// Send the oldest queued set to the phone to POST; on its SQ_ACK we dequeue and
// send the next (see inbox_received). No-op if the queue is empty or the outbox
// is momentarily busy — a later SQ_PULL retries.
static void flush_next_queued(void) {
  SqSet set;
  if (!session_queue_oldest(&set)) {
    s_flushing = false;
    return;
  }
  DictionaryIterator *iter;
  if (app_message_outbox_begin(&iter) != APP_MSG_OK) return;
  dict_write_uint8(iter, MESSAGE_KEY_MSG_TYPE, MSG_SESSION_SET);
  dict_write_uint32(iter, MESSAGE_KEY_CLIENT_ID, set.client_set_id);
  dict_write_uint8(iter, MESSAGE_KEY_MOVEMENT, set.movement_id);
  dict_write_uint8(iter, MESSAGE_KEY_SET_INDEX, set.set_index);
  dict_write_uint8(iter, MESSAGE_KEY_TIMED, set.timed ? 1 : 0);
  dict_write_uint8(iter, MESSAGE_KEY_ACTUAL, set.actual);
  dict_write_uint16(iter, MESSAGE_KEY_WORK_SECS, set.work_secs);
  dict_write_uint32(iter, MESSAGE_KEY_PERFORMED_AT, set.performed_at);
  dict_write_cstring(iter, MESSAGE_KEY_WORKOUT_NAME, set.workout_name);
  app_message_outbox_send();
}

// Workout sync from the phone: {WK_TOTAL, WK_INDEX, WK_DATA} per workout, then
// {WK_DONE}. Accumulate, commit on done, and refresh the menu.
static void inbox_received(DictionaryIterator *iter, void *ctx) {
  Tuple *idx = dict_find(iter, MESSAGE_KEY_WK_INDEX);
  Tuple *data = dict_find(iter, MESSAGE_KEY_WK_DATA);
  Tuple *total = dict_find(iter, MESSAGE_KEY_WK_TOTAL);
  Tuple *done = dict_find(iter, MESSAGE_KEY_WK_DONE);

  if (idx && data) {
    if (idx->value->uint8 == 0 && total) {
      workouts_sync_begin(total->value->uint8);
    }
    workouts_sync_set(idx->value->uint8, data->value->data, data->length);
  }
  if (done) {
    if (workouts_sync_commit() && s_menu) {
      menu_layer_reload_data(s_menu);
    }
  }

  // Counter configs: one message, {CN_COUNT, CN_DATA} of packed 14-byte records.
  Tuple *cn_count = dict_find(iter, MESSAGE_KEY_CN_COUNT);
  Tuple *cn_data = dict_find(iter, MESSAGE_KEY_CN_DATA);
  if (cn_count && cn_data) {
    counters_sync_set(cn_data->value->data, cn_data->length, cn_count->value->uint8);
  }

  // Sync indicator + offline session-queue flush.
  if (dict_find(iter, MESSAGE_KEY_SYNC_BEGIN)) sync_indicator(true);
  if (dict_find(iter, MESSAGE_KEY_SYNC_END)) sync_indicator(false);
  Tuple *sq_ack = dict_find(iter, MESSAGE_KEY_SQ_ACK);
  if (sq_ack) {
    session_queue_ack(sq_ack->value->uint32);  // a set reached the server
    if (s_flushing) flush_next_queued();       // continue draining the backlog
  }
  if (dict_find(iter, MESSAGE_KEY_SQ_PULL)) {
    s_flushing = true;
    flush_next_queued();  // phone connected -> start flushing offline sets
  }
}

static void window_load(Window *window) {
  Layer *root = window_get_root_layer(window);
  s_menu = menu_layer_create(layer_get_bounds(root));
  menu_layer_set_callbacks(s_menu, NULL, (MenuLayerCallbacks){
      .get_num_rows = get_num_rows,
      .draw_row = draw_row,
      .select_click = select_click,
  });
#ifdef PBL_COLOR
  menu_layer_set_highlight_colors(s_menu, GColorDarkCandyAppleRed, GColorWhite);
#endif
  menu_layer_set_click_config_onto_window(s_menu, window);
  layer_add_child(root, menu_layer_get_layer(s_menu));

  // Transient sync banner, overlaid at the top; hidden until a sync runs.
  GRect wb = layer_get_bounds(root);
  s_sync_layer = text_layer_create(GRect(0, 0, wb.size.w, 18));
  text_layer_set_text_alignment(s_sync_layer, GTextAlignmentCenter);
  text_layer_set_font(s_sync_layer, fonts_get_system_font(FONT_KEY_GOTHIC_14_BOLD));
  text_layer_set_background_color(s_sync_layer, GColorBlack);
  text_layer_set_text_color(s_sync_layer, GColorWhite);
  layer_set_hidden(text_layer_get_layer(s_sync_layer), true);
  layer_add_child(root, text_layer_get_layer(s_sync_layer));
}

static void window_unload(Window *window) {
  text_layer_destroy(s_sync_layer);
  s_sync_layer = NULL;
  menu_layer_destroy(s_menu);
  s_menu = NULL;
}

int main(void) {
  recorder_init();          // registers outbox handlers
  workouts_init();          // load persisted / embedded workouts
  counters_init();          // load persisted counter-config overrides
  session_queue_init();     // load any offline set summaries awaiting upload
  app_message_register_inbox_received(inbox_received);
  app_message_open(512, RECORDER_OUTBOX_SIZE);

  s_window = window_create();
  window_set_window_handlers(s_window, (WindowHandlers){
      .load = window_load,
      .unload = window_unload,
  });
  window_stack_push(s_window, true);
  app_event_loop();
  window_destroy(s_window);
}
