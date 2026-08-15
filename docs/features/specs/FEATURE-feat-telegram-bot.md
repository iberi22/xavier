# FEATURE: Telegram Bot Integration

**Status:** `stable` | **Score:** 100% | **Last Tested:** 2026-07-18

## Overview
A lightweight Telegram bot integration allowing remote users or operators to issue memory searches, inspect offline model and engine statuses, check health metrics, and receive push notifications from the Xavier event bus.

## Architecture & Design
The bot is implemented with the `teloxide` framework. It supports secure, token-based communication where the Telegram API token is retrieved from the encrypted credential vault. It integrates with the standard rate limiter to prevent denial of service and registers command route handlers for key system queries.

## Implementation Paths
- `src/telegram/` (Teloxide runner, command router, and Telegram client)
- `src/observability/notifier.rs` (notification integration forwarding events to Telegram chats)

## Sub-features
- **Teloxide Bot Runner:** Standard async loop supporting both long-polling and webhooks.
- **Bot Token Protection:** Stores sensitive API tokens securely in the `HardwareVault`/credentials store.
- **Command Router:** Standard commands like `/memory`, `/health`, and `/localstatus`.
- **Telemetry Notifier:** Automated dispatcher sending critical system error alerts or successful sync notices directly to configured chat IDs.

## Test References
- Command routing unit tests.
- Vault secret lookup and encryption checks under the telegram feature flag.

## Known Issues & Notes
- Extra convenience commands are managed in an incremental backlog.
- Relies on the standard fallback completions channel when local provider queries are triggered.
