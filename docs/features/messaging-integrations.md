# Xavier Messaging Integrations

## Overview

Xavier supports bidirectional communication via external messaging platforms. This enables:
- Receiving commands via Telegram/Discord/Slack
- Sending memory indexing notifications
- Agent activity alerts
- System health updates

## Supported Platforms

| Platform   | Status       | Direction  | Notes |
|------------|-------------|------------|-------|
| Telegram   | 🔜 Planned  | Send + Recv | Bot API via token |
| Discord    | 🔜 Planned  | Send only  | Webhook + optional bot |
| Slack      | 🔜 Planned  | Send only  | Bot OAuth token |
| MS Teams   | 🔜 Planned  | Send only  | Incoming webhook |
| WhatsApp   | 🔜 Planned  | Send only  | Meta Business API |

## Architecture (Target)

```
Xavier Core (Rust)
    │
    ├── messaging-gateway/
    │       ├── telegram_handler.rs    # Bot polling / webhook receiver
    │       ├── discord_handler.rs     # Webhook sender
    │       ├── slack_handler.rs       # Bot API sender
    │       ├── teams_handler.rs       # Incoming webhook sender
    │       └── whatsapp_handler.rs   # Meta API sender
    │
    └── notification-bus/
            ├── event_emitter.rs      # Internal event system
            └── delivery.rs           # Routes events to configured channels
```

## UI Components

The messaging configuration UI lives in `panel-ui/src/components/`:

- `MessagingConfigModal.tsx` — Full-screen modal triggered from TopStatusBar icons
- `MessagingConfigInner` (exported) — Embedded version inside ConfigModal → Messaging tab

Each platform has 3 sub-sections:
1. **Credentials** — Token/webhook/chat ID fields
2. **Permissions** — Granular control of what Xavier can send/receive
3. **Advanced** — Rate limits, prefixes, retry config

## Configuration Storage (Target)

Credentials will be stored in the local Xavier config file (`~/.xavier/config.toml`) using AES-256 encryption backed by the platform keychain (TPM on Windows, Keychain on macOS, libsecret on Linux).

```toml
[messaging.telegram]
bot_token_encrypted = "<encrypted>"
chat_id = "-1001234567890"
enabled = true
permissions = { receive_messages = true, send_agent_alerts = true }
```

## Pending Backend Issues

See GitHub issues:
- `[backend] Telegram bot integration` — #TBD
- `[backend] Discord webhook sender` — #TBD
- `[backend] Slack Bot API integration` — #TBD
- `[backend] MS Teams webhook` — #TBD
- `[backend] WhatsApp Business API` — #TBD
- `[backend] Notification persistence system` — #TBD

## Commands (Telegram, future)

Xavier will support a command interface via Telegram:
```
/memory search <query>     — Search Xavier memories
/agent status              — List active agents
/health                    — System health check
/help                      — List available commands
```

---
_Last updated: 2026-06-10 | Status: UI Mock complete, backend pending_
