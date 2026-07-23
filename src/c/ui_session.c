#include <pebble.h>
#include "movements.h"
#include "packfmt.h"
#include "recorder.h"
#include "rep_counter.h"
#include "counters_store.h"
#include "session_queue.h"
#include "ui_session.h"

// Guided session: Active set -> Rest -> ... -> Summary (SPEC.md §7).
// M1: rep counts are entered with Up/Down; the accel counter arrives in M2.

typedef enum { PHASE_ACTIVE, PHASE_REST, PHASE_SUMMARY } Phase;
typedef enum { HOLD_IDLE, HOLD_LEADIN, HOLD_RUNNING } HoldState;

enum {
  ACTION_RESUME,
  ACTION_SKIP_EXERCISE,
  ACTION_END_WORKOUT,
  ACTION_DISCARD,
};

static Window *s_window;
static Layer *s_layer;
static ActionMenuLevel *s_menu_level;
static AppTimer *s_timer;

static PackedWorkout s_workout;
static Phase s_phase;
static uint8_t s_cur_ex;
static uint8_t s_cur_set;
static int16_t s_counter;         // rep mode: reps to confirm
static HoldState s_hold_state;
static int16_t s_hold_remaining;  // timed mode: seconds left in hold
static uint8_t s_leadin;
static int16_t s_rest_remaining;
static uint8_t s_actual[PACK_MAX_EXERCISES][PACK_MAX_SETS];
static uint16_t s_work_secs[PACK_MAX_EXERCISES][PACK_MAX_SETS];
static time_t s_started;
static bool s_discard_on_close;

// Work timer for rep sets: the big display until the rep counter is trusted.
static int16_t s_work_elapsed;
// Set once Up/Down corrects the count, so auto-counting can't clobber the fix.
static bool s_count_locked;

// M2: accel capture + auto rep counting.
static RepCounter s_rc;
static bool s_accel_on;
static bool s_label_pending;
static uint8_t s_label_ex, s_label_set;
static uint32_t s_label_client_id;  // stable set id, shared by accel + queue paths

static PackExercise *cur_ex(void) { return &s_workout.exercises[s_cur_ex]; }
static PackSet cur_set(void) { return cur_ex()->sets[s_cur_set]; }
static bool cur_timed(void) { return (cur_ex()->flags & PACK_FLAG_TIMED) != 0; }
static bool cur_amrap(void) { return (cur_ex()->flags & PACK_FLAG_AMRAP) != 0; }

static void cancel_timer(void) {
  if (s_timer) {
    app_timer_cancel(s_timer);
    s_timer = NULL;
  }
}

static void redraw(void);

static void accel_handler(AccelData *data, uint32_t num) {
  bool counted = false;
  for (uint32_t i = 0; i < num; i++) {
    if (data[i].did_vibrate) {
      continue;
    }
    recorder_feed(data[i].x, data[i].y, data[i].z);
    if (s_phase == PHASE_ACTIVE && !cur_timed()) {
      if (rep_counter_feed(&s_rc, data[i].x, data[i].y, data[i].z,
                           (uint32_t)data[i].timestamp)) {
        // Keep counting for the tuning corpus, but never over a manual fix.
        if (!s_count_locked && s_counter < 250) s_counter++;
        counted = true;
      }
    }
  }
  // No per-rep vibe — a miscount shouldn't buzz; you confirm/correct the count
  // on the rest screen with Up/Down.
  if (counted && !s_count_locked) {
    redraw();
  }
}

static void accel_start(void) {
  if (s_accel_on) return;
  s_accel_on = true;
  accel_data_service_subscribe(25, accel_handler);
  accel_service_set_sampling_rate(ACCEL_SAMPLING_25HZ);
}

static void accel_stop(void) {
  if (!s_accel_on) return;
  s_accel_on = false;
  accel_data_service_unsubscribe();
}

// Ship the (possibly rest-screen-corrected) count as the recording's label.
static void finalize_label(void) {
  if (s_label_pending) {
    s_label_pending = false;
    uint8_t actual = s_actual[s_label_ex][s_label_set];
    recorder_set_label(actual);
    // Durable offline queue: enqueue every finished set (summary only). Drained
    // by the accel-upload ack when online, flushed on reconnect when not.
    const PackExercise *e = &s_workout.exercises[s_label_ex];
    SqSet set;
    set.client_set_id = s_label_client_id;
    set.performed_at = (uint32_t)time(NULL);
    set.movement_id = e->movement_id;
    set.set_index = s_label_set;
    set.timed = (e->flags & PACK_FLAG_TIMED) != 0;
    set.actual = actual;
    set.work_secs = s_work_secs[s_label_ex][s_label_set];
    strncpy(set.workout_name, s_workout.name, sizeof set.workout_name - 1);
    set.workout_name[sizeof set.workout_name - 1] = '\0';
    session_queue_enqueue(&set);
  }
}

static void tick(void *context);

static void schedule_tick(void) {
  cancel_timer();
  s_timer = app_timer_register(1000, tick, NULL);
}

static void redraw(void) {
  if (s_layer) {
    layer_mark_dirty(s_layer);
  }
}

static bool is_last_set(void) {
  return s_cur_ex == s_workout.exercise_count - 1 && s_cur_set == cur_ex()->set_count - 1;
}

// Begin capturing the current set — recorder + accel + the driving tick. Called
// once we're actually in position: either the 3-2-1 lead-in just finished, or the
// preceding rest counted its last 3s down (so no separate lead-in is needed).
// Keeping the get-into-position motion *out* of the recording is what makes the
// rep count clean.
static void start_capture(void) {
  if (cur_timed()) {
    s_hold_state = HOLD_RUNNING;
    s_hold_remaining = cur_set().target;
  } else {
    s_hold_state = HOLD_IDLE;  // out of the lead-in; rep views key off !cur_timed()
  }
  recorder_begin();
  accel_start();
  schedule_tick();
}

// Enter a set. `with_leadin` runs a 3-2-1 "get into position" countdown before
// capture starts — used when there's no preceding rest (first set, zero rest, or
// a skipped exercise). After a real rest we pass false: the rest's final 3s
// already served as the lead-in, so capture starts immediately.
static void enter_active(bool with_leadin) {
  s_phase = PHASE_ACTIVE;
  cancel_timer();
  if (cur_timed()) {
    s_hold_remaining = cur_set().target;
  } else {
    // Work timer is the headline; auto-count runs underneath it. Up/Down
    // corrects (and locks) the count, Select confirms.
    s_counter = 0;
    s_work_elapsed = 0;
    s_count_locked = false;
    // Data-driven counter: the movement's tuned CounterConfig (axis + band-pass
    // + thresholds) — a downloaded override if synced, else the compiled default.
    CounterConfig cfg;
    counter_config_get(cur_ex()->movement_id, &cfg);
    rep_counter_init(&s_rc, &cfg, 25);
  }
  if (with_leadin) {
    s_hold_state = HOLD_LEADIN;
    s_leadin = 3;
    vibes_short_pulse();  // "3" — get into position
    schedule_tick();
  } else {
    start_capture();
  }
  redraw();
}

static void enter_summary(void) {
  s_phase = PHASE_SUMMARY;
  cancel_timer();
  accel_stop();
  recorder_abort();   // drop any unstaged mid-set capture
  finalize_label();   // label a set whose rest was cut short
  vibes_long_pulse();
  redraw();
}

// `led_in` = a countdown already happened (the rest's final 3s), so the next set
// starts capturing immediately. Otherwise (zero rest, or a rest skipped before
// its countdown) the next set gets its own 3-2-1 lead-in.
static void advance_after_rest(bool led_in) {
  finalize_label();
  if (s_cur_set + 1 < cur_ex()->set_count) {
    s_cur_set++;
  } else {
    s_cur_ex++;
    s_cur_set = 0;
    if (s_cur_ex >= s_workout.exercise_count) {
      enter_summary();
      return;
    }
  }
  enter_active(!led_in);
}

static void finish_set(uint8_t actual) {
  s_actual[s_cur_ex][s_cur_set] = actual;
  // Work time: counted up for rep sets, elapsed hold for timed ones.
  s_work_secs[s_cur_ex][s_cur_set] =
      cur_timed() ? (uint16_t)actual : (uint16_t)s_work_elapsed;
  accel_stop();
  s_label_client_id = session_queue_next_id();
  recorder_stage(cur_ex()->movement_id, s_cur_set, cur_timed(), s_workout.name,
                 s_label_client_id);
  s_label_pending = true;
  s_label_ex = s_cur_ex;
  s_label_set = s_cur_set;
  if (is_last_set()) {
    enter_summary();
    return;
  }
  uint16_t rest = packfmt_rest_secs(cur_set());
  if (rest == 0) {
    advance_after_rest(false);  // no rest -> the next set gets its own lead-in
    return;
  }
  s_phase = PHASE_REST;
  s_rest_remaining = rest;
  schedule_tick();
  redraw();
}

static void tick(void *context) {
  s_timer = NULL;
  if (s_phase == PHASE_REST) {
    s_rest_remaining--;
    if (s_rest_remaining <= 0) {
      vibes_double_pulse();      // GO — the set starts now
      advance_after_rest(true);  // the last 3s were the get-into-position lead-in
      return;
    }
    // 10s warning, then a 3-2-1 countdown so you're in position as rest ends.
    if (s_rest_remaining == 10 || s_rest_remaining <= 3) {
      vibes_short_pulse();
    }
    schedule_tick();
    redraw();
  } else if (s_phase == PHASE_ACTIVE && s_hold_state == HOLD_LEADIN) {
    // Get-into-position countdown (rep and timed sets alike). Checked before the
    // rep branch below, since a rep set is also !cur_timed() during its lead-in.
    s_leadin--;
    if (s_leadin == 0) {
      vibes_double_pulse();      // GO
      start_capture();
      redraw();
      return;
    }
    vibes_short_pulse();
    schedule_tick();
    redraw();
  } else if (s_phase == PHASE_ACTIVE && !cur_timed()) {
    s_work_elapsed++;
    schedule_tick();
    redraw();
  } else if (s_phase == PHASE_ACTIVE && s_hold_state == HOLD_RUNNING) {
    s_hold_remaining--;
    if (s_hold_remaining <= 0) {
      vibes_long_pulse();
      finish_set(cur_set().target);
      return;
    }
    schedule_tick();
    redraw();
  }
}

// ---- Rendering ----

static void draw_text(GContext *ctx, const char *text, const char *font_key, GRect box) {
  graphics_draw_text(ctx, text, fonts_get_system_font(font_key), box,
                     GTextOverflowModeTrailingEllipsis, GTextAlignmentCenter, NULL);
}

// Same, but wraps onto a second line instead of ellipsizing — for longer
// strings like "Next: 20s Bulgarian Split Squat".
static void draw_text_wrap(GContext *ctx, const char *text, const char *font_key, GRect box) {
  graphics_draw_text(ctx, text, fonts_get_system_font(font_key), box,
                     GTextOverflowModeWordWrap, GTextAlignmentCenter, NULL);
}

// NB: ROBOTO_BOLD_SUBSET_49 is a subset font — digits and ':' only. Any string
// containing letters must use a GOTHIC face or it renders blank.
#define FONT_HUGE_NUM FONT_KEY_ROBOTO_BOLD_SUBSET_49

static void render_active(GContext *ctx, GRect b) {
  char buf[64];
  const PackExercise *e = cur_ex();

  draw_text(ctx, movement_name(e->movement_id), FONT_KEY_GOTHIC_24_BOLD,
            GRect(2, 0, b.size.w - 4, 28));

  if (e->weight_q > 0) {
    snprintf(buf, sizeof buf, "Set %d/%d  %d kg", s_cur_set + 1, e->set_count,
             e->weight_q / 4);
  } else {
    snprintf(buf, sizeof buf, "Set %d of %d", s_cur_set + 1, e->set_count);
  }
  draw_text(ctx, buf, FONT_KEY_GOTHIC_24, GRect(2, 26, b.size.w - 4, 26));

  GRect big = GRect(0, 50, b.size.w, 54);       // huge digits
  GRect sub = GRect(2, 104, b.size.w - 4, 32);  // large secondary line
  GRect foot = GRect(2, b.size.h - 24, b.size.w - 4, 22);

#ifdef PBL_COLOR
  graphics_context_set_text_color(ctx, GColorDarkCandyAppleRed);
#endif
  if (s_hold_state == HOLD_LEADIN) {
    // Get into position — the 3-2-1 before capture starts (rep and timed alike).
    snprintf(buf, sizeof buf, "%d", s_leadin);
    draw_text(ctx, buf, FONT_HUGE_NUM, big);
    graphics_context_set_text_color(ctx, GColorBlack);
    draw_text(ctx, "get ready", FONT_KEY_GOTHIC_28_BOLD, sub);
    draw_text(ctx, "get into position", FONT_KEY_GOTHIC_18, foot);
  } else if (!cur_timed()) {
    // Reps are the headline — the big count you read and correct. (BITHAM has
    // the '/', unlike the digits-only huge font.) Work timer sits small below.
    if (cur_amrap()) {
      snprintf(buf, sizeof buf, "%d", s_counter);
    } else {
      snprintf(buf, sizeof buf, "%d / %d", s_counter, cur_set().target);
    }
    draw_text(ctx, buf, FONT_KEY_BITHAM_42_BOLD, GRect(0, 50, b.size.w, 50));
    graphics_context_set_text_color(ctx, GColorBlack);
    snprintf(buf, sizeof buf, "reps    work %d:%02d", s_work_elapsed / 60, s_work_elapsed % 60);
    draw_text(ctx, buf, FONT_KEY_GOTHIC_18, GRect(2, 104, b.size.w - 4, 22));
    draw_text(ctx, "select = done", FONT_KEY_GOTHIC_18, foot);
  } else if (s_hold_state == HOLD_IDLE) {
    snprintf(buf, sizeof buf, "%d", cur_set().target);
    draw_text(ctx, buf, FONT_HUGE_NUM, big);
    graphics_context_set_text_color(ctx, GColorBlack);
    draw_text(ctx, "second hold", FONT_KEY_GOTHIC_28_BOLD, sub);
    draw_text(ctx, "select = start", FONT_KEY_GOTHIC_18, foot);
  } else {
    snprintf(buf, sizeof buf, "%d", s_hold_remaining);
    draw_text(ctx, buf, FONT_HUGE_NUM, big);
    graphics_context_set_text_color(ctx, GColorBlack);
    draw_text(ctx, "hold!", FONT_KEY_GOTHIC_28_BOLD, sub);
    draw_text(ctx, "select = end early", FONT_KEY_GOTHIC_18, foot);
  }
}

static void render_rest(GContext *ctx, GRect b) {
  char buf[48];
  // The last 3s of rest double as the get-into-position countdown for the next
  // set, so capture starts clean the instant rest hits zero.
  bool leadin = s_rest_remaining <= 3;
  draw_text(ctx, leadin ? "Get ready" : "Rest", FONT_KEY_GOTHIC_24_BOLD,
            GRect(2, 0, b.size.w - 4, 22));

#ifdef PBL_COLOR
  graphics_context_set_text_color(ctx, leadin ? GColorDarkCandyAppleRed : GColorDukeBlue);
#endif
  if (leadin) {
    snprintf(buf, sizeof buf, "%d", s_rest_remaining);  // 3 · 2 · 1
  } else {
    snprintf(buf, sizeof buf, "%d:%02d", s_rest_remaining / 60, s_rest_remaining % 60);
  }
  draw_text(ctx, buf, FONT_HUGE_NUM, GRect(0, 20, b.size.w, 50));
  graphics_context_set_text_color(ctx, GColorBlack);

  // The set you just finished — correct it here with Up/Down.
  snprintf(buf, sizeof buf, "done: %d%s", s_actual[s_cur_ex][s_cur_set],
           cur_timed() ? " s" : "");
  draw_text(ctx, buf, FONT_KEY_GOTHIC_28_BOLD, GRect(2, 68, b.size.w - 4, 30));

  // What's coming up — the thing you're resting for — in red, with its target.
  uint8_t nx = s_cur_ex, ns = s_cur_set + 1;
  if (ns >= cur_ex()->set_count) {
    nx++;
    ns = 0;
  }
  if (nx < s_workout.exercise_count) {
    const PackExercise *e = &s_workout.exercises[nx];
    uint8_t target = e->sets[ns].target;
    const char *name = movement_name(e->movement_id);
    if (e->flags & PACK_FLAG_TIMED) {
      snprintf(buf, sizeof buf, "Next: %ds %s", target, name);
    } else if (e->flags & PACK_FLAG_AMRAP) {
      snprintf(buf, sizeof buf, "Next: %s", name);
    } else {
      snprintf(buf, sizeof buf, "Next: %d %s", target, name);
    }
#ifdef PBL_COLOR
    graphics_context_set_text_color(ctx, GColorDarkCandyAppleRed);
#endif
    draw_text_wrap(ctx, buf, FONT_KEY_GOTHIC_24_BOLD, GRect(2, 98, b.size.w - 4, 48));
    graphics_context_set_text_color(ctx, GColorBlack);
  }

  draw_text(ctx, "up/dn fix  ·  select go", FONT_KEY_GOTHIC_18,
            GRect(2, b.size.h - 20, b.size.w - 4, 18));
}

static void render_summary(GContext *ctx, GRect b) {
  char buf[96];
  uint32_t reps = 0, volume_q = 0, work_s = 0;
  for (uint8_t i = 0; i < s_workout.exercise_count; i++) {
    const PackExercise *e = &s_workout.exercises[i];
    for (uint8_t s = 0; s < e->set_count; s++) {
      work_s += s_work_secs[i][s];
      if (!(e->flags & PACK_FLAG_TIMED)) {
        reps += s_actual[i][s];
        volume_q += (uint32_t)s_actual[i][s] * e->weight_q;
      }
    }
  }
  int dur = (int)(time(NULL) - s_started);

  draw_text(ctx, "Done!", FONT_KEY_GOTHIC_28_BOLD, GRect(2, 2, b.size.w - 4, 32));
  snprintf(buf, sizeof buf, "%d:%02d", dur / 60, dur % 60);
  draw_text(ctx, buf, FONT_HUGE_NUM, GRect(0, 32, b.size.w, 54));
  snprintf(buf, sizeof buf, "%lu reps\nwork %lu:%02lu   vol %lu kg", (unsigned long)reps,
           (unsigned long)(work_s / 60), (unsigned long)(work_s % 60),
           (unsigned long)(volume_q / 4));
  draw_text(ctx, buf, FONT_KEY_GOTHIC_24, GRect(2, 88, b.size.w - 4, 56));
  draw_text(ctx, "select = finish", FONT_KEY_GOTHIC_18, GRect(2, b.size.h - 24, b.size.w - 4, 22));
}

static void layer_update(Layer *layer, GContext *ctx) {
  GRect b = layer_get_bounds(layer);
#ifdef PBL_ROUND
  b = grect_inset(b, GEdgeInsets(6, 14));
#endif
  graphics_context_set_text_color(ctx, GColorBlack);
  switch (s_phase) {
    case PHASE_ACTIVE: render_active(ctx, b); break;
    case PHASE_REST: render_rest(ctx, b); break;
    case PHASE_SUMMARY: render_summary(ctx, b); break;
  }
}

// ---- Buttons ----

// Correct the rep count, during the set or (more usefully) during rest — your
// hands are busy mid-set, so rest is where corrections actually happen.
// Locks the count so the untuned auto-counter can't overwrite the fix, which
// is what previously made the buttons feel dead.
static void adjust(int delta) {
  if (s_phase == PHASE_ACTIVE && !cur_timed() && s_hold_state != HOLD_LEADIN) {
    s_count_locked = true;
    s_counter += delta;
    if (s_counter < 0) s_counter = 0;
    if (s_counter > 250) s_counter = 250;
    redraw();
  } else if (s_phase == PHASE_REST) {
    int v = s_actual[s_cur_ex][s_cur_set] + delta;
    if (v < 0) v = 0;
    if (v > 250) v = 250;
    s_actual[s_cur_ex][s_cur_set] = (uint8_t)v;
    redraw();
  }
}

static void up_click(ClickRecognizerRef ref, void *ctx) { adjust(1); }
static void down_click(ClickRecognizerRef ref, void *ctx) { adjust(-1); }

static void select_click(ClickRecognizerRef ref, void *ctx) {
  switch (s_phase) {
    case PHASE_ACTIVE:
      if (s_hold_state == HOLD_LEADIN) {
        // Skip the get-into-position countdown and start capturing now.
        vibes_double_pulse();
        start_capture();
        redraw();
        break;
      }
      if (cur_timed()) {
        if (s_hold_state == HOLD_IDLE) {
          s_hold_state = HOLD_LEADIN;
          s_leadin = 3;
          vibes_short_pulse();
          schedule_tick();
          redraw();
        } else if (s_hold_state == HOLD_RUNNING) {
          finish_set((uint8_t)(cur_set().target - s_hold_remaining));
        }
      } else {
        finish_set((uint8_t)s_counter);
      }
      break;
    case PHASE_REST:
      // Already in the last-3s countdown -> you're in position, so go straight
      // in; otherwise give the next set its own lead-in.
      advance_after_rest(s_rest_remaining <= 3);
      break;
    case PHASE_SUMMARY:
      window_stack_pop(true);
      break;
  }
}

static void action_performed(ActionMenu *menu, const ActionMenuItem *item, void *context) {
  switch ((int)(intptr_t)action_menu_item_get_action_data(item)) {
    case ACTION_RESUME:
      break;
    case ACTION_SKIP_EXERCISE:
      cancel_timer();
      accel_stop();
      recorder_abort();
      finalize_label();
      s_cur_ex++;
      s_cur_set = 0;
      if (s_cur_ex >= s_workout.exercise_count) {
        enter_summary();
      } else {
        enter_active(true);  // new exercise -> get-into-position lead-in
      }
      break;
    case ACTION_END_WORKOUT:
      enter_summary();
      break;
    case ACTION_DISCARD:
      // Popping here would pop the ActionMenu itself (it's the top window);
      // defer until the menu has fully closed.
      s_discard_on_close = true;
      break;
  }
}

static void menu_did_close(ActionMenu *menu, const ActionMenuItem *item, void *context) {
  if (s_discard_on_close) {
    s_discard_on_close = false;
    window_stack_pop(true);
  }
}

static void back_click(ClickRecognizerRef ref, void *ctx) {
  if (s_phase == PHASE_SUMMARY) {
    window_stack_pop(true);
    return;
  }
  action_menu_open(&(ActionMenuConfig){
      .root_level = s_menu_level,
      .colors = {.background = PBL_IF_COLOR_ELSE(GColorDarkCandyAppleRed, GColorBlack),
                 .foreground = GColorWhite},
      .align = ActionMenuAlignCenter,
      .did_close = menu_did_close,
  });
}

static void click_config(void *ctx) {
  window_single_repeating_click_subscribe(BUTTON_ID_UP, 150, up_click);
  window_single_repeating_click_subscribe(BUTTON_ID_DOWN, 150, down_click);
  window_single_click_subscribe(BUTTON_ID_SELECT, select_click);
  window_single_click_subscribe(BUTTON_ID_BACK, back_click);
}

// ---- Window lifecycle ----

static void window_load(Window *window) {
  Layer *root = window_get_root_layer(window);
  s_layer = layer_create(layer_get_bounds(root));
  layer_set_update_proc(s_layer, layer_update);
  layer_add_child(root, s_layer);

  s_menu_level = action_menu_level_create(4);
  action_menu_level_add_action(s_menu_level, "Resume", action_performed,
                               (void *)ACTION_RESUME);
  action_menu_level_add_action(s_menu_level, "Skip exercise", action_performed,
                               (void *)ACTION_SKIP_EXERCISE);
  action_menu_level_add_action(s_menu_level, "End workout", action_performed,
                               (void *)ACTION_END_WORKOUT);
  action_menu_level_add_action(s_menu_level, "Discard", action_performed,
                               (void *)ACTION_DISCARD);
}

static void window_unload(Window *window) {
  cancel_timer();
  accel_stop();
  recorder_abort();
  finalize_label();  // a discarded session still labels its last staged set
  action_menu_hierarchy_destroy(s_menu_level, NULL, NULL);
  s_menu_level = NULL;
  layer_destroy(s_layer);
  s_layer = NULL;
  window_destroy(s_window);
  s_window = NULL;
}

void session_window_push(const PackedWorkout *workout) {
  s_workout = *workout;
  memset(s_actual, 0, sizeof s_actual);
  memset(s_work_secs, 0, sizeof s_work_secs);
  s_cur_ex = 0;
  s_cur_set = 0;
  s_started = time(NULL);

  s_window = window_create();
  window_set_background_color(s_window, GColorWhite);
  window_set_window_handlers(s_window, (WindowHandlers){
      .load = window_load,
      .unload = window_unload,
  });
  window_set_click_config_provider(s_window, click_config);
  window_stack_push(s_window, true);
  enter_active(true);  // first set -> 3-2-1 get-into-position lead-in
}
