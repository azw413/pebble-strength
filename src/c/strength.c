#include <pebble.h>
#include "packfmt.h"
#include "recorder.h"
#include "ui_preview.h"
#include "workouts_store.h"

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
}

static void window_unload(Window *window) {
  menu_layer_destroy(s_menu);
  s_menu = NULL;
}

int main(void) {
  recorder_init();          // registers outbox handlers
  workouts_init();          // load persisted / embedded workouts
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
