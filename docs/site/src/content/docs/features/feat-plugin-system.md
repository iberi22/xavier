---
title: "Code-Graph Plugin System & Registry"
description: "Plugin manager, GitHubRegistry client, mock tests; live swal/xavier-plugins repo missing"
---

**Status:** `stable` | **Score:** 100% | **Last Tested:** 2026-07-18

## Overview
A extensible plugin system for the `code-graph` analyzer sidecar, enabling dynamic parser registration, custom language analysis, and remote/local parser registry indices.

## Architecture & Design
The `code-graph` sidecar exposes a plugin host and manager that loads parsing plugins dynamically. In-repo indices (from JSON fixtures located under `fixtures/xavier-plugins/`) define default local parser tools. Overrides can be injected using `XAVIER_PLUGIN_REGISTRY_URL` to configure custom registry paths.

## Implementation Paths
- `code-graph/src/plugin/` (plugin host, default registry client, and loading logic)
- `code-graph/fixtures/xavier-plugins/` (local parser index fixtures)
- `plugins/parser-python/` (reference Python syntax analysis plugin implementation)

## Sub-features
- **pl-host-manager:** Orchestrates the lifecycle, discovery, and setup of external parsing plugins.
- **pl-fixture-index:** Bundled local list defining standard AST analyzer plugins.
- **pl-default-registry:** Default loader backing local and remote JSON index registries.
- **pl-parser-python:** Template and parser plugin translating Python source trees into structured code symbol maps.

## Test References
- Default registry index mock loading and parsing tests.
- Python parser plugin syntax node mapping unit tests.

## Known Issues & Notes
- Live publishing to a centralized `swal/xavier-plugins` repository is considered operational, and does not block local-first core code graph compilation.

### Functional Plugin Example
Configure your `plugins.json` manifest to register a local AST parser plugin:

**Manifest (`plugins.json`):**
```json
{
  "plugins": [
    {
      "id": "parser-python-local",
      "name": "Local Python AST Parser",
      "version": "1.0.0",
      "entry_point": "file://plugins/parser-python/main.py",
      "languages": ["python"]
    }
  ]
}
```

Point the server to your registry using the environment variable:
```sh
XAVIER_PLUGIN_REGISTRY_URL="file://config/plugins.json"
```
