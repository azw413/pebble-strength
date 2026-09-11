#include "crumb.h"

#define PERSIST_CRUMB 80

// Live breadcrumb: {code, ctx, heap}. Present (code != CRUMB_NONE) means we're
// inside a risky section; if it survives to the next boot, that section crashed.
static void write_crumb(uint8_t code, uint32_t ctx, uint32_t heap) {
  uint32_t rec[2];
  rec[0] = ((uint32_t)code << 24);  // code in top byte
  rec[1] = ctx;
  // pack heap into the low 24 bits of rec[0]
  rec[0] |= (heap & 0x00FFFFFF);
  persist_write_data(PERSIST_CRUMB, rec, sizeof rec);
}

bool crumb_check_previous(Crumb *out) {
  if (!persist_exists(PERSIST_CRUMB)) return false;
  uint32_t rec[2] = {0, 0};
  persist_read_data(PERSIST_CRUMB, rec, sizeof rec);
  uint8_t code = (uint8_t)(rec[0] >> 24);
  // Clear immediately so we report a given crash only once.
  persist_delete(PERSIST_CRUMB);
  if (code == CRUMB_NONE) return false;
  if (out) {
    out->code = code;
    out->ctx = rec[1];
    out->heap = rec[0] & 0x00FFFFFF;
  }
  return true;
}

void crumb_set(uint8_t code, uint32_t ctx) {
  write_crumb(code, ctx, (uint32_t)heap_bytes_free());
}

void crumb_clear(void) {
  if (persist_exists(PERSIST_CRUMB)) persist_delete(PERSIST_CRUMB);
}
