#pragma once
#include "packfmt.h"

// Push the guided-session window. The workout is copied; the caller's copy
// may go out of scope.
void session_window_push(const PackedWorkout *workout);
