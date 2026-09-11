#pragma once
#include <pebble.h>

// Crash breadcrumbs. Pebble can't catch its own hard-fault and offers no remote
// telemetry, so we detect crashes on the NEXT launch: set a persisted breadcrumb
// before a risky section and clear it on clean completion. If a breadcrumb
// survives to the next boot, the previous run crashed there — we report it to
// the server over the sync rail (see strength.c + pkjs).

// Breadcrumb codes (what the app was doing).
#define CRUMB_NONE       0
#define CRUMB_MODEL      1  // learned-counter feature extraction (ctx = movement_id)
#define CRUMB_STAGE      2  // recorder staging a set
#define CRUMB_SYNC       3  // processing a sync message

typedef struct {
  uint8_t code;
  uint32_t ctx;   // code-specific (e.g. movement_id)
  uint32_t heap;  // free heap when the crumb was set
} Crumb;

// Read + clear any breadcrumb left by the previous run. Returns true and fills
// *out if the previous run crashed mid-section. Call once at startup.
bool crumb_check_previous(Crumb *out);

// Mark entry to a risky section (persists) / clear on clean exit.
void crumb_set(uint8_t code, uint32_t ctx);
void crumb_clear(void);
