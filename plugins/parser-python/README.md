# parser-python

A high-performance Python parser plugin template for `code-graph`.

This plugin implements the stdin/stdout JSON parsing protocol for extracting symbols (such as imports, classes, functions, and methods) from Python files, along with position information and cyclomatic complexity estimates.

## Features

- **Tree-sitter Compatible**: Ready to use `tree-sitter` and `tree_sitter_python` if they are installed in the Python environment.
- **Robust Built-in Fallback**: Falls back automatically to Python's built-in `ast` module if `tree-sitter` is unavailable. This requires zero external dependencies and guarantees real-time parsing out-of-the-box.
- **Rich Symbols Extraction**: Extracts imports, classes, functions, and class methods.
- **Complexity Assessment**: Computes cyclomatic complexity for functions and methods using decision-point AST analysis.

## Usage

### 🟢 1. Health Operation

To execute a health check against the plugin, run:

```bash
python3 plugins/parser-python/plugin.py --health
# or
python3 plugins/parser-python/plugin.py health
```

**Expected Output:**
```text
Success
```

### 📂 2. Parse Operation

To parse a batch of source files, stream a JSON request into the plugin's standard input:

```bash
python3 plugins/parser-python/plugin.py < plugins/parser-python/fixtures/request.json
```

**Expected Output:**
```json
{
  "symbols": [
    {
      "id": null,
      "stable_id": null,
      "name": "math",
      "kind": "Import",
      "lang": "Python",
      "file_path": "example.py",
      "start_line": 1,
      "end_line": 1,
      "start_col": 0,
      "end_col": 11,
      "signature": "import math",
      "parent": null,
      "complexity": null
    },
    ...
  ],
  "error": null
}
```

---

## JSON Schema Details

### Request Protocol (stdin)

The plugin expects a JSON object matching the `PluginRequest` Rust structure:

```json
{
  "language": "Python",
  "files": [
    {
      "path": "path/to/file.py",
      "source": "def hello():\n    pass\n"
    }
  ]
}
```

### Response Protocol (stdout)

The plugin returns a JSON object matching the `PluginResponse` Rust structure:

```json
{
  "symbols": [
    {
      "id": null,
      "stable_id": null,
      "name": "hello",
      "kind": "Function",
      "lang": "Python",
      "file_path": "path/to/file.py",
      "start_line": 1,
      "end_line": 2,
      "start_col": 0,
      "end_col": 8,
      "signature": "def hello(...)",
      "parent": null,
      "complexity": 1.0
    }
  ],
  "error": null
}
```
