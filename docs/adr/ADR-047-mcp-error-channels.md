# ADR-047 — MCP error channels by phase

| Campo | Valor |
|-------|--------|
| **ID** | ADR-047 |
| **Estado** | Aceptado (Brahyan, 2026-10-23) |
| **Fecha** | 2026-10-23 |
| **Autores** | Xavier architecture contributors |
| **Relacionados** | `docs/MCP_CONTRACT.md` (Error Handling and Codes); ADR-046 (format reference) |

## Context

Xavier's JSON-RPC MCP endpoint reports a failed `tools/call` on two channels:

- a JSON-RPC `error` object, or
- a successful JSON-RPC `result` whose payload has `isError: true` (a tool-level failure).

Before #3496 the channel was chosen by the code path that raised the failure, not by the kind of failure. The result is that clients see some failures the same way and others differently:

- A missing required argument is returned as a tool result with `isError: true`.
- A wrong-type or invalid-value argument is returned as a JSON-RPC error with code -32001.
- A memory or fragment that does not exist is returned as a JSON-RPC error with code -32002.

The first split happens by accident of implementation. Clients cannot branch on a stable error category either: the numeric code is the only identifier, and two different failures can share one code.

Constraints:

- Protocol-level problems (malformed envelope, unknown method, unknown tool) must remain JSON-RPC errors.
- Existing numeric codes must not be renumbered. Clients already match on them.
- Authorization and internal failures must not start leaking their messages through a new channel without a separate review.

## Options considered

| Option | Description | Cost / complexity |
|--------|-------------|-------------------|
| A. Everything raised after dispatch becomes a tool result | Every handler error is returned with `isError: true`. | Low to implement. Moves authorization and internal failures onto the result channel and exposes internal messages without review. Rejected. |
| B. Channel chosen by negotiated protocol revision | Different channel rules per advertised revision. | High: doubles the test matrix for no client benefit. Rejected. |
| C. Channel chosen by phase, with a stable `kind` field | Framing, routing, envelope and pre-dispatch authorization stay JSON-RPC. Argument validation, absent entities and content-policy blocks become tool results. Each error carries `kind`. | Medium: one shape change and one test matrix. Chosen. |
| D. Renumber codes or add a code for rate limits | New codes for each category. | Churn for existing clients. Rejected; the `kind` field carries the category instead. |

## Decision

1. **Channel by phase.** The channel is decided by where the failure happens:

   | Phase | Examples | Channel | Code | Kind |
   |-------|----------|---------|------|------|
   | Framing | parse error, invalid request shape | JSON-RPC error | -32700 / -32600 | — |
   | Routing | unknown method | JSON-RPC error | -32601 | — |
   | `tools/call` envelope | params not an object, missing `name`, unknown tool, `arguments` not an object | JSON-RPC error | -32602 / -32001 | — |
   | Pre-dispatch authorization | anonymous caller, role check, local identity | JSON-RPC error | -32000 | — |
   | Argument validation | missing required argument, wrong type, invalid value | tool result, `isError: true` | -32001 | `validation` |
   | Entity absent | memory or fragment not found | tool result, `isError: true` | -32002 | `not_found` |
   | Content security policy | prompt-injection block | tool result, `isError: true` | -32000 | `security_policy` |
   | Other execution failures (clearance inside a tool, rate limit, internal) | — | JSON-RPC error | -32000 / -32603 | — |

   The `arguments`-not-an-object case keeps code -32001 on the JSON-RPC channel. No code is renumbered.

2. **Tool-result error shape.** For the rows that use a tool result:

   - `structuredContent.error` is an object with `code` (integer), `kind` (one of `validation`, `not_found`, `security_policy`) and `message` (string).
   - `content[0].text` equals `message`.
   - The result has no top-level `error` member.

3. **Unchanged in this decision.** Pre-dispatch authorization, authorization failures raised inside a tool, rate limits and internal errors keep their current channel. A later change may revisit them; that is not covered here.

4. **Scope of the implementation change.** Routing of the rows above, the `kind` field, typed error construction in the MCP server, and the tests that pin the probe matrix over HTTP and stdio. Handlers that build their own tool-result errors with a string-valued `structuredContent.error` are not normalised by this ADR.

## Simulation

Not run. The template requires a multi-scenario simulation before acceptance; the owner waived it when accepting this ADR (2026-10-23). The decision is a protocol-channel rule, and its behaviour is pinned by the probe-matrix tests added in the implementation change (#3496) instead.

## Assumptions

| Parameter | Value used | Source |
|-----------|------------|--------|
| Missing-required arguments already return tool results | true at HEAD | existing missing-argument tests in `src/server/mcp/tests.rs` |
| Known clients read `isError` on tool results | assumed; not verified for every client | ASSUMPTION — accepted by the owner on 2026-10-23 without a per-client check |
| Renumbering codes is unnecessary | `kind` carries the category | Decision 1 |

## Consequences

- **Positive:**
  - One rule decides the channel for every failure class.
  - Clients can branch on a stable `kind` without parsing messages.
  - Protocol-level errors keep their current behaviour.
- **Negative / cost:**
  - Clients that treat only a top-level `error` as failure will read not-found and validation failures as successful results. The numeric code remains available in `structuredContent.error.code`.
  - The not-found test that pins the current shape changes, and the change is named in the implementation PR.
  - Handler-built tool-result errors with a string-valued `structuredContent.error` stay non-uniform until a later change.
- **What would invalidate this decision:**
  - A client in use that cannot read `isError` on tool results.
  - A revision of the protocol that requires argument-validation errors on the JSON-RPC channel, if the project must comply with it.

## Verification

- The implementation change adds a probe matrix over the HTTP and stdio transports. Each row checks channel, code and `kind`, and the two transports must return the same result payload.
- Alias pairs (for example the two names of the same memory lookup) must return the same code and `kind`.
- A unit test checks that a typed error wins over message-based classification.
- The implementation landed in #3496. The owner accepted this ADR on 2026-10-23.
