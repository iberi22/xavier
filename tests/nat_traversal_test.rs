use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;
use tokio::net::UdpSocket;
use xavier::mesh::p2p::nat_traversal::{
    CandidatePairState, HolePunchState, IceCandidate, IceCandidateType, NatType,
    NatTraversalEngine, StunAttribute, StunMessage, StunMessageType,
    TransportProtocol, STUN_MAGIC_COOKIE,
};

#[test]
fn test_stun_binding_request_encode_decode() {
    let transaction_id = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
    let req = StunMessage::create_binding_request(transaction_id);

    let bytes = req.encode();
    assert_eq!(bytes.len(), 20); // 20-byte header, no attributes
    assert_eq!(bytes[0..2], [0x00, 0x01]); // Binding Request type
    assert_eq!(bytes[2..4], [0x00, 0x00]); // Length 0
    assert_eq!(
        u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
        STUN_MAGIC_COOKIE
    );
    assert_eq!(&bytes[8..20], &transaction_id);

    let parsed = StunMessage::parse(&bytes).expect("Failed to parse STUN binding request");
    assert_eq!(parsed.message_type, StunMessageType::BindingRequest);
    assert_eq!(parsed.transaction_id, transaction_id);
    assert!(parsed.attributes.is_empty());
}

#[test]
fn test_stun_xor_mapped_address_ipv4() {
    let transaction_id = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120];
    let expected_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 1)), 3478);

    let resp = StunMessage {
        message_type: StunMessageType::BindingResponse,
        transaction_id,
        attributes: vec![StunAttribute::XorMappedAddress(expected_addr)],
    };

    let encoded = resp.encode();
    let parsed = StunMessage::parse(&encoded).expect("Failed to parse STUN response");

    assert_eq!(parsed.message_type, StunMessageType::BindingResponse);
    assert_eq!(parsed.transaction_id, transaction_id);
    assert_eq!(parsed.get_reflexive_address(), Some(expected_addr));
}

#[test]
fn test_stun_xor_mapped_address_ipv6() {
    let transaction_id = [1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6];
    let expected_v6 = Ipv6Addr::new(0x2001, 0xdb8, 0x85a3, 0, 0, 0x8a2e, 0x0370, 0x7334);
    let expected_addr = SocketAddr::new(IpAddr::V6(expected_v6), 54321);

    let resp = StunMessage {
        message_type: StunMessageType::BindingResponse,
        transaction_id,
        attributes: vec![StunAttribute::XorMappedAddress(expected_addr)],
    };

    let encoded = resp.encode();
    let parsed = StunMessage::parse(&encoded).expect("Failed to parse IPv6 STUN response");

    assert_eq!(parsed.get_reflexive_address(), Some(expected_addr));
}

#[test]
fn test_ice_candidate_priority_ordering() {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);

    let host = IceCandidate::new("c1", 1, TransportProtocol::Udp, addr, IceCandidateType::Host, None);
    let prflx = IceCandidate::new(
        "c2",
        1,
        TransportProtocol::Udp,
        addr,
        IceCandidateType::PeerReflexive,
        None,
    );
    let srflx = IceCandidate::new(
        "c3",
        1,
        TransportProtocol::Udp,
        addr,
        IceCandidateType::ServerReflexive,
        None,
    );
    let relay = IceCandidate::new("c4", 1, TransportProtocol::Udp, addr, IceCandidateType::Relayed, None);

    assert!(host.priority > prflx.priority);
    assert!(prflx.priority > srflx.priority);
    assert!(srflx.priority > relay.priority);
}

#[test]
fn test_candidate_pair_formation_and_ranking() {
    let local_addr1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)), 5000);
    let remote_addr1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20)), 5000);
    let remote_srflx = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 5)), 40000);

    let mut engine = NatTraversalEngine::new(vec![], vec![]);
    engine.add_local_address(local_addr1);
    engine.gather_host_candidates();

    let remote_host = IceCandidate::new(
        "r1",
        1,
        TransportProtocol::Udp,
        remote_addr1,
        IceCandidateType::Host,
        None,
    );
    let remote_reflexive = IceCandidate::new(
        "r2",
        1,
        TransportProtocol::Udp,
        remote_srflx,
        IceCandidateType::ServerReflexive,
        None,
    );

    engine.add_remote_candidate(remote_host);
    engine.add_remote_candidate(remote_reflexive);

    let pairs = engine.form_candidate_pairs(true);
    assert_eq!(pairs.len(), 2);

    // First pair should be Host-Host (highest priority)
    assert_eq!(pairs[0].remote.candidate_type, IceCandidateType::Host);
    assert_eq!(
        pairs[1].remote.candidate_type,
        IceCandidateType::ServerReflexive
    );

    let best = engine.select_best_pair().expect("Should select best pair");
    assert_eq!(best.remote.addr, remote_addr1);
}

#[test]
fn test_nat_type_determination() {
    let local_port = 5555;

    let public_ip = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 10)), 5555);
    let nat_a = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 10)), 40001);
    let nat_b = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 10)), 40002);

    assert_eq!(
        NatTraversalEngine::determine_nat_type_from_responses(local_port, &[public_ip]),
        NatType::OpenInternet
    );

    assert_eq!(
        NatTraversalEngine::determine_nat_type_from_responses(local_port, &[nat_a, nat_a]),
        NatType::FullCone
    );

    assert_eq!(
        NatTraversalEngine::determine_nat_type_from_responses(local_port, &[nat_a, nat_b]),
        NatType::Symmetric
    );
}

#[tokio::test]
async fn test_stun_discovery_async() {
    // Spin up mock STUN server
    let stun_server_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let stun_server_addr = stun_server_socket.local_addr().unwrap();

    // Background server loop: respond to STUN Binding Requests
    let server_handle = tokio::spawn(async move {
        let mut buf = [0u8; 1024];
        if let Ok((len, src)) = stun_server_socket.recv_from(&mut buf).await {
            if let Ok(req) = StunMessage::parse(&buf[..len]) {
                let resp = StunMessage {
                    message_type: StunMessageType::BindingResponse,
                    transaction_id: req.transaction_id,
                    attributes: vec![StunAttribute::XorMappedAddress(src)],
                };
                let _ = stun_server_socket.send_to(&resp.encode(), src).await;
            }
        }
    });

    // Client setup
    let client_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let mut engine = NatTraversalEngine::new(vec![stun_server_addr], vec![]);

    let candidate = engine
        .discover_server_reflexive_candidate(
            &client_socket,
            stun_server_addr,
            Duration::from_secs(2),
        )
        .await
        .expect("STUN discovery should succeed");

    assert_eq!(candidate.candidate_type, IceCandidateType::ServerReflexive);
    assert_eq!(candidate.addr, client_socket.local_addr().unwrap());

    let _ = server_handle.await;
}

#[tokio::test]
async fn test_hole_punching_state_machine() {
    let p1_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let p2_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();

    let p1_addr = p1_socket.local_addr().unwrap();
    let p2_addr = p2_socket.local_addr().unwrap();

    let p1_cand = IceCandidate::new("p1", 1, TransportProtocol::Udp, p1_addr, IceCandidateType::Host, None);
    let p2_cand = IceCandidate::new("p2", 1, TransportProtocol::Udp, p2_addr, IceCandidateType::Host, None);

    let pair = xavier::mesh::p2p::nat_traversal::CandidatePair::new(p1_cand, p2_cand, true);

    // Responder side (P2)
    let responder = tokio::spawn(async move {
        let mut buf = [0u8; 1024];
        if let Ok((len, src)) = p2_socket.recv_from(&mut buf).await {
            if let Ok(msg) = StunMessage::parse(&buf[..len]) {
                let ack = StunMessage {
                    message_type: StunMessageType::BindingResponse,
                    transaction_id: msg.transaction_id,
                    attributes: vec![],
                };
                let _ = p2_socket.send_to(&ack.encode(), src).await;
            }
        }
    });

    // Requester side (P1)
    let mut engine = NatTraversalEngine::new(vec![], vec![]);

    let result = engine
        .perform_hole_punch(&p1_socket, &pair, 3, Duration::from_millis(100))
        .await
        .expect("Hole punch should succeed");

    assert_eq!(result.state, CandidatePairState::Succeeded);
    if let HolePunchState::Connected { active_pair } = engine.state() {
        assert_eq!(active_pair.remote.addr, p2_addr);
    } else {
        panic!("Expected HolePunchState::Connected state");
    }

    let _ = responder.await;
}
