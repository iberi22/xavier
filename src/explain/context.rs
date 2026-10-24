//! Evidence model and the context assembler.

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use anyhow::{bail, ensure, Context, Result};

/// Maximum items requested from each source per assembly.
const SOURCE_LIMIT: usize = 20;

/// Kind of evidence gathered for an explanation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EvidenceKind {
    Code,
    CallGraph,
    Memory,
    Decision,
    Commit,
}

/// The code fragment being explained. Lines are 1-based and inclusive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplainTarget {
    pub repo_root: PathBuf,
    /// Path relative to `repo_root`.
    pub file: PathBuf,
    pub start_line: u32,
    pub end_line: u32,
    pub symbol: Option<String>,
}

/// One piece of evidence about the target.
#[derive(Debug, Clone, PartialEq)]
pub struct Evidence {
    pub kind: EvidenceKind,
    /// Identifies the origin, e.g. a file range or a commit hash.
    pub source_ref: String,
    pub text: String,
    /// Higher is more relevant.
    pub score: f32,
}

/// A provider of evidence for a target.
pub trait EvidenceSource {
    fn name(&self) -> &str;
    fn collect(&self, target: &ExplainTarget, limit: usize) -> Result<Vec<Evidence>>;
}

/// Assembled evidence ready for rendering.
#[derive(Debug, Clone, PartialEq)]
pub struct ExplainContext {
    pub target: ExplainTarget,
    pub evidence: Vec<Evidence>,
    /// `(source name, reason)` for each source that failed.
    pub skipped_sources: Vec<(String, String)>,
}

/// Builds the context for `target`.
///
/// The code fragment is always first. Other evidence is deduplicated by
/// `(kind, source_ref)` (highest score wins), ordered by score, and cut so the
/// total evidence text stays within `budget_chars`. Failing sources, and a
/// fragment that cannot be read, are recorded in `skipped_sources`.
pub fn assemble(
    target: &ExplainTarget,
    sources: &[&dyn EvidenceSource],
    budget_chars: usize,
) -> ExplainContext {
    let mut skipped_sources = Vec::new();

    let code = match read_fragment(target) {
        Ok(text) => Some(Evidence {
            kind: EvidenceKind::Code,
            source_ref: fragment_ref(target),
            text,
            score: f32::MAX,
        }),
        Err(err) => {
            skipped_sources.push(("code".to_string(), format!("{err:#}")));
            None
        }
    };

    let mut collected = Vec::new();
    for source in sources {
        match source.collect(target, SOURCE_LIMIT) {
            Ok(items) => collected.extend(items),
            Err(err) => skipped_sources.push((source.name().to_string(), format!("{err:#}"))),
        }
    }
    collected.sort_by(|a, b| b.score.total_cmp(&a.score));

    let mut seen = HashSet::new();
    let mut remaining = budget_chars;
    let mut evidence = Vec::new();
    for item in code.into_iter().chain(collected) {
        if !seen.insert((item.kind, item.source_ref.clone())) {
            continue;
        }
        if remaining == 0 {
            break;
        }
        let text: String = item.text.chars().take(remaining).collect();
        remaining -= text.chars().count();
        evidence.push(Evidence { text, ..item });
    }

    ExplainContext {
        target: target.clone(),
        evidence,
        skipped_sources,
    }
}

fn fragment_ref(target: &ExplainTarget) -> String {
    format!(
        "{}:{}-{}",
        target.file.display(),
        target.start_line,
        target.end_line
    )
}

/// Reads the target lines from `repo_root/file`, rejecting paths that escape the repo.
fn read_fragment(target: &ExplainTarget) -> Result<String> {
    ensure!(
        target.start_line >= 1 && target.start_line <= target.end_line,
        "invalid line range {}-{}",
        target.start_line,
        target.end_line
    );
    if !is_repo_relative(&target.file) {
        bail!(
            "file must be relative to the repo: {}",
            target.file.display()
        );
    }
    let path = target.repo_root.join(&target.file);
    let content =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let lines: Vec<&str> = content.lines().collect();
    let end = target.end_line as usize;
    ensure!(
        end <= lines.len(),
        "line range {}-{} is outside {} ({} lines)",
        target.start_line,
        end,
        target.file.display(),
        lines.len()
    );
    Ok(lines[target.start_line as usize - 1..end].join("\n"))
}

fn is_repo_relative(path: &Path) -> bool {
    path.components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeSource {
        name: &'static str,
        items: Vec<Evidence>,
        fail: bool,
    }

    impl EvidenceSource for FakeSource {
        fn name(&self) -> &str {
            self.name
        }

        fn collect(&self, _target: &ExplainTarget, _limit: usize) -> Result<Vec<Evidence>> {
            if self.fail {
                bail!("backend unavailable");
            }
            Ok(self.items.clone())
        }
    }

    fn ev(kind: EvidenceKind, source_ref: &str, text: &str, score: f32) -> Evidence {
        Evidence {
            kind,
            source_ref: source_ref.to_string(),
            text: text.to_string(),
            score,
        }
    }

    /// Creates a repo dir with `src/lib.rs` holding five numbered lines.
    fn fixture() -> (tempfile::TempDir, ExplainTarget) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/lib.rs"),
            "line one\nline two\nline three\nline four\nline five\n",
        )
        .unwrap();
        let target = ExplainTarget {
            repo_root: dir.path().to_path_buf(),
            file: PathBuf::from("src/lib.rs"),
            start_line: 2,
            end_line: 3,
            symbol: Some("demo".to_string()),
        };
        (dir, target)
    }

    #[test]
    fn code_fragment_is_first_and_sources_ordered_by_score() {
        let (_dir, target) = fixture();
        let low = FakeSource {
            name: "low",
            items: vec![ev(EvidenceKind::Memory, "mem-low", "low", 0.1)],
            fail: false,
        };
        let high = FakeSource {
            name: "high",
            items: vec![
                ev(EvidenceKind::Decision, "dec-1", "high", 0.9),
                ev(EvidenceKind::Commit, "abc", "mid", 0.5),
            ],
            fail: false,
        };
        let ctx = assemble(&target, &[&low, &high], 10_000);

        assert!(ctx.skipped_sources.is_empty());
        let kinds: Vec<_> = ctx.evidence.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![
                EvidenceKind::Code,
                EvidenceKind::Decision,
                EvidenceKind::Commit,
                EvidenceKind::Memory
            ]
        );
        assert_eq!(ctx.evidence[0].text, "line two\nline three");
        assert_eq!(ctx.evidence[0].source_ref, "src/lib.rs:2-3");
    }

    #[test]
    fn duplicates_keep_highest_score() {
        let (_dir, target) = fixture();
        let a = FakeSource {
            name: "a",
            items: vec![ev(EvidenceKind::Memory, "same", "weak copy", 0.2)],
            fail: false,
        };
        let b = FakeSource {
            name: "b",
            items: vec![
                ev(EvidenceKind::Memory, "same", "strong copy", 0.8),
                ev(EvidenceKind::Code, "src/lib.rs:2-3", "dup of code", 0.1),
            ],
            fail: false,
        };
        let ctx = assemble(&target, &[&a, &b], 10_000);

        let memories: Vec<_> = ctx
            .evidence
            .iter()
            .filter(|e| e.kind == EvidenceKind::Memory)
            .collect();
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].text, "strong copy");
        // A Code item reusing the fragment's own key is dropped as a duplicate.
        assert_eq!(
            ctx.evidence
                .iter()
                .filter(|e| e.kind == EvidenceKind::Code)
                .count(),
            1
        );
    }

    #[test]
    fn budget_truncates_total_text() {
        let (_dir, target) = fixture();
        let src = FakeSource {
            name: "big",
            items: vec![
                ev(EvidenceKind::Memory, "m1", &"x".repeat(50), 0.9),
                ev(EvidenceKind::Memory, "m2", &"y".repeat(50), 0.8),
            ],
            fail: false,
        };
        // The fragment "line two\nline three" is 19 chars, leaving 11 for memories.
        let ctx = assemble(&target, &[&src], 30);

        let total: usize = ctx.evidence.iter().map(|e| e.text.chars().count()).sum();
        assert_eq!(total, 30);
        assert_eq!(ctx.evidence[0].kind, EvidenceKind::Code);
        assert_eq!(ctx.evidence.len(), 2);
        assert_eq!(ctx.evidence[1].text, "x".repeat(11));
    }

    #[test]
    fn failing_source_is_recorded_and_others_still_used() {
        let (_dir, target) = fixture();
        let broken = FakeSource {
            name: "broken",
            items: vec![],
            fail: true,
        };
        let good = FakeSource {
            name: "good",
            items: vec![ev(EvidenceKind::CallGraph, "caller", "called by x", 0.4)],
            fail: false,
        };
        let ctx = assemble(&target, &[&broken, &good], 10_000);

        assert_eq!(ctx.skipped_sources.len(), 1);
        assert_eq!(ctx.skipped_sources[0].0, "broken");
        assert!(ctx.skipped_sources[0].1.contains("backend unavailable"));
        assert!(ctx
            .evidence
            .iter()
            .any(|e| e.kind == EvidenceKind::CallGraph));
    }

    #[test]
    fn unreadable_fragment_is_recorded_not_fatal() {
        let (_dir, mut target) = fixture();
        target.file = PathBuf::from("missing.rs");
        let good = FakeSource {
            name: "good",
            items: vec![ev(EvidenceKind::Memory, "m", "still here", 0.3)],
            fail: false,
        };
        let ctx = assemble(&target, &[&good], 10_000);

        assert_eq!(ctx.skipped_sources[0].0, "code");
        assert_eq!(ctx.evidence.len(), 1);
        assert_eq!(ctx.evidence[0].kind, EvidenceKind::Memory);
    }

    #[test]
    fn rejects_paths_escaping_the_repo() {
        let (_dir, mut target) = fixture();
        target.file = PathBuf::from("../outside.rs");
        let ctx = assemble(&target, &[], 100);

        assert!(ctx.evidence.is_empty());
        assert!(ctx.skipped_sources[0].1.contains("relative to the repo"));
    }
}
