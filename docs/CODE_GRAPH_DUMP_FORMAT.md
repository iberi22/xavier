# Xavier Code Graph Portable Dump Format

The Xavier Code Graph can be exported to a portable JSON format, typically stored at `.xavier/codegraph.json` within the repository root. This document describes the structure of this dump, which is used by tools like the MCP `get_code_graph` tool.

## Root Object: `CodeGraphDump`

| Field | Type | Description |
|-------|------|-------------|
| `_meta` | `CodeGraphMeta` | Metadata about the scan and repository. |
| `symbols` | `Array<Symbol>` | All code symbols (functions, classes, etc.) discovered. |
| `edges` | `Array<CodeEdge>` | Relationships between symbols (calls, uses, etc.). |
| `hotspots` | `Array<ComplexityHotspot>` | Areas of high complexity or risk in the code. |
| `hubs` | `Array<HubNode>` | Central nodes in the code graph with high connectivity. |

### `CodeGraphMeta`

| Field | Type | Description |
|-------|------|-------------|
| `repo` | `string` | Name of the repository. |
| `scanned_at` | `string` | RFC3339 timestamp of when the scan was performed. |
| `total_files` | `number` | Total number of files scanned. |
| `total_symbols` | `number` | Total number of symbols indexed. |
| `total_edges` | `number` | Total number of edges (relationships) indexed. |
| `version` | `string` | Format version (e.g., "1.0"). |

### `Symbol`

Represents a code entity.

| Field | Type | Description |
|-------|------|-------------|
| `id` | `number \| null` | Internal database ID (optional). |
| `stable_id` | `string \| null` | Deterministic, portable identifier for the symbol. |
| `name` | `string` | Name of the symbol (e.g., "my_function"). |
| `kind` | `string` | Type of symbol. Values: `Function`, `Struct`, `Enum`, `Trait`, `Impl`, `Class`, `Method`, `Variable`, `Constant`, `Import`, `Export`, `Module`, `File`, `Symbol`. |
| `lang` | `string` | Programming language. Values: `Rust`, `TypeScript`, `JavaScript`, `Python`, `Go`, `Java`, `C`, `Cpp`, `Unknown`. |
| `file_path` | `string` | Path to the file containing the symbol. |
| `start_line` | `number` | Line number where the symbol starts (1-indexed). |
| `end_line` | `number` | Line number where the symbol ends. |
| `start_col` | `number` | Column where the symbol starts. |
| `end_col` | `number` | Column where the symbol ends. |
| `signature` | `string \| null` | Code signature of the symbol (e.g., function header). |
| `parent` | `string \| null` | Parent container (e.g., class or struct name). |
| `complexity` | `number \| null` | Cyclomatic or cognitive complexity score. |

### `CodeEdge`

Represents a relationship between two symbols.

| Field | Type | Description |
|-------|------|-------------|
| `id` | `number \| null` | Internal database ID (optional). |
| `from_symbol` | `string` | `stable_id` of the source symbol. |
| `to_symbol` | `string` | `stable_id` of the target symbol. |
| `edge_type` | `string` | Type of relationship. Values: `Calls`, `Defines`, `Uses`, `Imports`, `Contains`, `References`. |
| `file_path` | `string` | Path to the file where the relationship was discovered. |
| `line` | `number` | Line number of the relationship. |
| `confidence` | `number` | Heuristic confidence score (0.0 to 1.0). |
| `metadata` | `object \| null` | Additional relationship-specific metadata. |

### `ComplexityHotspot`

Identifies high-risk areas in the codebase.

| Field | Type | Description |
|-------|------|-------------|
| `symbol` | `Symbol` | The symbol identified as a hotspot. |
| `incoming` | `number` | Number of incoming edges. |
| `outgoing` | `number` | Number of outgoing edges. |
| `risk_score` | `number` | Calculated risk score based on complexity and connectivity. |

### `HubNode`

Identifies central points of connectivity.

| Field | Type | Description |
|-------|------|-------------|
| `symbol` | `Symbol` | The symbol identified as a hub. |
| `incoming` | `number` | Number of incoming edges. |
| `outgoing` | `number` | Number of outgoing edges. |
| `total` | `number` | Total connectivity (`incoming + outgoing`). |
