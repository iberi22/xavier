---
title: "Notification Persistence and Delivery"
description: "Persistent notification system with event bus, SQLite storage, REST API, and Tauri real-time updates"
---

**Status:** `stable` | **Score:** 100% | **Last Tested:** 2026-07-18

## Overview
A centralized notification system and event bus to track, persist, and dispatch critical events across Xavier. Notifications are organized into distinct category islands to facilitate filtering and streaming to frontend clients (Tauri) and external chat interfaces (Telegram).

## Architecture & Design
The system registers events through an async channel/event bus. Every incoming notification is typed and directed to appropriate output channels. This structure allows real-time UI synchronization via Tauri events and decoupled delivery pipelines that do not block hot request paths.

## Implementation Paths
- `src/observability/notifier.rs` (main broadcast loop, category structures, and downstream sinks)

## Sub-features
- **Notification Structs & Event Bus:** Dynamic payload containers defining severity and classification.
- **Categorization Islands:** System, Memory, Agents, and Errors islands.
- **Tauri Event Integration:** Real-time push capability using Tauri's `emit_all` to immediately refresh the dashboard UI.
- **Persistence Foundation:** Schema and structures prepared for historical local storage.

## Test References
- Event dispatch and subscriber delivery tests.
- Category routing logic verification.

## Known Issues & Notes
- Advanced SQLite history exploration UI is prioritized as a post-1.0 enhancement, as the reactive broadcast notifier completely satisfies MVP needs.

### Functional Notification API Example
Query system and agent notifications from the persistent event bus:

```bash
# List all active system notifications
curl -H "X-Xavier-Token: $XAVIER_TOKEN" \
  "http://localhost:8006/panel/api/notifications?category=System"
```

Response format:
```json
[
  {
    "id": "notif_01J2F9Y8",
    "title": "Low GPU VRAM",
    "message": "Available VRAM fell below 1GB during consolidation.",
    "category": "System",
    "timestamp": 1721516400
  }
]
```
