# C4: libp2p peer discovery (10% → 35%)

## Problem

libp2p transport is at 10%. No peer discovery mechanism exists. Nodes
cannot find each other without manual configuration.

## Solution

Implement mDNS + Kademlia DHT for automatic peer discovery.

### Discovery flow

1. Node starts → registers mDNS service on local network
2. mDNS discovers local peers automatically
3. Bootstrap nodes join Kademlia DHT for WAN discovery
4. Peer list maintained in-memory with health checks

### Steps

1. Add mDNS discovery in `src/mesh/transport/libp2p/discovery.rs`
2. Implement `DiscoveryBehaviour` combining mDNS + Kademlia
3. Add bootstrap node configuration (hardcoded + config file)
4. Implement `PeerHealth` check (ping every 30s, mark unhealthy after 3 misses)
5. Wire discovery events to existing mesh peer management
6. Add integration test: 2 nodes discover each other via mDNS

## Acceptance

- [ ] Two local nodes discover each other via mDNS
- [ ] Node connects to bootstrap node and joins DHT
- [ ] Peer health checks run every 30s
- [ ] Unhealthy peers removed from peer list after 3 missed pings
- [ ] Existing libp2p tests still pass

## Files

- `src/mesh/transport/libp2p/discovery.rs` (new)
- `src/mesh/transport/libp2p/mod.rs` (modify)
- `src/mesh/peer.rs` (modify)
