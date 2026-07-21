# adapters Module SRC

## Overview

Documentation for the `adapters` module.

## Dependencies

This module depends on the following modules:
- [agents](agents_SRC.md)
- [coordination](coordination_SRC.md)
- [domain](domain_SRC.md)
- [memory](memory_SRC.md)
- [ports](ports_SRC.md)
- [security](security_SRC.md)
- [session](session_SRC.md)
- [settings](settings_SRC.md)
- [tasks](tasks_SRC.md)
- [time](time_SRC.md)
- [verification](verification_SRC.md)

## Components

- `adapters/mod.rs`
- `adapters/inbound/mod.rs`
- `adapters/inbound/http/state.rs`
- `adapters/inbound/http/dto.rs`
- `adapters/inbound/http/routes.rs`
- `adapters/inbound/http/mod.rs`
- `adapters/inbound/http/time_metrics_adapter.rs`
- `adapters/inbound/http/handlers/agent.rs`
- `adapters/inbound/http/handlers/sync.rs`
- `adapters/inbound/http/handlers/memory.rs`
- `adapters/inbound/http/handlers/security.rs`
- `adapters/inbound/http/handlers/code.rs`
- `adapters/inbound/http/handlers/mod.rs`
- `adapters/inbound/http/plugins/pgheart.rs`
- `adapters/inbound/http/plugins/mod.rs`
- `adapters/outbound/http_health_adapter.rs`
- `adapters/outbound/mod.rs`
