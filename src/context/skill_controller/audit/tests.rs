use super::super::policy::PACKAGE_BOUNDS;
use super::member::member_path;
use super::rules::*;
use super::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tempfile::TempDir;

/// `(tempdir, source root)`: the tempdir owns the fixture tree.
fn fixture() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().expect("temp dir");
    let source = tmp.path().join("store");
    fs::create_dir_all(&source).expect("create source root");
    (tmp, source)
}

fn placement(name: &str, path: &str) -> PlacementRecord {
    PlacementRecord {
        source: "canonical".into(),
        path: path.into(),
        name: name.into(),
        targets: vec!["t1".into()],
    }
}

fn policy(source: &Path) -> FsPolicy {
    FsPolicy::new(std::slice::from_ref(&source.to_path_buf()), &[]).expect("policy")
}

fn roots(source: &Path) -> ResolvedRoots {
    ResolvedRoots {
        sources: BTreeMap::from([("canonical".to_string(), source.to_path_buf())]),
        targets: BTreeMap::new(),
    }
}

fn audited(name: &str, path: &str, recorded: Option<&str>) -> AuditedPackage {
    AuditedPackage {
        placement: placement(name, path),
        recorded_hash: recorded.map(str::to_string),
    }
}

/// A fake credential assembled at runtime, so no fixture holds secret bytes.
fn fake_google_key() -> String {
    let mut key = String::from("AIza");
    for _ in 0..10 {
        key.push_str("SyD");
    }
    key
}

fn member(dir: &Path, relative: &str, body: &str) {
    let path = dir.join(relative);
    fs::create_dir_all(path.parent().expect("parent dir")).expect("create parent");
    fs::write(&path, body).expect("write member");
}

fn audit(source: &Path, packages: &[AuditedPackage]) -> AuditReport {
    audit_packages(&policy(source), &roots(source), packages).expect("audit")
}

/// `-----BEGIN <kind> PRIVATE KEY-----`, assembled at runtime so no PEM literal is
/// ever committed (the repo secret scanners read tracked bytes).
fn pem_header(kind: &str) -> String {
    let dashes = "-".repeat(5);
    let kind = kind.trim();
    let kind = if kind.is_empty() {
        String::new()
    } else {
        format!("{kind} ")
    };
    format!("{dashes}BEGIN {kind}PRIVATE KEY{dashes}")
}

/// Every file under `root` as `(relative path, bytes)`, the evidence that the
/// audit is read-only.
fn tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            if let Ok(bytes) = fs::read(&path) {
                out.insert(relative, bytes);
            }
        }
    }
    out
}

#[test]
fn test_fixture_reports_description_reference_conflict_and_secret() {
    let (_tmp, source) = fixture();
    let secret = fake_google_key();
    member(
        &source.join("alpha"),
        "SKILL.md",
        concat!(
            "---\nname: alpha\nversion: 1.0.0\n---\n\n",
            "Read [the guide](references/missing.md) first.\n",
            "Endpoint https://user:pw@example.com/docs?token=abc is unreachable.\n",
        ),
    );
    member(
        &source.join("beta"),
        "SKILL.md",
        "---\nname: beta\ndescription: Use beta when reconciling.\nversion: 2.0.0\n---\n\nBody.\n",
    );
    member(
        &source.join("beta/references"),
        "notes.md",
        &format!("notes\nkey = {secret}\n"),
    );
    member(
        &source.join("alpha-again"),
        "SKILL.md",
        "---\nname: alpha\nversion: 3.0.0\n---\n\nBody.\n",
    );
    let packages = vec![
        audited("alpha", "alpha", None),
        audited("beta", "beta", Some(&"0".repeat(64))),
        audited("alpha", "alpha-again", None),
    ];
    let report = audit(&source, &packages);
    let rendered = format!("{:?} {:?}", report.findings, report.proposals);
    let rule = |id: Rule| {
        report
            .findings
            .iter()
            .find(|finding| finding.rule == id)
            .unwrap_or_else(|| panic!("no {} finding in {rendered}", id.id))
            .clone()
    };

    let description = rule(DESC_MISSING);
    assert_eq!(description.package, "alpha");
    assert_eq!(description.location, "SKILL.md:1");
    assert_eq!(description.severity, Severity::Warning);

    let reference = rule(REF_BROKEN);
    assert_eq!(reference.location, "SKILL.md:6");
    assert!(
        reference.detail.starts_with("link ") && reference.detail.ends_with(" missing"),
        "{}",
        reference.detail
    );
    assert!(!reference.detail.contains("missing.md"));

    let conflict = rule(DECL_CONFLICT);
    assert_eq!(conflict.package, "alpha");
    assert!(conflict.detail.contains("2 placements"));

    let version = rule(DECL_VERSION);
    assert_eq!(version.package, "alpha");
    assert!(version.detail.contains("2 declared versions"));

    let secret_finding = rule(SECRET);
    assert_eq!(secret_finding.severity, Severity::Error);
    assert_eq!(secret_finding.package, "beta");
    assert_eq!(secret_finding.location, "references/notes.md:2");
    assert!(secret_finding.detail.contains("google_api_key"));
    assert!(report
        .findings
        .iter()
        .any(|f| f.severity == Severity::Error));

    assert_eq!(rule(HASH_DRIFT).package, "beta");

    let endpoint = rule(ENDPOINT);
    assert_eq!(endpoint.severity, Severity::Info);
    assert_eq!(endpoint.detail, "not probed: https://example.com");

    // Redaction: no secret bytes, no credentials, no absolute temp path.
    assert!(!rendered.contains(&secret));
    assert!(!rendered.contains("user:pw"));
    assert!(!rendered.contains("token=abc"));
    assert!(!rendered.contains(&source.to_string_lossy().into_owned()));
    // `alpha` holds no secret, so its proposal survives with no context line.
    let alpha_proposal = report
        .proposals
        .iter()
        .find(|proposal| proposal.package == "alpha")
        .expect("alpha earns a proposal");
    assert_eq!(
        alpha_proposal.diff,
        "--- a/SKILL.md\n+++ b/SKILL.md\n@@ -2,0 +3 @@\n+description: TODO describe when to use alpha\n"
    );
}

#[test]
fn test_proposal_is_a_unified_diff_and_leaves_the_source_byte_identical() {
    let (_tmp, source) = fixture();
    member(
        &source.join("demo"),
        "SKILL.md",
        "---\nname: demo\n---\n\nBody.\n",
    );
    let before = tree(&source);
    let packages = vec![audited("demo", "demo", None)];
    let report = audit(&source, &packages);
    assert_eq!(report.proposals.len(), 1, "{report:?}");
    let proposal = &report.proposals[0];
    assert_eq!(proposal.rule, DESC_MISSING);
    assert_eq!(proposal.path, "SKILL.md");
    assert!(
        proposal.diff.contains("--- a/SKILL.md"),
        "{}",
        proposal.diff
    );
    assert!(
        proposal.diff.contains("+++ b/SKILL.md"),
        "{}",
        proposal.diff
    );
    assert!(proposal.diff.contains("+description:"), "{}", proposal.diff);
    assert_eq!(
        tree(&source),
        before,
        "the audit must not rewrite the package"
    );
}

#[test]
fn test_proposal_diff_never_carries_a_context_or_removed_line() {
    let (_tmp, source) = fixture();
    member(
        &source.join("demo"),
        "SKILL.md",
        "---\nname: demo\n---\n\nBody.\n",
    );
    let packages = vec![audited("demo", "demo", None)];
    let report = audit(&source, &packages);
    let diff = &report.proposals[0].diff;
    assert!(!diff.contains("Body."), "{diff}");
    assert!(!diff.contains("name: demo"), "{diff}");
    assert!(!diff.lines().any(|line| line.starts_with(' ')), "{diff}");
    assert_eq!(
        diff.lines()
            .filter(|l| l.starts_with('+') && !l.starts_with("+++"))
            .count(),
        1,
        "{diff}"
    );
    assert_eq!(
        diff.lines().filter(|l| l.starts_with('-')).count(),
        1,
        "{diff}"
    );
}

#[test]
fn test_no_proposal_is_rendered_for_a_secret_finding() {
    let (_tmp, source) = fixture();
    member(
        &source.join("demo"),
        "SKILL.md",
        "---\nname: demo\ndescription: ok\n---\n\nBody.\n",
    );
    let secret = fake_google_key();
    member(
        &source.join("demo/references"),
        "notes.md",
        &format!("key = {secret}\n"),
    );
    let packages = vec![audited("demo", "demo", None)];
    let report = audit(&source, &packages);
    assert!(report.proposals.is_empty(), "{report:?}");
    let rendered = format!("{:?}", report.findings);
    assert!(!rendered.contains(&secret));
    assert!(report
        .findings
        .iter()
        .any(|f| f.severity == Severity::Error));
}

/// One leaking input per family the review listed, each sitting on the line right
/// below `name:` so it would land inside a context-carrying hunk.
fn leaking_bodies() -> Vec<(&'static str, String)> {
    let google = fake_google_key();
    vec![
        (
            "url_userinfo",
            "https://user:pw@example.com/docs?token=abc".into(),
        ),
        ("google_class", google),
        ("openai_class", format!("sk-or-v1-{}", "a".repeat(24))),
        (
            "aws_env_secret",
            "AWS_SECRET_ACCESS_KEY=env-review-secret-99".into(),
        ),
        ("rsa_private_key", pem_header("RSA")),
        ("pkcs8_private_key", pem_header("")),
        (
            "base64_blob",
            "b64-review-QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVo=".into(),
        ),
        ("split_pair", "AIzaSyDSyDSyDSy".into()),
    ]
}

#[test]
fn test_proposal_never_leaks_any_secret_family_the_review_listed() {
    for (label, marker) in leaking_bodies() {
        let (_tmp, source) = fixture();
        let body = if label == "split_pair" {
            format!("---\nname: demo\n{marker}\nsplit-rest-review-99\n---\n")
        } else {
            format!("---\nname: demo\n{marker}\n---\n")
        };
        member(&source.join("demo"), "SKILL.md", &body);
        let packages = vec![audited("demo", "demo", None)];
        let report = audit(&source, &packages);
        let rendered = format!("{:?} {:?}", report.findings, report.proposals);
        assert!(!rendered.contains(&marker), "{label} leaked in {rendered}");
        if label == "pkcs8_private_key" || label == "aws_env_secret" {
            assert!(
                !rendered.contains("pkcs8-body") && !rendered.contains("env-review-secret"),
                "{label} leaked its body"
            );
        }
        for proposal in &report.proposals {
            assert!(
                !proposal.diff.lines().any(|line| line.starts_with(' ')),
                "{label} rendered a context line: {}",
                proposal.diff
            );
            assert!(
                !proposal.diff.contains(&marker),
                "{label} rendered the marker: {}",
                proposal.diff
            );
        }
        // The scanner's own classes also keep the value out of the finding.
        if matches!(label, "google_class" | "openai_class" | "rsa_private_key") {
            let secret = report
                .findings
                .iter()
                .find(|finding| finding.rule == SECRET)
                .unwrap_or_else(|| panic!("{label} produced no secret finding: {rendered}"));
            assert!(
                secret.detail.contains("pattern matched"),
                "{}",
                secret.detail
            );
            assert!(report.proposals.is_empty(), "{label} still proposed a diff");
        }
    }
}

/// A secret the scanner classifies inside the package rules out any proposal,
/// even though the description is missing and a diff would otherwise be due.
#[test]
fn test_a_secret_in_skill_md_rules_out_the_description_proposal() {
    let (_tmp, source) = fixture();
    let secret = fake_google_key();
    member(
        &source.join("demo"),
        "SKILL.md",
        &format!("---\nname: demo\n---\n\nkey = {secret}\n"),
    );
    let packages = vec![audited("demo", "demo", None)];
    let report = audit(&source, &packages);
    assert!(
        report.findings.iter().any(|f| f.rule == SECRET),
        "{report:?}"
    );
    assert!(report.proposals.is_empty(), "{report:?}");
    assert!(
        !format!("{:?}", report).contains(&secret),
        "the key reached the report"
    );
}

#[test]
fn test_proposal_is_suppressed_when_the_composed_line_matches_a_secret_shape() {
    let (_tmp, source) = fixture();
    member(&source.join("one"), "SKILL.md", "---\nname: one\n---\n");
    member(&source.join("two"), "SKILL.md", "---\nname: two\n---\n");
    let packages = vec![
        audited("one BEGIN PRIVATE KEY one", "one", None),
        audited("two_SECRET_VALUE=fake-env-review-99", "two", None),
    ];
    let report = audit(&source, &packages);
    assert!(report.proposals.is_empty(), "{report:?}");
    let suppressed: Vec<&Finding> = report
        .findings
        .iter()
        .filter(|finding| finding.rule == PROPOSAL_SUPPRESSED)
        .collect();
    assert_eq!(suppressed.len(), 2, "{report:?}");
    for finding in suppressed {
        assert_eq!(
            finding.detail,
            "proposal withheld: it matched a sensitive pattern"
        );
        assert!(!finding.detail.contains("BEGIN PRIVATE KEY"));
    }
    let rendered = format!("{:?}", report.proposals);
    assert!(!rendered.contains("+description:"), "{rendered}");
    assert!(!rendered.contains("fake-env-review-99"), "{rendered}");
    assert!(!rendered.contains("BEGIN PRIVATE KEY"), "{rendered}");
}

#[test]
fn test_endpoint_findings_keep_only_scheme_host_and_port() {
    let (_tmp, source) = fixture();
    member(
        &source.join("demo"),
        "SKILL.md",
        concat!(
            "---\nname: demo\ndescription: ok\n---\n\n",
            "Hook https://example.com/hook/path-token-review-99?token=abc then ",
            "https://example.com/user:pw@not-userinfo/x and https://example.com:8443/z end.\n",
        ),
    );
    let packages = vec![audited("demo", "demo", None)];
    let report = audit(&source, &packages);
    let details: Vec<&str> = report
        .findings
        .iter()
        .filter(|finding| finding.rule == ENDPOINT)
        .map(|finding| finding.detail.as_str())
        .collect();
    assert_eq!(
        details,
        vec![
            "not probed: https://example.com",
            "not probed: https://example.com",
            "not probed: https://example.com:8443",
        ],
        "{report:?}"
    );
    let rendered = format!("{report:?}");
    assert!(!rendered.contains("path-token-review-99"));
    assert!(!rendered.contains("token=abc"));
    assert!(!rendered.contains("user:pw"));
    assert!(!rendered.contains("not-userinfo"));
}

#[test]
fn test_endpoint_cap_marks_the_report_truncated() {
    let (_tmp, source) = fixture();
    let many = (0..AUDIT_BOUNDS.2 + 1)
        .map(|index| format!("https://example.com/{index}"))
        .collect::<Vec<_>>()
        .join(" ");
    member(
        &source.join("demo"),
        "SKILL.md",
        &format!("---\nname: demo\ndescription: ok\n---\n\n{many}\n"),
    );
    let packages = vec![audited("demo", "demo", None)];
    let report = audit(&source, &packages);
    assert_eq!(
        report
            .findings
            .iter()
            .filter(|finding| finding.rule == ENDPOINT)
            .count(),
        AUDIT_BOUNDS.2
    );
    assert!(report.truncated, "{report:?}");
}

#[test]
fn test_broken_link_reports_a_digest_not_the_target() {
    let (_tmp, source) = fixture();
    let key_link = format!("keys/sk-or-v1-{}", "a".repeat(24));
    member(
        &source.join("demo"),
        "SKILL.md",
        &format!("---\nname: demo\ndescription: ok\n---\n\nSee [k]({key_link}).\n"),
    );
    let packages = vec![audited("demo", "demo", None)];
    let report = audit(&source, &packages);
    let broken: Vec<&Finding> = report
        .findings
        .iter()
        .filter(|finding| finding.rule == REF_BROKEN)
        .collect();
    assert_eq!(broken.len(), 1, "{report:?}");
    assert_eq!(broken[0].location, "SKILL.md:6");
    let detail = broken[0].detail.as_str();
    let digest = detail
        .strip_prefix("link ")
        .and_then(|rest| rest.strip_suffix(" missing"))
        .unwrap_or_else(|| panic!("unexpected detail {detail}"));
    assert_eq!(digest.len(), 8, "{detail}");
    assert!(digest.chars().all(|c| c.is_ascii_hexdigit()), "{detail}");
    assert!(!digest.chars().any(|c| c.is_ascii_uppercase()), "{detail}");
    assert!(!format!("{report:?}").contains("sk-or-v1-aaaa"));
}

#[test]
fn test_member_path_refuses_traversal_absolute_and_symlinked_inputs() {
    let (_tmp, source) = fixture();
    member(&source.join("demo"), "SKILL.md", "---\nname: demo\n---\n");
    member(&source.join("demo/references"), "notes.md", "notes\n");
    let root = source.join("demo");
    let policy = policy(&source);
    let dir = policy
        .authorize(RootKind::Source, &root)
        .expect("authorize the fixture package");

    assert!(member_path(dir.path(), "SKILL.md").is_some());
    assert!(member_path(dir.path(), "references/notes.md").is_some());
    assert!(member_path(dir.path(), "../outside.md").is_none());
    assert!(member_path(dir.path(), "references/../../outside.md").is_none());
    assert!(member_path(dir.path(), "").is_none());
    assert!(member_path(dir.path(), "/etc/passwd").is_none());

    let outside = _tmp.path().join("outside.md");
    member(_tmp.path(), "outside.md", "outside-canary\n");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, source.join("demo/leak.md")).expect("symlink member");
        assert!(member_path(dir.path(), "leak.md").is_none());
        // An in-package symlink is refused too: the reader never follows a link.
        std::os::unix::fs::symlink(source.join("demo/SKILL.md"), source.join("demo/alias.md"))
            .expect("symlink alias");
        assert!(member_path(dir.path(), "alias.md").is_none());
        let sub = source.join("demo/sub");
        fs::create_dir_all(&sub).expect("create sub");
        std::os::unix::fs::symlink(_tmp.path(), source.join("demo/out")).expect("symlink dir");
        assert!(member_path(dir.path(), "out/outside.md").is_none());
        assert!(read_member_text(&dir, "leak.md").is_none());
        assert!(!format!("{:?}", read_member_text(&dir, "SKILL.md")).contains("canary"));
    }
}

#[test]
fn test_read_member_refuses_an_oversized_member_before_reading_it() {
    let (_tmp, source) = fixture();
    member(&source.join("demo"), "SKILL.md", "---\nname: demo\n---\n");
    let big = format!("HUGEMARK{}", "a".repeat(PACKAGE_BOUNDS.3 as usize));
    member(&source.join("demo"), "notes.md", &big);
    let policy = policy(&source);
    let dir = policy
        .authorize(RootKind::Source, &source.join("demo"))
        .expect("authorize the fixture package");
    assert!(read_member_text(&dir, "notes.md").is_none());
    assert!(read_member_text(&dir, "SKILL.md").is_some());
}

#[cfg(unix)]
#[test]
fn test_read_member_refuses_a_fifo_without_opening_it() {
    let (_tmp, source) = fixture();
    member(&source.join("demo"), "SKILL.md", "---\nname: demo\n---\n");
    let policy = policy(&source);
    let dir = policy
        .authorize(RootKind::Source, &source.join("demo"))
        .expect("authorize the fixture package");
    let status = std::process::Command::new("mkfifo")
        .arg(source.join("demo/pipe.md"))
        .status()
        .expect("mkfifo");
    assert!(status.success(), "mkfifo failed: {status}");
    let started = Instant::now();
    assert!(read_member_text(&dir, "pipe.md").is_none());
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "reading a FIFO blocked for {:?}",
        started.elapsed()
    );
}

#[test]
fn test_audit_refuses_a_symlinked_skill_md_pointing_out_of_the_package() {
    let (tmp, source) = fixture();
    member(&source.join("demo"), "notes.md", "notes\n");
    let outside = tmp.path().join("outside");
    member(&outside, "secret.md", "---\nname: demo\noutside-canary-review-99\nhttps://example.com/hook/outside-path-token-99\n---\n");
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.join("secret.md"), source.join("demo/SKILL.md"))
        .expect("symlink SKILL.md");
    let before = tree(&outside);
    let packages = vec![audited("demo", "demo", None)];
    let report = audit(&source, &packages);
    let rendered = format!("{:?} {:?}", report.findings, report.proposals);
    assert!(!rendered.contains("outside-canary-review-99"), "{rendered}");
    assert!(!rendered.contains("outside-path-token-99"), "{rendered}");
    assert!(report.proposals.is_empty(), "{report:?}");
    assert_eq!(report.packages_audited, 1);
    assert!(
        report.findings.iter().any(|f| f.rule == PKG_UNSCANNABLE),
        "{report:?}"
    );
    assert!(
        !report.findings.iter().any(|f| f.rule == SKILL_MD_MISSING),
        "the audit kept reading after the scanner refused the package: {report:?}"
    );
    assert_eq!(tree(&outside), before, "the outside tree must be untouched");
}

#[cfg(unix)]
#[test]
fn test_audit_does_not_block_on_a_fifo_skill_md() {
    let (_tmp, source) = fixture();
    member(&source.join("demo"), "notes.md", "notes\n");
    let status = std::process::Command::new("mkfifo")
        .arg(source.join("demo/SKILL.md"))
        .status()
        .expect("mkfifo");
    assert!(status.success(), "mkfifo failed: {status}");
    let packages = vec![audited("demo", "demo", None)];
    let started = Instant::now();
    let report = audit(&source, &packages);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the audit blocked on the FIFO for {:?}",
        started.elapsed()
    );
    assert!(report.proposals.is_empty(), "{report:?}");
    assert!(
        !format!("{report:?}").contains("notes\n"),
        "the audit read another member instead of stopping"
    );
    assert!(
        !report.findings.iter().any(|f| f.rule == SKILL_MD_MISSING),
        "the audit kept reading after the scanner refused the package: {report:?}"
    );
}

#[test]
fn test_an_oversized_skill_md_is_never_read_by_the_audit() {
    let (_tmp, source) = fixture();
    let big = format!(
        "---\nname: demo\ndescription: ok\n---\nHUGEMARK{}",
        "a".repeat(PACKAGE_BOUNDS.3 as usize)
    );
    member(&source.join("demo"), "SKILL.md", &big);
    let packages = vec![audited("demo", "demo", None)];
    let report = audit(&source, &packages);
    let rendered = format!("{report:?}");
    assert!(!rendered.contains("HUGEMARK"), "{rendered}");
    assert!(report.proposals.is_empty(), "{report:?}");
    assert!(
        report.findings.iter().any(|f| f.rule == PKG_UNSCANNABLE),
        "{report:?}"
    );
    assert!(
        !report.findings.iter().any(|f| f.rule == SKILL_MD_MISSING),
        "{report:?}"
    );
}

#[test]
fn test_read_only_audit_reports_broken_and_escaping_references() {
    let (tmp, source) = fixture();
    member(
        &source.join("demo"),
        "SKILL.md",
        "---\nname: demo\ndescription: ok\n---\n\nSee [x](../outside.md) and [y](missing.md).\n",
    );
    member(&tmp.path().join("outside"), "outside.md", "outside\n");
    let before = tree(&source);
    let packages = vec![audited("demo", "demo", None)];
    let report = audit(&source, &packages);
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.rule == REF_BROKEN && f.location == "SKILL.md:6"),
        "{report:?}"
    );
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.rule == REF_ESCAPES && f.location == "SKILL.md:6"),
        "{report:?}"
    );
    assert_eq!(tree(&source), before, "the audit must not write");
}

#[test]
fn test_policy_violations_and_bounds_are_errors_not_findings() {
    let (tmp, source) = fixture();
    member(
        &tmp.path().join("outside/demo"),
        "SKILL.md",
        "---\nname: outside\n---\n",
    );
    let escaping = vec![audited("outside", "../outside/demo", None)];
    assert_eq!(
        audit_packages(&policy(&source), &roots(&source), &escaping),
        Err(AuditError::OutsideAllowlist)
    );
    let unknowns = vec![AuditedPackage {
        placement: PlacementRecord {
            source: "missing".into(),
            path: "demo".into(),
            name: "demo".into(),
            targets: vec!["t1".into()],
        },
        recorded_hash: None,
    }];
    assert_eq!(
        audit_packages(&policy(&source), &roots(&source), &unknowns),
        Err(AuditError::UnresolvedSource("missing".into()))
    );
    let mut overflow: Vec<AuditedPackage> = Vec::new();
    for _ in 0..=AUDIT_BOUNDS.0 {
        overflow.push(audited("demo", "demo", None));
    }
    assert_eq!(
        audit_packages(&policy(&source), &roots(&source), &overflow),
        Err(AuditError::BoundExceeded {
            bound: "package count"
        })
    );
}

#[test]
fn test_missing_package_is_reported_without_failing_the_audit() {
    let (_tmp, source) = fixture();
    let packages = vec![audited("ghost", "ghost", None)];
    let report = audit(&source, &packages);
    let finding = report
        .findings
        .iter()
        .find(|finding| finding.rule == PKG_MISSING)
        .unwrap_or_else(|| panic!("no unavailable finding in {report:?}"));
    assert_eq!(finding.package, "ghost");
    assert_eq!(finding.severity, Severity::Warning);
    assert_eq!(report.packages_audited, 1);
}

#[test]
fn test_finding_cap_marks_the_report_truncated() {
    let (_tmp, source) = fixture();
    let links = (0..AUDIT_BOUNDS.1 + 1)
        .map(|index| format!("[k{index}](missing{index}.md)"))
        .collect::<Vec<_>>()
        .join("\n");
    member(
        &source.join("demo"),
        "SKILL.md",
        &format!("---\nname: demo\ndescription: ok\n---\n\n{links}\n"),
    );
    let packages = vec![audited("demo", "demo", None)];
    let report = audit(&source, &packages);
    assert_eq!(report.findings.len(), AUDIT_BOUNDS.1, "{report:?}");
    assert!(report.truncated, "{report:?}");
}

#[test]
fn test_description_over_the_limit_is_counted_not_echoed() {
    let (_tmp, source) = fixture();
    let value = "x".repeat(DESCRIPTION_MAX + 1);
    member(
        &source.join("demo"),
        "SKILL.md",
        &format!("---\nname: demo\ndescription: {value}\n---\n"),
    );
    let packages = vec![audited("demo", "demo", None)];
    let report = audit(&source, &packages);
    let finding = report
        .findings
        .iter()
        .find(|finding| finding.rule == DESC_TOO_LONG)
        .unwrap_or_else(|| panic!("no over-long description finding in {report:?}"));
    assert_eq!(
        finding.detail,
        format!("{} chars, limit 1024", DESCRIPTION_MAX + 1)
    );
    assert!(!format!("{report:?}").contains(&value));
}
