# Issue: Create `swal/xavier-plugins` Registry Repository

## Labels
`infrastructure`, `plugin-system`, `P5-repo`

## Description
Create the GitHub repository `swal/xavier-plugins` that will serve as the official plugin registry for Xavier's code-graph plugin system. This repo hosts the `plugins.json` manifest and GitHub Releases with plugin binaries.

## Repository Structure

```
swal/xavier-plugins/
├── plugins.json                    # Registry index (canonical source)
├── plugins.schema.json             # JSON Schema for validation
├── CONTRIBUTING.md                 # Guide for plugin authors
├── README.md                       # Overview and quick start
├── LICENSE                         # MIT or Apache-2.0
│
├── templates/                      # Plugin SDK templates
│   ├── plugin-python/
│   │   ├── plugin.py               # Python parser template
│   │   ├── Cargo.toml              # (if Rust wasm-plugin)
│   │   └── README.md
│   └── plugin-rust/
│       ├── src/lib.rs
│       ├── Cargo.toml
│       └── README.md
│
└── plugins/                        # Development plugins
    ├── parser-python/
    ├── parser-typescript/
    ├── parser-go/
    ├── parser-java/
    ├── parser-c/
    ├── parser-cpp/
    └── parser-rust/
```

## Initial plugins.json

```json
{
  "registry_version": 1,
  "updated_at": "2026-07-13T12:00:00Z",
  "min_engine_version": "0.12.0",
  "plugins": [
    {
      "name": "parser-python",
      "display_name": "Python Parser",
      "description": "Tree-sitter based Python parser plugin",
      "version": "0.1.0",
      "author": "xavier-contributors",
      "languages": ["Python"],
      "capabilities": ["parse"],
      "platform": {
        "linux": {
          "url": "https://github.com/swal/xavier-plugins/releases/download/parser-python-v0.1.0/parser-python-x86_64-linux.tar.gz",
          "checksum": "PLACEHOLDER",
          "format": "tar.gz"
        }
      },
      "min_engine_version": "0.12.0",
      "license": "MIT"
    }
  ]
}
```

## First Plugin: Python Parser
Priority: HIGH — Python is the most commonly indexed language after Rust.

The Python parser plugin:
1. Reads PluginRequest from stdin (JSON)
2. Uses tree-sitter-python grammar to extract symbols
3. Writes PluginResponse to stdout (JSON)
4. Supports operations: parse, health, capabilities

## Definition of Done
- [ ] `swal/xavier-plugins` repository created
- [ ] `plugins.json` with schema validation
- [ ] First Python parser plugin built and released as GitHub Release
- [ ] SHA-256 checksums for all release artifacts
- [ ] `CONTRIBUTING.md` with plugin authoring guide
- [ ] Plugin templates for Python and Rust
- [ ] Integration test: Xavier can download and use the Python parser plugin
