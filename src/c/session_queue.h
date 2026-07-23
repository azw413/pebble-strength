#pragma once
#include <pebble.h>

// Durable, persist-backed queue of completed set summaries — the offline
// safety net. Every finished set is enqueued at completion; the live accel
// upload drains it on ack, and anything left (performed with the phone away)
// flushes to the phone on the next connection. Summaries only (~38 bytes) —
// raw accel stays best-effort, so a whole gym session survives offline.

#define SQ_MAX 20  // queued sets held durably; fits the per-app persist budget

typedef struct {
  uint32_t client_set_id;  // watch-unique, monotonic — the server idempotency key
  uint32_t performed_at;   // unix seconds (watch RTC, valid offline)
  uint8_t movement_id;
  uint8_t set_index;
  bool timed;
  uint8_t actual;          // reps, or hold seconds
  uint16_t work_secs;
  char workout_name[25];
} SqSet;

void session_queue_init(void);

// A fresh watch-unique monotonic id for the next set (persisted across boots).
uint32_t session_queue_next_id(void);

// Enqueue a finished set. If full, the oldest is dropped to make room.
void session_queue_enqueue(const SqSet *set);

// Remove a set once accepted (accel-upload ack, or server ack after a flush).
void session_queue_ack(uint32_t client_set_id);

uint8_t session_queue_count(void);

// Copy the oldest queued set into *out; returns false if the queue is empty.
bool session_queue_oldest(SqSet *out);
