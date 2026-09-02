//! Integration tests for Espacio SpaceManager, ChannelManager, InviteManager, and Permissions.
//!
//! Covers:
//! - SpaceManager creation, directory isolation, namespaces, and lifecycle.
//! - ChannelManager append-only log, incremental list_since, sequence monotonicity, cross-space isolation, and CRDT merge.
//! - InviteManager create, canonical payload structure, signature attachment, validation, revocation, and expiry.
//! - Role-based permissions checks (can) end-to-end.

use chrono::{Duration as ChronoDuration, Utc};
use tempfile::tempdir;

use xavier::espacio::{
    can, ChannelManager, ChannelMessage, InviteManager, SpaceAction, SpaceManager, SpaceMembership,
    SpaceRole,
};

#[tokio::test]
async fn test_channel_manager_full_lifecycle_and_merge() {
    let mgr = ChannelManager::new();
    let space_id = "esp_channel_test_01";

    // 1. Post initial messages to Space
    let m0 = mgr
        .post(space_id.into(), "xv1_alice".into(), "Hello Space".into())
        .await;
    assert_eq!(m0.seq, 0);
    assert_eq!(m0.space_id, space_id);
    assert_eq!(m0.author, "xv1_alice");
    assert_eq!(m0.content, "Hello Space");

    let m1 = mgr
        .post(space_id.into(), "xv1_bob".into(), "Hi Alice".into())
        .await;
    assert_eq!(m1.seq, 1);

    let m2 = mgr
        .post(space_id.into(), "xv1_charlie".into(), "Welcome".into())
        .await;
    assert_eq!(m2.seq, 2);

    assert_eq!(mgr.len(space_id).await, 3);

    // 2. Query list_all and list_since incremental
    let all_msgs = mgr.list_all(space_id).await;
    assert_eq!(all_msgs.len(), 3);
    assert_eq!(all_msgs[0].seq, 0);
    assert_eq!(all_msgs[1].seq, 1);
    assert_eq!(all_msgs[2].seq, 2);

    // list_since(0) should return seq 1 and 2
    let since0 = mgr.list_since(space_id, 0).await;
    assert_eq!(since0.len(), 2);
    assert_eq!(since0[0].seq, 1);
    assert_eq!(since0[1].seq, 2);

    // list_since(1) should return seq 2
    let since1 = mgr.list_since(space_id, 1).await;
    assert_eq!(since1.len(), 1);
    assert_eq!(since1[0].seq, 2);

    // list_since boundary (since_seq == max_seq) should return empty
    let since2 = mgr.list_since(space_id, 2).await;
    assert!(since2.is_empty());

    // 3. CRDT merge from a simulated second manager / remote peer
    let remote_messages = vec![
        // Duplicate seq 1 (should be ignored by merge dedup logic)
        ChannelMessage {
            seq: 1,
            space_id: space_id.into(),
            author: "xv1_bob".into(),
            content: "Hi Alice".into(),
            created_at: Utc::now(),
        },
        // Remote seq 3
        ChannelMessage {
            seq: 3,
            space_id: space_id.into(),
            author: "xv1_dave".into(),
            content: "Merged remote message 3".into(),
            created_at: Utc::now(),
        },
        // Remote seq 4
        ChannelMessage {
            seq: 4,
            space_id: space_id.into(),
            author: "xv1_eve".into(),
            content: "Merged remote message 4".into(),
            created_at: Utc::now(),
        },
    ];

    mgr.merge(space_id.into(), remote_messages).await;

    // Length should now be 5 (0, 1, 2 local + 3, 4 remote)
    assert_eq!(mgr.len(space_id).await, 5);

    let updated_all = mgr.list_all(space_id).await;
    assert_eq!(updated_all.len(), 5);
    assert_eq!(updated_all[3].seq, 3);
    assert_eq!(updated_all[3].content, "Merged remote message 3");
    assert_eq!(updated_all[4].seq, 4);
    assert_eq!(updated_all[4].content, "Merged remote message 4");
}

#[tokio::test]
async fn test_channel_monotonic_sequence_and_cross_space_isolation() {
    let mgr = ChannelManager::new();
    let space_a = "esp_space_alpha";
    let space_b = "esp_space_beta";

    // Space A postings
    let a_m0 = mgr
        .post(space_a.into(), "xv1_node1".into(), "Alpha Msg 0".into())
        .await;
    let a_m1 = mgr
        .post(space_a.into(), "xv1_node1".into(), "Alpha Msg 1".into())
        .await;

    // Space B postings
    let b_m0 = mgr
        .post(space_b.into(), "xv1_node2".into(), "Beta Msg 0".into())
        .await;
    let b_m1 = mgr
        .post(space_b.into(), "xv1_node2".into(), "Beta Msg 1".into())
        .await;
    let b_m2 = mgr
        .post(space_b.into(), "xv1_node2".into(), "Beta Msg 2".into())
        .await;

    // Verify sequences are monotonic starting from 0 per space
    assert_eq!(a_m0.seq, 0);
    assert_eq!(a_m1.seq, 1);

    assert_eq!(b_m0.seq, 0);
    assert_eq!(b_m1.seq, 1);
    assert_eq!(b_m2.seq, 2);

    // Verify space lengths and message contents remain isolated
    assert_eq!(mgr.len(space_a).await, 2);
    assert_eq!(mgr.len(space_b).await, 3);

    let a_all = mgr.list_all(space_a).await;
    let b_all = mgr.list_all(space_b).await;

    assert!(a_all.iter().all(|m| m.space_id == space_a));
    assert!(b_all.iter().all(|m| m.space_id == space_b));

    // Verify unknown space returns empty vec and 0 length
    assert_eq!(mgr.len("esp_unknown").await, 0);
    assert!(mgr.list_all("esp_unknown").await.is_empty());
    assert!(mgr.list_since("esp_unknown", 0).await.is_empty());
}

#[tokio::test]
async fn test_space_manager_directory_and_namespace_isolation() {
    let tmp = tempdir().expect("create temp dir for space manager");
    let mgr = SpaceManager::new(tmp.path());

    let space_id_1 = "esp_01H_alpha";
    let space_id_2 = "esp_01H_beta";

    // Create Space 1
    let s1 = mgr
        .create(
            space_id_1.into(),
            "Alpha Workspace".into(),
            "Primary space for team Alpha".into(),
            "xv1_owner_1".into(),
            false,
        )
        .await
        .expect("create space 1 should succeed");

    // Create Space 2
    let s2 = mgr
        .create(
            space_id_2.into(),
            "Beta Workspace".into(),
            "Public space for team Beta".into(),
            "xv1_owner_2".into(),
            true,
        )
        .await
        .expect("create space 2 should succeed");

    // Verify metadata and namespaces
    assert_eq!(s1.id, space_id_1);
    assert_eq!(s1.name, "Alpha Workspace");
    assert!(!s1.is_public);
    assert_eq!(
        s1.namespace,
        SpaceManager::namespace_for(space_id_1, "xavier", "default")
    );
    assert_eq!(
        s1.namespace,
        format!("xavier://{space_id_1}/xavier/default")
    );

    assert_eq!(s2.id, space_id_2);
    assert_eq!(s2.name, "Beta Workspace");
    assert!(s2.is_public);

    // Verify storage directory paths
    assert_eq!(s1.storage_path, tmp.path().join(space_id_1));
    assert_eq!(s2.storage_path, tmp.path().join(space_id_2));
    assert!(s1.storage_path.exists());
    assert!(s2.storage_path.exists());

    // Verify cross-space isolation check
    assert!(mgr.are_isolated(space_id_1, space_id_2).await);

    // Verify listing spaces
    let space_list = mgr.list().await;
    assert_eq!(space_list.len(), 2);

    // Delete Space 1 and verify cleanup
    mgr.delete(space_id_1)
        .await
        .expect("delete space 1 should succeed");
    assert!(mgr.get(space_id_1).await.is_err());
    assert!(!s1.storage_path.exists());

    let remaining_list = mgr.list().await;
    assert_eq!(remaining_list.len(), 1);
    assert_eq!(remaining_list[0].id, space_id_2);
}

#[tokio::test]
async fn test_invite_lifecycle_roundtrip() {
    let invite_mgr = InviteManager::new();
    let space_id = "esp_invite_space_01";
    let inviter = "xv1_admin_node";
    let target = "xv1_peer_node";

    // 1. Create invitation
    let invite = invite_mgr
        .create(
            space_id.into(),
            inviter.into(),
            target.into(),
            SpaceRole::Moderator,
        )
        .await
        .expect("invite creation should succeed");

    assert_eq!(invite.space_id, space_id);
    assert_eq!(invite.inviter_node, inviter);
    assert_eq!(invite.target_node, target);
    assert_eq!(invite.role, SpaceRole::Moderator);
    assert!(!invite.revoked);
    assert!(invite.is_valid());
    assert!(!invite.is_expired());

    // 2. Check canonical payload stability
    let expected_payload = format!(
        "{}:{}:{}:{}:moderator",
        invite.id, space_id, inviter, target
    );
    assert_eq!(invite.canonical_payload(), expected_payload);

    // 3. Attach Ed25519 signature hex stub
    let sig_hex = "3045022100abc123def4567890abcdef1234567890";
    invite_mgr
        .attach_signature(&invite.id, sig_hex.into())
        .await
        .expect("signature attachment should succeed");

    let signed_invite = invite_mgr
        .get(&invite.id)
        .await
        .expect("fetch signed invite");
    assert_eq!(signed_invite.signature.as_deref(), Some(sig_hex));

    // 4. Validate active invite
    let validated = invite_mgr
        .validate(&invite.id)
        .await
        .expect("validation of active invite should pass");
    assert_eq!(validated.id, invite.id);

    // 5. Revoke invite and confirm validation fails
    invite_mgr
        .revoke(&invite.id)
        .await
        .expect("revoke should succeed");

    let revoked_invite = invite_mgr
        .get(&invite.id)
        .await
        .expect("get revoked invite");
    assert!(revoked_invite.revoked);
    assert!(!revoked_invite.is_valid());

    let val_err = invite_mgr.validate(&invite.id).await;
    assert!(val_err.is_err());
    let err_msg = val_err.unwrap_err().to_string();
    assert!(err_msg.contains("revoked"));
}

#[tokio::test]
async fn test_invite_expiry_and_space_listing() {
    let invite_mgr = InviteManager::new();
    let space_a = "esp_space_listing_a";
    let space_b = "esp_space_listing_b";

    // Create expired invite using create_with_expiry
    let past_expiry = Utc::now() - ChronoDuration::hours(2);
    let expired_inv = invite_mgr
        .create_with_expiry(
            space_a.into(),
            "xv1_admin".into(),
            "xv1_target1".into(),
            SpaceRole::Reader,
            past_expiry,
        )
        .await
        .expect("create expired invite should succeed");

    assert!(expired_inv.is_expired());
    assert!(!expired_inv.is_valid());

    let exp_val_res = invite_mgr.validate(&expired_inv.id).await;
    assert!(exp_val_res.is_err());
    assert!(exp_val_res.unwrap_err().to_string().contains("expired"));

    // Create multiple valid invites across Space A and Space B
    invite_mgr
        .create(
            space_a.into(),
            "xv1_admin".into(),
            "xv1_target2".into(),
            SpaceRole::Member,
        )
        .await
        .unwrap();

    invite_mgr
        .create(
            space_b.into(),
            "xv1_admin".into(),
            "xv1_target3".into(),
            SpaceRole::Admin,
        )
        .await
        .unwrap();

    // Verify list_for_space filtering
    let invites_a = invite_mgr.list_for_space(space_a).await;
    let invites_b = invite_mgr.list_for_space(space_b).await;

    assert_eq!(invites_a.len(), 2);
    assert_eq!(invites_b.len(), 1);
    assert_eq!(invites_b[0].role, SpaceRole::Admin);
}

#[tokio::test]
async fn test_membership_permissions_end_to_end() {
    // Create membership records for different node roles
    let admin_member = SpaceMembership {
        node_id: "node_admin".into(),
        role: SpaceRole::Admin,
        joined_at: Utc::now(),
    };

    let mod_member = SpaceMembership {
        node_id: "node_mod".into(),
        role: SpaceRole::Moderator,
        joined_at: Utc::now(),
    };

    let regular_member = SpaceMembership {
        node_id: "node_member".into(),
        role: SpaceRole::Member,
        joined_at: Utc::now(),
    };

    let reader_member = SpaceMembership {
        node_id: "node_reader".into(),
        role: SpaceRole::Reader,
        joined_at: Utc::now(),
    };

    // 1. Admin checks (can perform all actions)
    assert!(can(admin_member.role, SpaceAction::Read));
    assert!(can(admin_member.role, SpaceAction::Write));
    assert!(can(admin_member.role, SpaceAction::ManageMembers));
    assert!(can(admin_member.role, SpaceAction::Admin));

    // 2. Moderator checks (Read, Write, ManageMembers, but not Admin)
    assert!(can(mod_member.role, SpaceAction::Read));
    assert!(can(mod_member.role, SpaceAction::Write));
    assert!(can(mod_member.role, SpaceAction::ManageMembers));
    assert!(!can(mod_member.role, SpaceAction::Admin));

    // 3. Regular Member checks (Read, Write, but not ManageMembers or Admin)
    assert!(can(regular_member.role, SpaceAction::Read));
    assert!(can(regular_member.role, SpaceAction::Write));
    assert!(!can(regular_member.role, SpaceAction::ManageMembers));
    assert!(!can(regular_member.role, SpaceAction::Admin));

    // 4. Reader checks (Read only)
    assert!(can(reader_member.role, SpaceAction::Read));
    assert!(!can(reader_member.role, SpaceAction::Write));
    assert!(!can(reader_member.role, SpaceAction::ManageMembers));
    assert!(!can(reader_member.role, SpaceAction::Admin));
}
