#include <pebble.h>
#include "movements.h"
#include "packfmt.h"
#include "ui_preview.h"
#include "ui_session.h"

static Window *s_window;
static MenuLayer *s_menu;
static const PackedWorkout *s_workout;

static uint16_t get_num_rows(MenuLayer *menu, uint16_t section, void *ctx) {
  return 1 + s_workout->exercise_count;
}

static void draw_row(GContext *ctx, const Layer *cell, MenuIndex *index, void *context) {
  char sub[40];
  if (index->row == 0) {
    menu_cell_basic_draw(ctx, cell, "Start workout", s_workout->name, NULL);
    return;
  }
  const PackExercise *e = &s_workout->exercises[index->row - 1];
  PackSet first = e->sets[0];
  const char *unit = (e->flags & PACK_FLAG_TIMED) ? " s hold" : "";
  if (e->weight_q > 0) {
    snprintf(sub, sizeof sub, "%d x %d%s @ %d kg", e->set_count, first.target, unit,
             e->weight_q / 4);
  } else {
    snprintf(sub, sizeof sub, "%d x %d%s · rest %d s", e->set_count, first.target, unit,
             packfmt_rest_secs(first));
  }
  menu_cell_basic_draw(ctx, cell, movement_name(e->movement_id), sub, NULL);
}

static void select_click(MenuLayer *menu, MenuIndex *index, void *ctx) {
  if (index->row == 0) {
    session_window_push(s_workout);
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
  window_destroy(s_window);
  s_window = NULL;
}

void preview_window_push(const PackedWorkout *workout) {
  s_workout = workout;
  s_window = window_create();
  window_set_window_handlers(s_window, (WindowHandlers){
      .load = window_load,
      .unload = window_unload,
  });
  window_stack_push(s_window, true);
}
