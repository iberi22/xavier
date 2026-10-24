//! Commit history of a line range, taken from `git log -L`.

use std::path::{Path, PathBuf};
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
        // The workspace may hold several repos; git must run in the one that owns the file.
        let repo = locate_repo(&target.repo_root, &target.file)?;
        let mut cmd = Command::new("git");
        clear_git_env(&mut cmd);
        let output = cmd
            .current_dir(&repo.toplevel)
            .arg("log")
            .arg(format!(
                "-L{},{}:{}",
                target.start_line,
                target.end_line,
                repo.file.display()
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

/// The git repository that contains a file of the workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RepoLocation {
    /// Absolute path of the repo's top-level directory.
    pub toplevel: PathBuf,
    /// The file relative to `toplevel`, in the form git and the code graph use.
    pub file: PathBuf,
    /// The top-level relative to the workspace root, e.g. `apps/xavier`; `.` for the workspace itself.
    /// Outside the workspace it is the absolute top-level path.
    pub name: String,
}

/// Finds the innermost git repo containing `workspace/file`, searching from the file's directory.
pub(crate) fn locate_repo(workspace: &Path, file: &Path) -> Result<RepoLocation> {
    let absolute = workspace
        .join(file)
        .canonicalize()
        .with_context(|| format!("resolving {}", file.display()))?;
    let dir = absolute
        .parent()
        .with_context(|| format!("{} has no parent directory", file.display()))?;

    let mut cmd = Command::new("git");
    clear_git_env(&mut cmd);
    let output = cmd
        .current_dir(dir)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("running git rev-parse")?;
    if !output.status.success() {
        bail!("{} is not inside a git repository", file.display());
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    let toplevel = PathBuf::from(raw.trim_end_matches(['\r', '\n']))
        .canonicalize()
        .context("resolving the repo top-level")?;
    let relative = absolute
        .strip_prefix(&toplevel)
        .with_context(|| format!("{} is outside its repo", file.display()))?
        .to_path_buf();

    let root = workspace
        .canonicalize()
        .context("resolving the workspace root")?;
    let name = match toplevel.strip_prefix(&root) {
        Ok(inside) if inside.as_os_str().is_empty() => ".".to_string(),
        Ok(inside) => inside.display().to_string(),
        Err(_) => toplevel.display().to_string(),
    };

    Ok(RepoLocation {
        toplevel,
        file: relative,
        name,
    })
}

/// Removes every `GIT_*` variable. Git hooks export repo overrides (GIT_DIR,
/// GIT_COMMON_DIR, GIT_CONFIG*, ...) that would otherwise redirect our git calls.
pub(crate) fn clear_git_env(cmd: &mut Command) {
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

    /// Writes `lib.rs` in `repo` and commits it, initialising the repo on first use.
    fn commit_lib(repo: &Path, content: &str, message: &str) {
        std::fs::create_dir_all(repo).unwrap();
        if !repo.join(".git").exists() {
            git(repo, &["init", "-q"]);
            git(repo, &["config", "user.name", "Test Author"]);
            git(repo, &["config", "user.email", "test@example.invalid"]);
            git(repo, &["config", "commit.gpgsign", "false"]);
        }
        std::fs::write(repo.join("lib.rs"), content).unwrap();
        git(repo, &["add", "lib.rs"]);
        git(repo, &["commit", "-q", "-m", message]);
    }

    #[test]
    fn explained_file_uses_commits_of_its_own_repo_only() {
        let workspace = tempfile::tempdir().unwrap();
        let repo_a = workspace.path().join("a");
        let repo_b = workspace.path().join("b");
        commit_lib(&repo_a, "fn a() {}\n", "repo a first");
        commit_lib(&repo_a, "fn a() { 1 }\n", "repo a tweak");
        commit_lib(&repo_b, "fn b() {}\n", "repo b first");
        commit_lib(&repo_b, "fn b() { 2 }\n", "repo b tweak");

        let target = ExplainTarget {
            repo_root: workspace.path().to_path_buf(),
            file: PathBuf::from("b/lib.rs"),
            start_line: 1,
            end_line: 1,
            symbol: None,
        };
        let evidence = GitHistorySource.collect(&target, 10).unwrap();

        assert_eq!(evidence.len(), 2);
        assert!(evidence[0].text.ends_with("repo b tweak"));
        assert!(evidence[1].text.ends_with("repo b first"));
        assert!(!evidence.iter().any(|e| e.text.contains("repo a")));
    }

    #[test]
    fn locate_repo_names_the_innermost_repo() {
        let workspace = tempfile::tempdir().unwrap();
        let nested = workspace.path().join("apps").join("xavier");
        commit_lib(&nested, "fn x() {}\n", "init");
        std::fs::create_dir_all(nested.join("src")).unwrap();
        std::fs::write(nested.join("src/lib.rs"), "fn y() {}\n").unwrap();

        let repo = locate_repo(workspace.path(), Path::new("apps/xavier/src/lib.rs")).unwrap();

        assert_eq!(repo.name, "apps/xavier");
        assert_eq!(repo.file, PathBuf::from("src/lib.rs"));
        assert_eq!(
            repo.toplevel,
            nested.canonicalize().unwrap(),
            "toplevel must be the nested repo"
        );
    }
}
