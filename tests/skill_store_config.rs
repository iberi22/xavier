//! Integration: `SkillRegistry::with_defaults` honours `XAVIER_SKILL_STORE`.
//! Uses tempdirs only; HOME and the store variable are restored after each test.

use std::ffi::OsString;
use std::path::Path;
use xavier::context::SkillRegistry;

xavier::isolate_test_process!();

static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct EnvGuard(Vec<(&'static str, Option<OsString>)>);

impl EnvGuard {
    fn capture(keys: &[&'static str]) -> Self {
        Self(keys.iter().map(|k| (*k, std::env::var_os(k))).collect())
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (k, v) in &self.0 {
            match v {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
    }
}

fn write_skill(root: &Path, name: &str) {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: \"Skill {name} for testing\"\n---\n\n# {name}\n"),
    )
    .unwrap();
}

struct Fixture {
    workspace: tempfile::TempDir,
    home: tempfile::TempDir,
    store: tempfile::TempDir,
}

fn fixture() -> Fixture {
    let f = Fixture {
        workspace: tempfile::tempdir().unwrap(),
        home: tempfile::tempdir().unwrap(),
        store: tempfile::tempdir().unwrap(),
    };
    write_skill(&f.workspace.path().join("skills"), "scan-ws-skills");
    write_skill(
        &f.workspace.path().join(".agents").join("skills"),
        "scan-ws-agents",
    );
    write_skill(&f.home.path().join(".hermes").join("skills"), "scan-hermes");
    write_skill(f.store.path(), "scan-override");
    f
}

async fn indexed(workspace: &Path) -> Vec<String> {
    let mut registry = SkillRegistry::with_defaults(workspace);
    registry.reindex().await.unwrap();
    let mut names: Vec<String> = registry.list().into_iter().map(String::from).collect();
    names.sort();
    names
}

#[tokio::test]
async fn unset_store_scans_workspace_roots_and_hermes_store() {
    let _lock = ENV_LOCK.lock().await;
    let _env = EnvGuard::capture(&["HOME", "XAVIER_SKILL_STORE"]);
    let f = fixture();
    std::env::set_var("HOME", f.home.path());
    std::env::remove_var("XAVIER_SKILL_STORE");

    assert_eq!(
        indexed(f.workspace.path()).await,
        vec!["scan-hermes", "scan-ws-agents", "scan-ws-skills"]
    );
}

#[tokio::test]
async fn blank_store_is_treated_as_unset() {
    let _lock = ENV_LOCK.lock().await;
    let _env = EnvGuard::capture(&["HOME", "XAVIER_SKILL_STORE"]);
    let f = fixture();
    std::env::set_var("HOME", f.home.path());
    std::env::set_var("XAVIER_SKILL_STORE", "   ");

    assert_eq!(
        indexed(f.workspace.path()).await,
        vec!["scan-hermes", "scan-ws-agents", "scan-ws-skills"]
    );
}

#[tokio::test]
async fn set_store_replaces_hermes_store_and_keeps_workspace_roots() {
    let _lock = ENV_LOCK.lock().await;
    let _env = EnvGuard::capture(&["HOME", "XAVIER_SKILL_STORE"]);
    let f = fixture();
    std::env::set_var("HOME", f.home.path());
    std::env::set_var("XAVIER_SKILL_STORE", f.store.path());

    assert_eq!(
        indexed(f.workspace.path()).await,
        vec!["scan-override", "scan-ws-agents", "scan-ws-skills"]
    );
}

#[tokio::test]
async fn set_store_works_without_home() {
    let _lock = ENV_LOCK.lock().await;
    let _env = EnvGuard::capture(&["HOME", "XAVIER_SKILL_STORE"]);
    let f = fixture();
    std::env::remove_var("HOME");
    std::env::set_var("XAVIER_SKILL_STORE", f.store.path());

    assert_eq!(
        indexed(f.workspace.path()).await,
        vec!["scan-override", "scan-ws-agents", "scan-ws-skills"]
    );
}
