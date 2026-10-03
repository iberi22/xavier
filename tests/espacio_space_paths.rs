//! A space workspace keeps EVERY file it creates inside its own directory.
//!
//! Own test binary (single test) because it points the process-global `HOME`,
//! cwd and data-dir env at scratch dirs, then asserts that those stay empty
//! while the space's files land under `{space}/`. Deleting the space moves
//! all of it, conversations database included, to the trash.

use std::path::{Path, PathBuf};

use xavier::espacio::SpaceManager;
use xavier::workspace::{invalidate_space_workspace, space_workspace_context};

fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        if let Ok(rd) = std::fs::read_dir(&d) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    out.push(p);
                }
            }
        }
    }
    out
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_workspace_files_stay_inside_the_space_dir_and_follow_it_to_trash() {
    let scratch = tempfile::tempdir().unwrap();
    let home = scratch.path().join("home");
    let cwd = scratch.path().join("cwd");
    let data = scratch.path().join("data");
    let state = scratch.path().join("state");
    for d in [&home, &cwd, &data, &state] {
        std::fs::create_dir_all(d).unwrap();
    }
    // Everything a stray default path could resolve to points at scratch dirs
    // that must stay empty.
    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_DATA_HOME", home.join(".local/share"));
    std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
    std::env::set_var("XAVIER_DATA_DIR", &data);
    std::env::set_var("XAVIER_STATE_DIR", &data);
    std::env::set_var("XAVIER_WORKSPACE_DIR", &data);
    std::env::set_var("XAVIER_EMBEDDING_PROVIDER_MODE", "disabled");
    std::env::set_current_dir(&cwd).unwrap();

    let manager = SpaceManager::open(&state);
    manager
        .create("esp_p".into(), "p".into(), "".into(), "owner".into(), false)
        .await
        .unwrap();
    let ctx = space_workspace_context(&manager, "esp_p").await.unwrap();
    ctx.workspace
        .conversations_db
        .create_thread(Some("t"), None, None)
        .await
        .unwrap();
    let _ = ctx.workspace.memory.search("anything", 3).await;
    // Let the workspace's background tasks run.
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

    for (label, dir) in [("HOME", &home), ("cwd", &cwd), ("data dir", &data)] {
        let stray = files_under(dir);
        assert!(
            stray.is_empty(),
            "space wrote outside its dir into {label}: {stray:?}"
        );
    }
    let space_dir = state.join("spaces/esp_p");
    let inside: Vec<_> = files_under(&space_dir)
        .into_iter()
        .map(|p| {
            p.strip_prefix(&space_dir)
                .unwrap()
                .to_string_lossy()
                .to_string()
        })
        .collect();
    assert!(inside.iter().any(|f| f == "conversations.db"), "{inside:?}");
    assert!(inside.iter().any(|f| f == "memory.sqlite"), "{inside:?}");
    // Nothing else under the state root except the other espacio files.
    for f in files_under(&state) {
        assert!(
            f.starts_with(&space_dir),
            "file outside the space dir: {f:?}"
        );
    }

    // Deleting the space trashes the conversations database with it.
    invalidate_space_workspace("esp_p").await;
    drop(ctx);
    manager.delete("esp_p").await.unwrap();
    assert!(!space_dir.exists());
    let trashed: Vec<_> = std::fs::read_dir(state.join("spaces/.trash"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    assert_eq!(trashed.len(), 1, "{trashed:?}");
    assert!(trashed[0].join("conversations.db").is_file());
    assert!(trashed[0].join("memory.sqlite").is_file());
    for (label, dir) in [("HOME", &home), ("cwd", &cwd), ("data dir", &data)] {
        let stray = files_under(dir);
        assert!(
            stray.is_empty(),
            "stray files in {label} after delete: {stray:?}"
        );
    }
}
