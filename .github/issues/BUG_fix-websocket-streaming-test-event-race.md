---
title: "BUG: Fix Websocket Streaming Test Event Race"
labels:
  - bug
  - test
assignees: ["jules"]
protocol_version: 1.3.0
---

## Descripción

The test suite in `tests/websocket_events.rs` has a regression in `test_websocket_streaming`.

During execution, the test panics at line 220:
```
thread 'test_websocket_streaming' panicked at tests/websocket_events.rs:220:9:
assertion `left == right` failed
  left: "timeline_event"
 right: "memory.add"
```

### Root Cause
1. When `store.put(record)` is called inside the handler simulation, the inner store automatically broadcasts a `"timeline_event"` to the configured `event_tx` (configured via `store_inner.set_event_tx(event_tx.clone())`).
2. Immediately after, the test route handler manually broadcasts the `"memory.add"` event over the same `event_tx`.
3. The WebSocket stream receiver in the test consumes the first event from the channel. Because of the timing, the automatically broadcasted `"timeline_event"` arrives first, and the test's strict assertion `assert_eq!(evt.event_type, "memory.add")` fails.

---

## Criterios de Aceptación

- [ ] Modify `tests/websocket_events.rs` to handle both events correctly (e.g., consume both messages or assert that the stream eventually receives `"memory.add"` rather than failing instantly on the first message).
- [ ] Ensure that running `CARGO_TARGET_DIR=target_local cargo test --test websocket_events` succeeds consistently without flakiness.
- [ ] No regressions introduced in production WebSocket event streams.

---

## Contexto Adicional

- File location: `tests/websocket_events.rs`
- You can compile and test locally using:
  ```bash
  CARGO_TARGET_DIR=target_local cargo test --test websocket_events
  ```
- Target assignee: `@jules`
