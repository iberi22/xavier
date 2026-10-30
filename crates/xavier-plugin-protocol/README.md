# xavier-plugin-protocol

Versioned contract and handshake definitions for Xavier plugins (sidecars).

## Protocol Overview
The protocol uses a shared envelope `Frame` for all communication between the core and sidecars:
```json
{
  "protocol_version": "xavier-core/1",
  "message_id": "uuid-v4",
  "kind": "codegraph.parse.request | mesh.sync.offer | ...",
  "payload": {}
}
```

## Versioning Rule
`protocol_version` uses major compatibility: a sidecar with a higher major than the core MUST be rejected with a `VersionSkew` error; a higher minor is accepted with unknown-field tolerance.

See [ADR-019](../../docs/adr/ADR-019-plugin-first-boundary.md) for full context.

## TODO
- Part B: framing + C1/C2 payloads.
