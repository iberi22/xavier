//! Commit history of a line range, taken from `git log -L`.

use std::process::Command;

use anyhow::{bail, Context, Result};

use super::context::{Evidence, EvidenceKind, EvidenceSource, ExplainTarget};

/// Field separator used in the `git log` format (ASCII unit separator).
const FIELD_SEP: char = '\x1f';

/// Evidence source listing the commits that touched the target lines, newest first.
#[derive(Debug, Default, Clone, Copy)]
pub struct GitHistorySource;

impl EvidenceSource for GitHistorySource {
    fn name(&self) -> &str {
        "git-history"
    }

    fn collect(&self, target: &ExplainTarget, limit: usize) -> Result<Vec<Evidence>> {
        let mut cmd = Command::new("git");
        clear_git_env(&mut cmd);
        let output = cmd
            .current_dir(&target.repo_root)
            .arg("log")
            .arg(format!(
                "-L{},{}:{}",
                target.start_line,
                target.end_line,
                target.file.display()
            ))
            .arg("--format=%H%x1f%ad%x1f%s")
            .arg("--date=short")
            .arg("--no-patch")
            .arg("-n")
            .arg(limit.to_string())
            .output()
            .context("running git log")?;

        if !output.status.success() {
            bail!(
                "git log failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(parse_log(&stdout))
    }
}

/// Removes every `GIT_*` variable. Git hooks export repo overrides (GIT_DIR,
/// GIT_COMMON_DIR, GIT_CONFIG*, ...) that would otherwise redirect our git calls.
fn clear_git_env(cmd: &mut Command) {
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            cmd.env_remove(key);
        }
    }
}

/// Parses `hash\x1fdate\x1fsubject` lines into commit evidence.
fn parse_log(stdout: &str) -> Vec<Evidence> {
    stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .enumerate()
        .filter_map(|(rank, line)| {
            let mut fields = line.splitn(3, FIELD_SEP);
            let hash = fields.next()?.trim();
            let date = fields.next()?;
            let subject = fields.next()?;
            Some(Evidence {
                kind: EvidenceKind::Commit,
                source_ref: hash.to_string(),
                text: format!("{date} {subject}"),
                score: 1.0 / (rank as f32 + 1.0),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    fn git(repo: &Path, args: &[&str]) {
        let mut cmd = Command::new("git");
        clear_git_env(&mut cmd);
        let status = cmd
            .current_dir(repo)
            .args(["-c", "core.hooksPath=/dev/null"])
            .args(args)
            .status()
            .expect("git must be installed");
        assert!(status.success(), "git {args:?} failed");
    }

    #[test]
    fn lists_commits_touching_the_range_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        git(repo, &["init", "-q"]);
        git(repo, &["config", "user.name", "Test Author"]);
        git(repo, &["config", "user.email", "test@example.invalid"]);
        git(repo, &["config", "commit.gpgsign", "false"]);

        std::fs::write(repo.join("lib.rs"), "fn a() {}\nfn b() {}\n").unwrap();
        git(repo, &["add", "lib.rs"]);
        git(repo, &["commit", "-q", "-m", "first version"]);

        std::fs::write(repo.join("lib.rs"), "fn a() { 1 }\nfn b() {}\n").unwrap();
        git(repo, &["commit", "-q", "-am", "tweak a"]);

        let target = ExplainTarget {
            repo_root: repo.to_path_buf(),
            file: PathBuf::from("lib.rs"),
            start_line: 1,
            end_line: 1,
            symbol: None,
        };
        let evidence = GitHistorySource.collect(&target, 10).unwrap();

        assert_eq!(evidence.len(), 2);
        assert!(evidence.iter().all(|e| e.kind == EvidenceKind::Commit));
        assert!(evidence[0].text.ends_with("tweak a"));
        assert!(evidence[1].text.ends_with("first version"));
        assert_eq!(evidence[0].source_ref.len(), 40);
        assert!(evidence[0].score > evidence[1].score);
    }

    #[test]
    fn non_repo_directory_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("lib.rs"), "fn a() {}\n").unwrap();
        let target = ExplainTarget {
            repo_root: dir.path().to_path_buf(),
            file: PathBuf::from("lib.rs"),
            start_line: 1,
            end_line: 1,
            symbol: None,
        };

        assert!(GitHistorySource.collect(&target, 5).is_err());
    }
}
