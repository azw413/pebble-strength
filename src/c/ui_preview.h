#pragma once
#include "packfmt.h"

// Push the workout-preview window (exercise list + Start).
// `workout` must stay valid while the window is open.
void preview_window_push(const PackedWorkout *workout);
