---
title: "Dependency Security Hygiene (Dependabot)"
description: "Triage and remediate Dependabot alerts on main"
---

**Status:** `stable` | **Score:** 100% | **Last Tested:** 2026-07-18

## Overview
Continuous security auditing, vulnerability triage, and dependency remediation. This feature tracks known CVEs, overrides vulnerable node packages in the web panel, upgrades core crates like `jsonwebtoken`, and documents outstanding transitive issues.

## Architecture & Design
Vulnerability management is governed by Dependabot automated checks, combined with targeted manual remediations. Critical dependencies (such as cryptographic packages and web servers) are pinned or upgraded to safe targets (such as upgrading `jsonwebtoken` to version 10.3+ to protect against CVE-2026-25537).

## Implementation Paths
- `docs/SECURITY_DEPENDABOT.md` or `docs/DEPENDENCY_AUDIT_REPORT.md` (comprehensive vulnerability audit and dependency maps)
- `Cargo.lock` (reconciled crate versions)
- `panel-ui/package.json` (npm resolutions and overrides for safe web packages)

## Sub-features
- **sec-inventory:** Detailed dependency audits and vulnerability tracking.
- **sec-npm:** Reconciles panel dependencies by applying strict resolutions (such as forcing `undici` to 7.28.0+).
- **sec-cargo-jwt:** Upgrades cryptographic frameworks to bypass type-confusion vulnerabilities.
- **sec-cargo-deferred:** Cataloging and safe dismissal of unfixable or non-exploitable transitive crates.

## Test References
- Standard build compilation checks on updated dependencies.
- Token validation security tests.

## Known Issues & Notes
- Outlined thoroughly in the active Dependency Audit Report, with deferred items mapped to future update cycles once upstream fixes are available.

### Functional Security Audit Example
Audit dependencies for security and license compliance:

```bash
# Run a dependency security audit via pnpm
cd panel-ui && pnpm audit

# Audit Cargo dependencies
cargo audit
```
