//! Skill drift audit end to end on a temp fixture: conflict reported, secret
//! value never rendered, proposal is a unified diff, source stays untouched.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use xavier::api::skills::audit_report_json;
use xavier::context::skill_controller::audit::{audit_packages, AuditedPackage, Severity};
use xavier::context::skill_controller::manifest::PlacementRecord;
use xavier::context::skill_controller::plan::ResolvedRoots;
use xavier::context::skill_controller::policy::FsPolicy;

/// Fake credential assembled at runtime so no fixture holds secret bytes.
fn fake_key() -> String {
    format!("AIza{}", "SyD".repeat(10))
}

fn write(path: PathBuf, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

fn pkg(name: &str, path: &str) -> AuditedPackage {
    AuditedPackage {
        placement: PlacementRecord {
            source: "canonical".into(),
            path: path.into(),
            name: name.into(),
            targets: vec!["t1".into()],
        },
        recorded_hash: None,
    }
}

fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap().flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                let rel = p.strip_prefix(root).unwrap().to_string_lossy().into_owned();
                out.insert(rel, fs::read(&p).unwrap());
            }
        }
    }
    out
}

#[test]
fn skill_drift_audit_reports_conflict_hides_secret_and_proposes_diff() {
    let tmp = TempDir::new().unwrap();
    let source = tmp.path().join("store");
    let secret = fake_key();
    write(
        source.join("alpha/SKILL.md"),
        "---\nname: alpha\nversion: 1.0.0\n---\n\nBody.\n",
    );
    write(
        source.join("alpha-again/SKILL.md"),
        "---\nname: alpha\nversion: 2.0.0\n---\n\nBody.\n",
    );
    write(
        source.join("beta/SKILL.md"),
        "---\nname: beta\ndescription: Use beta.\n---\n\nBody.\n",
    );
    write(
        source.join("beta/references/notes.md"),
        &format!("notes\nkey = {secret}\n"),
    );
    let before = snapshot(&source);

    let policy = FsPolicy::new(std::slice::from_ref(&source), &[]).unwrap();
    let roots = ResolvedRoots {
        sources: BTreeMap::from([("canonical".to_string(), source.clone())]),
        targets: BTreeMap::new(),
    };
    let packages = vec![
        pkg("alpha", "alpha"),
        pkg("beta", "beta"),
        pkg("alpha", "alpha-again"),
    ];
    let report = audit_packages(&policy, &roots, &packages).expect("audit");

    // The duplicate declaration is reported as a conflict.
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.package == "alpha" && f.detail.contains("2 placements")),
        "{:?}",
        report.findings
    );
    // The secret is located but its value never appears in any output.
    let secret_finding = report
        .findings
        .iter()
        .find(|f| f.severity == Severity::Error && f.package == "beta")
        .expect("secret finding");
    assert_eq!(secret_finding.location, "references/notes.md:2");
    let rendered = format!(
        "{:?}{}",
        report,
        serde_json::to_string(&audit_report_json(&report)).unwrap()
    );
    assert!(!rendered.contains(&secret));

    // Proposals are unified diffs.
    assert!(!report.proposals.is_empty());
    for p in &report.proposals {
        assert!(p.diff.starts_with("--- a/"), "{}", p.diff);
        assert!(p.diff.contains("+++ b/"), "{}", p.diff);
        assert!(p.diff.contains("@@"), "{}", p.diff);
    }

    // Source is byte-identical after the audit.
    assert_eq!(snapshot(&source), before);
}
