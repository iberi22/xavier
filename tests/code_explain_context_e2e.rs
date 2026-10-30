//! feat-code-explain-context: End-to-end integration tests for code explain context.

xavier::isolate_test_process!();

use std::path::PathBuf;
use std::process::Command;

use xavier::explain::{assemble, EvidenceKind, EvidenceSource, ExplainTarget, GitHistorySource};

/// Clears git environment variables to avoid interference from outer test harnesses
/// when running git inside our temporary repositories.
fn clear_git_env(cmd: &mut Command) {
    for (k, _) in std::env::vars() {
        if k.starts_with("GIT_") || k == "SSH_AUTH_SOCK" {
            cmd.env_remove(&k);
        }
    }
    // Ignore the developer's global/system git config (hooks, signing,
    // templates) so the fixture repo is hermetic.
    cmd.env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0");
}

/// Sets up a temporary git repository with one file and returns its path and a target.
fn setup_temp_repo_with_file(
    content: &str,
    start_line: u32,
    end_line: u32,
) -> (tempfile::TempDir, ExplainTarget) {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo_root = dir.path().to_path_buf();

    // Init git repo
    let mut cmd = Command::new("git");
    clear_git_env(&mut cmd);
    let status = cmd
        .arg("init")
        .current_dir(&repo_root)
        .status()
        .expect("git init");
    assert!(status.success(), "git init failed");

    // Configure user
    let mut cmd = Command::new("git");
    clear_git_env(&mut cmd);
    cmd.args(["config", "user.name", "Test User"])
        .current_dir(&repo_root)
        .status()
        .expect("git config user.name");

    let mut cmd = Command::new("git");
    clear_git_env(&mut cmd);
    cmd.args(["config", "user.email", "test@example.com"])
        .current_dir(&repo_root)
        .status()
        .expect("git config user.email");

    // Create file
    let file_rel = PathBuf::from("src/lib.rs");
    let file_abs = repo_root.join(&file_rel);
    std::fs::create_dir_all(file_abs.parent().unwrap()).unwrap();
    std::fs::write(&file_abs, content).expect("write file");

    // Commit file
    let mut cmd = Command::new("git");
    clear_git_env(&mut cmd);
    let status = cmd
        .args(["add", "src/lib.rs"])
        .current_dir(&repo_root)
        .status()
        .expect("git add");
    assert!(status.success(), "git add failed");

    let mut cmd = Command::new("git");
    clear_git_env(&mut cmd);
    let status = cmd
        .args(["commit", "-m", "Initial commit"])
        .current_dir(&repo_root)
        .status()
        .expect("git commit");
    assert!(status.success(), "git commit failed");

    let target = ExplainTarget {
        repo_root,
        file: file_rel,
        start_line,
        end_line,
        symbol: None,
    };

    (dir, target)
}

#[test]
#[serial_test::serial]
fn code_explain_context_assembles_evidence_from_code_and_git_history() {
    let file_content = "pub fn test_fn() {\n    println!(\"hello\");\n}\n";
    let (_temp_dir, target) = setup_temp_repo_with_file(file_content, 1, 3);

    let history_source = Box::new(GitHistorySource) as Box<dyn EvidenceSource + Send + Sync>;
    let sources: Vec<&dyn EvidenceSource> = vec![history_source.as_ref()];

    // Assemble the context
    let budget_chars = 1000;
    let ctx = assemble(&target, &sources, budget_chars);

    // Verify context contains exactly two pieces of evidence: code and commit history
    assert_eq!(ctx.target, target);
    assert_eq!(ctx.skipped_sources.len(), 0, "No sources should be skipped");

    // Check that we have evidence from the code source (which is always included)
    let code_evidence = ctx
        .evidence
        .iter()
        .find(|e| e.kind == EvidenceKind::Code)
        .expect("Code evidence missing");
    assert_eq!(code_evidence.source_ref, "src/lib.rs:1-3");
    assert_eq!(code_evidence.text, file_content.trim_end());
    assert_eq!(code_evidence.score, f32::MAX);

    // Check that we have evidence from the git history source
    let commit_evidence = ctx
        .evidence
        .iter()
        .find(|e| e.kind == EvidenceKind::Commit)
        .expect("Commit evidence missing");
    assert_eq!(commit_evidence.source_ref.len(), 40);
    assert!(commit_evidence.text.contains("Initial commit"));
}

#[test]
#[serial_test::serial]
fn code_explain_context_handles_missing_git_repo_gracefully() {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo_root = dir.path().to_path_buf();

    let file_rel = PathBuf::from("src/lib.rs");
    let file_abs = repo_root.join(&file_rel);
    std::fs::create_dir_all(file_abs.parent().unwrap()).unwrap();
    std::fs::write(&file_abs, "pub fn foo() {}").unwrap();

    let target = ExplainTarget {
        repo_root,
        file: file_rel,
        start_line: 1,
        end_line: 1,
        symbol: None,
    };

    let history_source = Box::new(GitHistorySource) as Box<dyn EvidenceSource + Send + Sync>;
    let sources: Vec<&dyn EvidenceSource> = vec![history_source.as_ref()];

    let budget_chars = 1000;
    let ctx = assemble(&target, &sources, budget_chars);

    // The code evidence should still be there
    let code_evidence = ctx
        .evidence
        .iter()
        .find(|e| e.kind == EvidenceKind::Code)
        .expect("Code evidence missing");
    assert_eq!(code_evidence.text, "pub fn foo() {}");

    // The git history source should fail and be skipped since it's not a git repo
    assert!(ctx.evidence.iter().all(|e| e.kind != EvidenceKind::Commit));

    let skipped_git = ctx
        .skipped_sources
        .iter()
        .find(|(name, _)| name == "git-history")
        .expect("git-history should be skipped");
    assert!(skipped_git.1.contains("is not inside a git repository"));
}
