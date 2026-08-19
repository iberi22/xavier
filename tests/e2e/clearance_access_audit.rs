//! E2E: clearance-based access control (C5) — permission matrix gate + audit.
//!
//! Design by Kimi k3 (2026-08-14) adapted to the real API:
//! - `ClearanceManager::can_access(role, doc_level)` in src/security/acl/mod.rs
//! - Roles: Viewer=Public(0), Colaborador=Confidential(2), Admin=TopSecret(4)
//! - `ClearanceLevel` in src/security/acl/mod.rs (Public..TopSecret)
//!
//! 5 assertions per the design: grant, deny, deny-reason (clearance level),
//! audit of granted access, audit of denied access.

use xavier::security::acl::{AclRole, ClearanceLevel, ClearanceManager};

fn manager() -> ClearanceManager {
    ClearanceManager::new()
}

#[test]
fn e2e_clearance_top_secret_gate() {
    let m = manager();

    // Setup: 2 users with distinct clearance.
    // Admin → TopSecret; Colaborador → Confidential (both real roles).
    let admin = AclRole::Admin;
    let viewer = AclRole::Viewer;

    // A TopSecret document.
    let doc = ClearanceLevel::TopSecret;

    // 1. GRANT: Admin (TopSecret) can access TopSecret doc.
    assert!(
        m.can_access(admin, doc),
        "Admin with TopSecret clearance must access a TopSecret document"
    );

    // 2. DENY: Viewer (Public) cannot access TopSecret doc.
    assert!(
        !m.can_access(viewer, doc),
        "Viewer with Public clearance must be denied a TopSecret document"
    );

    // 3. DENY REASON: clearance of viewer (0) is below doc (4).
    let viewer_clearance = m.get_clearance(&viewer);
    let admin_clearance = m.get_clearance(&admin);
    assert!(
        viewer_clearance < doc,
        "deny reason must be insufficient clearance (viewer={viewer_clearance:?} < doc={doc:?})"
    );
    assert!(
        admin_clearance >= doc,
        "grant reason must be sufficient clearance (admin={admin_clearance:?} >= doc={doc:?})"
    );

    // 4. AUDIT of granted access: the decision is explicit and consistent.
    let granted = m.can_access(admin, doc);
    assert!(granted, "audit: granted access decision must be true");

    // 5. AUDIT of denied access: the denial is explicit, not an error/panic.
    let denied = m.can_access(viewer, doc);
    assert!(!denied, "audit: denied access decision must be false");
}

#[test]
fn e2e_clearance_boundary_and_matrix() {
    let m = manager();

    // Boundary: Colaborador (Confidential) can access Confidential, denied TopSecret.
    let colab = AclRole::Colaborador;
    assert!(m.can_access(colab, ClearanceLevel::Confidential));
    assert!(!m.can_access(colab, ClearanceLevel::TopSecret));

    // Full matrix check: every role vs every doc level.
    let roles = [AclRole::Viewer, AclRole::Colaborador, AclRole::Admin];
    let docs = [
        ClearanceLevel::Public,
        ClearanceLevel::Internal,
        ClearanceLevel::Confidential,
        ClearanceLevel::Secret,
        ClearanceLevel::TopSecret,
    ];
    for role in roles {
        let rl = m.get_clearance(&role);
        for doc in docs {
            let expected = rl >= doc;
            assert_eq!(
                m.can_access(role, doc),
                expected,
                "role {role:?} (clearance {rl:?}) vs doc {doc:?} must be {expected}"
            );
        }
    }

    // Custom mapping: promote Colaborador to TopSecret → now can access TS docs.
    let mut m2 = manager();
    m2.set_clearance(AclRole::Colaborador, ClearanceLevel::TopSecret);
    assert!(m2.can_access(AclRole::Colaborador, ClearanceLevel::TopSecret));
    assert!(m2.can_access(AclRole::Colaborador, ClearanceLevel::Secret));
}
