# Issue: Sovereign Mesh Service-Area Telemetry Collector

## Context
The external network handles governance and telemetry. We need a specialized collector that only gathers non-private, service-area metrics.

## Tasks
1. [ ] Implement `ServiceAreaCollector` in `src/mesh/telemetry_collector.rs`.
2. [ ] Define sanitized metric schemas (CPU, Memory, Node Uptime, Sync Latency).
3. [ ] Integrate with `DataSanitizer` to ensure NO private memory (chunks, queries) is included.
4. [ ] Implement a periodic "Offer" broadcast to external peers via `MeshTransport`.
5. [ ] Add XP reward triggering for successful telemetry contributions.

## References
- `docs/XAVIER_DATA_COMMONS_ARCHITECTURE.md`
- `src/mesh/data_sanitizer.rs`
