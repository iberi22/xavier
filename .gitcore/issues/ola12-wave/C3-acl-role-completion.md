# C3: ACL role completion (90% → 100%)

## Problem

ACL & permissions are at 90%. Role hierarchy, permission inheritance, and
audit trail are incomplete. The base RBAC system works but lacks the
finishing touches for production use.

## Solution

Complete the ACL system with:

1. **Role hierarchy**: Admin > Colaborador > Viewer (permissions inherit downward)
2. **Permission inheritance**: Child roles inherit parent permissions automatically
3. **Audit trail**: Log all permission checks (who accessed what, when, result)

### Steps

1. Add `RoleHierarchy` struct in `src/security/acl/hierarchy.rs`
2. Implement `inherits(child_role, parent_role) -> bool`
3. Add `AuditEntry` struct and `audit_log` table to security DB
4. Wire audit logging into existing permission check functions
5. Add tests for hierarchy inheritance and audit logging

## Acceptance

- [ ] Role hierarchy: Admin inherits Colaborador inherits Viewer
- [ ] Permission check for Colaborador includes Viewer permissions
- [ ] Audit log records all permission checks with timestamp + result
- [ ] `cargo test -p xavier --lib acl` passes
- [ ] No regression in existing ACL tests

## Files

- `src/security/acl/hierarchy.rs` (new)
- `src/security/acl/mod.rs` (modify)
- `src/security/audit.rs` (new or modify)
