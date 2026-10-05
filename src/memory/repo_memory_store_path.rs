//! Per-repo MEMORY store path resolution (XAV-REPO).
//!
//! This is the memory counterpart of
//! [`crate::codebase::codegraph_paths`]: the code graph already resolves a
//! per-repo SQLite file (`<repo>/.xavier/code_graph.db`), and this module
//! resolves the analogous *memory* file (`<repo>/.xavier/memory.sqlite3`)
//! without putting memory concerns into the codegraph module.
//!
//! # What is done here, and what is NOT (read this before assuming isolation)
//!
//! **Done:** the *route*. For a given repository root (or a given
//! [`RepoIdentity`]) this module says which file the memory store must live in,
//! and two different repositories therefore resolve to two different files.
//!
//! **NOT done:** the *physical isolation* of the data. As of this commit the
//! daemon still opens ONE global store
//! (`<data_dir>/vec-store.sqlite3`, see
//! [`VecSqliteStoreConfig::from_env`](crate::memory::sqlite_vec_store::config::VecSqliteStoreConfig::from_env)),
//! and every row inside it — `memory_records`, `memory_fts`,
//! `memory_embeddings_768`, `timeline_events` — is multiplexed by the
//! `workspace_id` column. Nothing in this module moves a row, copies a row, or
//! splits that database. Reading a per-repo path here as "this repo's data is
//! isolated" would be false today.
//!
//! Making the isolation physical means opening a separate SQLite file per repo
//! and dropping the `workspace_id` multiplexing (or keeping it as a
//! within-repo tag). That is follow-up work on top of this route; this module
//! is the first step and is deliberately additive so no existing memory is
//! read from a different file than the one it was written to.
//!
//! # Why two functions
//!
//! The existing codegraph path resolution is deliberately split in two, and so
//! is this:
//!
//! - [`memory_store_path_for`] mirrors
//!   [`code_graph_db_path_for`](crate::codebase::codegraph_paths::code_graph_db_path_for):
//!   it is cwd-aware, so the directory the process was started in (the daemon's
//!   own repo, or `.`) keeps using the daemon `data_dir`.
//! - [`memory_store_path_for_root`] mirrors
//!   [`code_graph_db_path_for_root`](crate::codebase::repo_identity::code_graph_db_path_for_root):
//!   it is unconditional, because a request that carries an explicit
//!   `RepoIdentity` must never collapse onto the daemon's own store. That is the
//!   property that keeps two checkouts apart.

use std::path::{Component, Path, PathBuf};

use crate::codebase::repo_identity::RepoIdentity;

/// Directory inside a repository that holds the whole per-repo Xavier package
/// (the same directory `repo_config.rs` writes `config.toml` into).
pub const REPO_PACKAGE_DIR: &str = ".xavier";

/// File name of the per-repo memory store inside [`REPO_PACKAGE_DIR`].
///
/// Deliberately NOT `vec-store.sqlite3` / `xavier_memory_vec.db`: those are the
/// names of the *global* store and of the legacy default. A per-repo file is a
/// new artefact, so nothing that already exists can be opened by accident as if
/// it were a per-repo store.
pub const REPO_MEMORY_DB_FILENAME: &str = "memory.sqlite3";

/// Env override, the memory analogue of `XAVIER_CODE_GRAPH_DB_PATH`.
pub const REPO_MEMORY_DB_PATH_ENV: &str = "XAVIER_REPO_MEMORY_DB_PATH";

/// Lexically normalise a path: make it absolute against the cwd, then drop `.`
/// and resolve `..` textually.
///
/// `canonicalize()` is not enough and not used here: it fails on a path that
/// does not exist yet (a repo the user has not indexed, which is a normal
/// first-run state) and it resolves symlinks, which would make the path depend
/// on where `/tmp` happens to point. Two spellings of the same repository must
/// resolve to the same store file, and no path may need to exist for that.
fn normalize_repo_root(repo_root: &Path) -> PathBuf {
    let absolute = if repo_root.as_os_str().is_empty() {
        std::path::absolute(Path::new(".")).unwrap_or_else(|_| PathBuf::from("."))
    } else {
        std::path::absolute(repo_root).unwrap_or_else(|_| repo_root.to_path_buf())
    };

    let mut out = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                // Never pop past an absolute root: `/..` is `/`.
                if !out.pop() {
                    out.push(Component::RootDir.as_os_str());
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    out
}

/// True when `repo_root` denotes the repository the daemon itself runs in (or
/// the ambiguous `.`), i.e. the case that falls back to the daemon `data_dir`.
fn is_daemon_repo(repo_root: &Path) -> bool {
    if repo_root.as_os_str().is_empty() || repo_root == Path::new(".") {
        return true;
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    if cwd.as_os_str().is_empty() {
        return false;
    }
    normalize_repo_root(repo_root) == normalize_repo_root(&cwd)
}

/// Path to the memory store for `repo_root`, cwd-aware.
///
/// Same fallback policy as
/// [`code_graph_db_path_for`](crate::codebase::codegraph_paths::code_graph_db_path_for):
///
/// 1. `XAVIER_REPO_MEMORY_DB_PATH` when set to a non-blank value — the explicit
///    override always wins, so an operator can point a repo at a store
///    elsewhere.
/// 2. the daemon's own repo, spelled as `.`, as an empty path, or as the
///    process cwd → [`XavierSettings::resolve_data_dir`](crate::settings::XavierSettings::resolve_data_dir)
///    joined with [`REPO_MEMORY_DB_FILENAME`], i.e. the same directory that
///    holds the global store today.
/// 3. every other repository → `<repo>/.xavier/memory.sqlite3`, next to its own
///    `code_graph.db`.
///
/// The file is not created here, and the parent directory is not created here:
/// this is a pure resolution function, callable from a test with no I/O.
pub fn memory_store_path_for(repo_root: &Path) -> PathBuf {
    if let Ok(override_path) = std::env::var(REPO_MEMORY_DB_PATH_ENV) {
        if !override_path.trim().is_empty() {
            return PathBuf::from(override_path);
        }
    }
    if is_daemon_repo(repo_root) {
        return crate::settings::XavierSettings::resolve_data_dir().join(REPO_MEMORY_DB_FILENAME);
    }
    memory_store_path_for_root(repo_root)
}

/// Unconditional per-repo memory store path: `<repo>/.xavier/memory.sqlite3`.
///
/// Deliberately independent of the process cwd and of
/// [`REPO_MEMORY_DB_PATH_ENV`], exactly like
/// [`code_graph_db_path_for_root`](crate::codebase::repo_identity::code_graph_db_path_for_root)
/// is independent of `XAVIER_CODE_GRAPH_DB_PATH`: a request that names a repo
/// resolves to that repo's file, never to another one.
pub fn memory_store_path_for_root(repo_root: &Path) -> PathBuf {
    normalize_repo_root(repo_root)
        .join(REPO_PACKAGE_DIR)
        .join(REPO_MEMORY_DB_FILENAME)
}

/// Path to the memory store for a request that carries an explicit
/// [`RepoIdentity`].
///
/// `project_id` is intentionally *not* part of the file name: the identity's
/// `root` already determines the location, and two spellings of one repo must
/// not produce two files. The project id is what tells two products inside one
/// monorepo apart, and that separation happens because each product carries its
/// own `root` (and its own `.xavier/config.toml`), not because the id is
/// encoded in a path.
pub fn memory_store_path_for_identity(identity: &RepoIdentity) -> PathBuf {
    memory_store_path_for_root(Path::new(&identity.root))
}

/// The store the daemon must open when it wants the per-repo store of
/// `repo_root`.
///
/// A thin, explicit wrapper over [`memory_store_path_for`] so a caller that
/// *is* selecting (rather than merely reporting a path) reads as a selection.
/// Note it returns the path only: opening a second store for the same process
/// is the follow-up work described in the module docs, not this function's job.
pub fn select_memory_store_path(repo_root: &Path) -> PathBuf {
    memory_store_path_for(repo_root)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mirrors `codegraph_paths.rs`: serialise against the tests that mutate
    /// the override env var.
    fn temp_env() -> crate::settings::tests::TempEnv {
        std::env::remove_var(REPO_MEMORY_DB_PATH_ENV);
        crate::settings::tests::TempEnv::new()
    }

    fn fake_repo(root: &Path) -> PathBuf {
        std::fs::create_dir_all(root.join(REPO_PACKAGE_DIR)).expect("mkdir .xavier");
        std::fs::create_dir_all(root.join(".git")).expect("mkdir .git");
        root.canonicalize().unwrap_or_else(|_| root.to_path_buf())
    }

    #[test]
    fn repo_a_and_repo_b_resolve_to_different_stores() {
        let _env = temp_env();
        let tmp = tempfile::tempdir().expect("tempdir");
        let a = fake_repo(&tmp.path().join("repo-alpha-xavrepo"));
        let b = fake_repo(&tmp.path().join("repo-beta-xavrepo"));

        let store_a = memory_store_path_for_root(&a);
        let store_b = memory_store_path_for_root(&b);
        assert_ne!(store_a, store_b, "two repos must not share a memory file");
        assert!(store_a.ends_with("repo-alpha-xavrepo/.xavier/memory.sqlite3"));
        assert!(store_b.ends_with("repo-beta-xavrepo/.xavier/memory.sqlite3"));

        // And they do not collide with the codegraph file of the same repo.
        let graph_a = crate::codebase::repo_identity::code_graph_db_path_for_root(&a);
        assert_ne!(store_a, graph_a, "memory and codegraph are separate files");
    }

    #[test]
    fn explicit_repo_path_is_repo_local_and_ignores_cwd() {
        let _env = temp_env();
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = fake_repo(&tmp.path().join("repo-gamma-xavrepo"));
        let path = memory_store_path_for(&repo);
        assert_eq!(path, memory_store_path_for_root(&repo));
        assert_eq!(
            path,
            repo.join(REPO_PACKAGE_DIR).join(REPO_MEMORY_DB_FILENAME)
        );
    }

    #[test]
    fn daemon_repo_resolves_to_data_dir() {
        let _env = temp_env();
        let expected =
            crate::settings::XavierSettings::resolve_data_dir().join(REPO_MEMORY_DB_FILENAME);
        assert_eq!(memory_store_path_for(Path::new(".")), expected);
        assert_eq!(memory_store_path_for(Path::new("")), expected);
        assert_eq!(select_memory_store_path(Path::new(".")), expected);
    }

    #[test]
    fn env_override_wins_and_blank_override_is_ignored() {
        let _env = temp_env();
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = fake_repo(&tmp.path().join("repo-override-xavrepo"));

        std::env::set_var(REPO_MEMORY_DB_PATH_ENV, "/tmp/override-memory.sqlite3");
        assert_eq!(
            memory_store_path_for(&repo),
            PathBuf::from("/tmp/override-memory.sqlite3")
        );

        std::env::set_var(REPO_MEMORY_DB_PATH_ENV, "   ");
        assert_eq!(
            memory_store_path_for(&repo),
            memory_store_path_for_root(&repo),
            "a blank override must not produce a file literally named with spaces"
        );
    }

    /// Spaces and odd characters are legal in a directory name and must survive
    /// intact, while `.`/`..` and duplicated separators must be normalised so
    /// two spellings of one repo share one file.
    #[test]
    fn awkward_repo_paths_normalise_correctly() {
        let _env = temp_env();
        let tmp = tempfile::tempdir().expect("tempdir");
        let spaced = fake_repo(&tmp.path().join("repo with spaces & sym#bols"));
        let spaced_path = memory_store_path_for_root(&spaced);
        assert_eq!(
            spaced_path.parent().unwrap().parent().unwrap(),
            spaced.as_path(),
            "spaces and unusual characters are preserved verbatim"
        );
        assert_eq!(spaced_path.file_name().unwrap(), "memory.sqlite3");

        // Non-normalised spellings of the same repo collapse to one file.
        let plain = fake_repo(&tmp.path().join("repo-plain-xavrepo"));
        let noisy = plain.join(".").join("src").join("..");
        assert_eq!(
            memory_store_path_for_root(&noisy),
            memory_store_path_for_root(&plain),
            "`.` and `..` must not fork the store into two files"
        );
    }

    /// A path that does not exist must still resolve, without panicking and
    /// without borrowing an ancestor repo's store — the `repo_identity` rule.
    #[test]
    fn missing_and_hostile_inputs_never_panic() {
        let _env = temp_env();
        for raw in [
            "",
            ".",
            "..",
            "../..",
            "/",
            "/does/not/exist/repo-xavrepo",
            "a/b/c",
        ] {
            let path = memory_store_path_for(Path::new(raw));
            assert_eq!(
                path.file_name().and_then(|n| n.to_str()),
                Some(REPO_MEMORY_DB_FILENAME),
                "raw {raw:?} resolved to {}",
                path.display()
            );
        }
        // A non-UTF-8 root is also only a path, never a panic.
        #[cfg(unix)]
        {
            use std::ffi::OsStr;
            use std::os::unix::ffi::OsStrExt;
            let raw = OsStr::from_bytes(b"/tmp/repo-\xff\xfe-xavrepo");
            let path = memory_store_path_for_root(Path::new(raw));
            assert_eq!(path.file_name().unwrap(), REPO_MEMORY_DB_FILENAME);
        }
    }

    /// The request-facing selector: given a `RepoIdentity`, the store is that
    /// repo's own file, and the daemon's own repo identity does not redirect an
    /// external repo onto the global store.
    #[test]
    fn identity_selection_is_per_repo_and_does_not_leak() {
        let _env = temp_env();
        let tmp = tempfile::tempdir().expect("tempdir");
        let a = fake_repo(&tmp.path().join("repo-identity-a"));
        let b = fake_repo(&tmp.path().join("repo-identity-b"));

        let id_a = crate::codebase::repo_identity::derive_repo_identity(&a);
        let id_b = crate::codebase::repo_identity::derive_repo_identity(&b);
        assert_ne!(id_a.project_id, id_b.project_id);

        let store_a = memory_store_path_for_identity(&id_a);
        let store_b = memory_store_path_for_identity(&id_b);
        assert_ne!(store_a, store_b);
        assert!(store_a.starts_with(&a));
        assert!(store_b.starts_with(&b));

        // Selecting from an identity never consults the daemon's data_dir.
        let global =
            crate::settings::XavierSettings::resolve_data_dir().join(REPO_MEMORY_DB_FILENAME);
        assert_ne!(store_a, global);
        assert_ne!(store_b, global);

        // Same identity, same answer — no fork from a differently spelled root.
        let respelled = PathBuf::from(&id_a.root).join(".").join("src").join("..");
        assert_eq!(memory_store_path_for_root(&respelled), store_a);
    }

    /// The generated config template keeps the package in the same directory as
    /// the store, so a repo can commit one coherent package.
    #[test]
    fn store_lives_next_to_the_repo_config_and_the_graph() {
        let _env = temp_env();
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = fake_repo(&tmp.path().join("repo-package-xavrepo"));
        let store = memory_store_path_for_root(&repo);
        let package_dir = store.parent().expect("store has a parent dir");

        assert_eq!(package_dir, repo.join(REPO_PACKAGE_DIR));
        assert_eq!(
            crate::codebase::repo_config::repo_config_path(&repo),
            package_dir.join(crate::codebase::repo_config::REPO_CONFIG_FILE),
            "config.toml and memory.sqlite3 share the package directory"
        );
        assert_eq!(
            crate::codebase::repo_identity::code_graph_db_path_for_root(&repo)
                .parent()
                .expect("graph has a parent dir"),
            package_dir
        );
    }
}
