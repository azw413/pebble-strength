#include <pebble.h>
#include "embedded_workouts.h"
#include "packfmt.h"
#include "recorder.h"
#include "ui_preview.h"

// Home: list of synced workouts. M1 ships them embedded in the binary
// (generated from the dev server); real AppMessage sync replaces this in M3.

static Window *s_window;
static MenuLayer *s_menu;
static PackedWorkout s_workout;  // unpacked on selection, shown by preview

static uint16_t get_num_rows(MenuLayer *menu, uint16_t section, void *ctx) {
  return EMBEDDED_WORKOUT_COUNT;
}

static void draw_row(GContext *ctx, const Layer *cell, MenuIndex *index, void *context) {
  const EmbeddedWorkout *w = &EMBEDDED_WORKOUTS[index->row];
  char name[25];
  char sub[24];
  memcpy(name, w->data, 24);
  name[24] = '\0';
  snprintf(sub, sizeof sub, "%d exercises", w->data[25]);
  menu_cell_basic_draw(ctx, cell, name, sub, NULL);
}

static void select_click(MenuLayer *menu, MenuIndex *index, void *ctx) {
  const EmbeddedWorkout *w = &EMBEDDED_WORKOUTS[index->row];
  if (packfmt_unpack(w->data, w->len, &s_workout)) {
    preview_window_push(&s_workout);
  } else {
    APP_LOG(APP_LOG_LEVEL_ERROR, "failed to unpack workout %d", index->row);
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
}

int main(void) {
  recorder_init();
  s_window = window_create();
  window_set_window_handlers(s_window, (WindowHandlers){
      .load = window_load,
      .unload = window_unload,
  });
  window_stack_push(s_window, true);
  app_event_loop();
  window_destroy(s_window);
}
