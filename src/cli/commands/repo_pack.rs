//! CLI for `xavier repo pack` / `xavier repo unpack`: the encrypted per-repo
//! memory package `<repo>/.xavier/code_graph.db.enc`.
//!
//! ## What this layer adds over `codebase::repo_package_crypto`
//!
//! The crypto module owns the *container* (framing, AEAD, per-repo KEK
//! derivation). This module owns the three things that are a **CLI** concern and
//! nothing else:
//!
//! 1. **Which repo** — resolved with the exact same rules as `xavier repo
//!    config` (`repo::resolve_root`): explicit `-C` wins, else the nearest
//!    ancestor-or-self carrying `.xavier/config.toml`, else the git root. A
//!    package is therefore always written next to the identity it belongs to.
//! 2. **Which key** — resolved through the crate's one master-key path,
//!    [`xavier::keystore::MasterKeyManager::load_or_init`], and expanded into the
//!    repo-scoped KEK by `repo_package_crypto::derive_package_kek`. No key
//!    material is read, parsed or hardcoded here; a `&[u8; 32]` KEK is passed
//!    into the working functions, which is what makes them testable without a
//!    live keyring.
//! 3. **What commit** — `repo_identity::read_indexed_commit`, i.e.
//!    `.xavier/codegraph-sync-commit` → git `HEAD` → the literal `"unknown"`.
//!    That is the value sealed into the header, so an unpack can report *which*
//!    commit the restored graph describes and whether it still matches the repo.
//!
//! ## Atomicity
//!
//! `pack` writes through `encrypt_file`, which only writes after the whole
//! container is sealed in memory. `unpack` is stricter still: it decrypts into a
//! `tempfile::TempPath` **inside the same directory** and only renames it onto
//! `code_graph.db` once the AEAD tag verified. A corrupt `.enc`, a foreign key
//! or a truncated container therefore cannot leave a half-written database
//! behind — the temp file is removed on drop.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use xavier::codebase::repo_config::RepoConfig;
use xavier::codebase::repo_identity::{
    code_graph_db_path_for_root, read_indexed_commit, UNKNOWN_COMMIT,
};
use xavier::codebase::repo_package_crypto::{
    derive_package_kek, encrypt_file, header_for, package_path_for, PackageHeader, PACKAGE_VERSION,
};
use xavier::keystore::MasterKeyManager;

use crate::cli::commands::enums::RepoPackageArgs;
use crate::cli::commands::repo;

/// Canonical file name of the packaged artifact inside `.xavier/`.
pub const PACKED_FILE_NAME: &str = "code_graph.db";

/// What `pack` produced, for reporting (human or JSON).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackOutcome {
    pub repo_root: PathBuf,
    pub project_id: String,
    pub indexed_commit: String,
    /// `<repo>/.xavier/code_graph.db`
    pub db_path: PathBuf,
    /// `<repo>/.xavier/code_graph.db.enc`
    pub enc_path: PathBuf,
    pub plaintext_len: u64,
    /// True when a previous `.enc` existed and was replaced.
    pub overwritten: bool,
}

/// What `unpack` restored, for reporting (human or JSON).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnpackOutcome {
    pub repo_root: PathBuf,
    pub enc_path: PathBuf,
    pub db_path: PathBuf,
    /// Identity fields read back out of the **sealed** header.
    pub project_id: String,
    pub indexed_commit: String,
    pub source_file_name: String,
    pub plaintext_len: u64,
    pub created_at: String,
    /// Commit the repo is at *now*, per the same checkpoint → HEAD → unknown rule.
    pub current_commit: String,
    /// `indexed_commit == current_commit`.
    pub commit_matches: bool,
}

/// Everything `pack` and `unpack` must decide before touching a byte: which
/// repo, which `project_id`, and which repo-scoped KEK.
struct Resolved {
    root: PathBuf,
    project_id: String,
    kek: [u8; 32],
}

/// Resolve the target repo and its package KEK from the node master key.
///
/// `-C` is authoritative and must not walk up to a monorepo root, so this uses
/// `resolve_root_for_init`'s discipline (verbatim target). Using `resolve_root`
/// here would let a product's own dir be captured by an ancestor config,
/// writing the package into the wrong `.xavier/`.
fn resolve_target(root: Option<PathBuf>) -> Result<Resolved> {
    let root = resolve_pack_root(root)?;

    // A malformed config must not be papered over: derive the id the same way
    // `show` does, but say so loudly. A missing config is the normal case, so
    // only a parse failure is worth a warning.
    let project_id = match RepoConfig::load(&root) {
        Ok(cfg) => cfg.project_id,
        Err(err) => {
            eprintln!(
                "[warn] config por repo invalido ({}); se usa el project_id derivado del directorio",
                err
            );
            xavier::codebase::repo_identity::derive_project_id(&root)
        }
    };

    // The one master-key path for the whole crate. `load_or_init` reads the
    // keyring, then the encrypted fallback at `~/.xavier/master.key` (wrapped
    // with a host+machine-material HKDF), and only mints a new key when neither
    // exists — so it can never silently rotate away from a key that already
    // encrypted a committed package.
    let master = MasterKeyManager::load_or_init()
        .context("no se pudo resolver la master key del nodo (keyring y ~/.xavier/master.key)")?;
    let kek = derive_package_kek(&master, &project_id)
        .with_context(|| format!("no se pudo derivar la KEK del paquete para {project_id}"))?;

    Ok(Resolved {
        root,
        project_id,
        kek,
    })
}

/// `xavier repo pack -C <dir>`: seal `code_graph.db` into `code_graph.db.enc`.
pub fn run_repo_pack_command(args: RepoPackageArgs) -> Result<()> {
    let json = args.json;
    let Resolved {
        root,
        project_id,
        kek,
    } = resolve_target(args.root)?;

    let outcome = pack_with_kek(&root, &project_id, &kek)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "action": "pack",
                "repo_root": outcome.repo_root.to_string_lossy(),
                "project_id": outcome.project_id,
                "indexed_commit": outcome.indexed_commit,
                "db_path": outcome.db_path.to_string_lossy(),
                "enc_path": outcome.enc_path.to_string_lossy(),
                "plaintext_len": outcome.plaintext_len,
                "overwritten": outcome.overwritten,
                "container_version": PACKAGE_VERSION,
            }))?
        );
    } else {
        println!("Paquete cifrado creado: {}", outcome.enc_path.display());
        println!("  repo root:     {}", outcome.repo_root.display());
        println!("  project_id:    {}", outcome.project_id);
        println!(
            "  indexed_commit: {}{}",
            outcome.indexed_commit,
            if outcome.indexed_commit == UNKNOWN_COMMIT {
                " (sin checkpoint ni HEAD: no se inventa un hash)"
            } else {
                ""
            }
        );
        println!("  plaintext:     {} bytes", outcome.plaintext_len);
        if outcome.overwritten {
            println!("  [aviso] ya existia un .enc; fue SOBREESCRITO");
        }
    }
    Ok(())
}

/// `xavier repo unpack -C <dir>`: restore `code_graph.db` and show its header.
pub fn run_repo_unpack_command(args: RepoPackageArgs) -> Result<()> {
    let json = args.json;
    let Resolved { root, kek, .. } = resolve_target(args.root)?;

    let outcome = unpack_with_kek(&root, &kek)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "action": "unpack",
                "repo_root": outcome.repo_root.to_string_lossy(),
                "project_id": outcome.project_id,
                "indexed_commit": outcome.indexed_commit,
                "source_file_name": outcome.source_file_name,
                "plaintext_len": outcome.plaintext_len,
                "created_at": outcome.created_at,
                "db_path": outcome.db_path.to_string_lossy(),
                "enc_path": outcome.enc_path.to_string_lossy(),
                "current_commit": outcome.current_commit,
                "commit_matches": outcome.commit_matches,
            }))?
        );
    } else {
        println!(
            "Paquete descifrado restaurado: {}",
            outcome.db_path.display()
        );
        print_header(&outcome);
    }
    if !outcome.commit_matches {
        // Not a failure: the package is authentic, it just describes a
        // different commit than the repo is at now.
        eprintln!(
            "[warn] el codegraph corresponde al commit {} pero el repo esta en {}; \
             reindexa con `xavier code scan` para alinearlos",
            outcome.indexed_commit, outcome.current_commit
        );
    }
    Ok(())
}

/// Show the sealed header, so `project_id` and `indexed_commit` are visible.
fn print_header(outcome: &UnpackOutcome) {
    println!("  header del paquete (descifrado y autenticado):");
    println!("    project_id:      {}", outcome.project_id);
    println!("    indexed_commit:  {}", outcome.indexed_commit);
    println!("    source_file:     {}", outcome.source_file_name);
    println!("    plaintext bytes: {}", outcome.plaintext_len);
    println!("    created_at:      {}", outcome.created_at);
    println!(
        "    commit del repo: {} ({})",
        outcome.current_commit,
        if outcome.commit_matches {
            "coincide"
        } else {
            "DIFIERE del paquete"
        }
    );
}

/// Target directory for pack/unpack.
///
/// `-C` is taken verbatim (canonicalized) so a monorepo product packages *its*
/// memory and not the git root's. Without `-C`, the cwd's nearest
/// config-carrying directory wins, else the git root, else the cwd itself —
/// the same ladder `xavier repo config show` uses, so both commands agree on
/// what "this repo" is.
fn resolve_pack_root(root: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(explicit) = root {
        let absolute = std::path::absolute(&explicit).unwrap_or(explicit);
        let canonical = absolute.canonicalize().unwrap_or(absolute);
        if !canonical.is_dir() {
            bail!("{} no es un directorio", canonical.display());
        }
        return Ok(canonical);
    }
    repo::resolve_root(None)
}

/// Encrypt `<repo>/.xavier/code_graph.db` into `<repo>/.xavier/code_graph.db.enc`.
///
/// Fails loudly when there is no database to pack — an empty `.enc` would look
/// like a committed memory package but restore nothing. An existing `.enc` is
/// not an error: `pack` is how you refresh a committed package, so it is
/// replaced and the caller is told.
pub fn pack_with_kek(root: &Path, project_id: &str, kek: &[u8; 32]) -> Result<PackOutcome> {
    let db_path = code_graph_db_path_for_root(root);
    if !db_path.is_file() {
        bail!(
            "no existe {} — indexa el repo primero (`xavier code scan -C {}`) y luego reintenta",
            db_path.display(),
            root.display()
        );
    }
    let enc_path = package_path_for(root, PACKED_FILE_NAME);
    let overwritten = enc_path.exists();

    // checkpoint → git HEAD → "unknown". Never invented.
    let indexed_commit = read_indexed_commit(root);
    let header = header_for(project_id, PACKED_FILE_NAME, &indexed_commit);
    let written = encrypt_file(&db_path, &enc_path, kek, &header)
        .with_context(|| format!("fallo cifrando {}", db_path.display()))?;

    Ok(PackOutcome {
        repo_root: root.to_path_buf(),
        project_id: project_id.to_string(),
        indexed_commit,
        db_path,
        enc_path,
        plaintext_len: written.plaintext_len,
        overwritten,
    })
}

/// Restore `<repo>/.xavier/code_graph.db.enc` over `code_graph.db`.
///
/// Decryption happens into a sibling temp file that is renamed into place only
/// after the AEAD tag verified, so a failure of any kind leaves the previous
/// `code_graph.db` (if any) untouched and no partial database on disk.
pub fn unpack_with_kek(root: &Path, kek: &[u8; 32]) -> Result<UnpackOutcome> {
    let enc_path = package_path_for(root, PACKED_FILE_NAME);
    if !enc_path.is_file() {
        bail!(
            "no existe el paquete cifrado {} — crealo con `xavier repo pack -C {}`",
            enc_path.display(),
            root.display()
        );
    }
    let db_path = code_graph_db_path_for_root(root);
    let parent = db_path.parent().ok_or_else(|| {
        anyhow!(
            "{} no tiene directorio padre; no se puede restaurar",
            db_path.display()
        )
    })?;

    let header = decrypt_to_temp_then_rename(&enc_path, &db_path, parent, kek)?;

    // A package sealed under another repo's KEK cannot even be opened, so a
    // successful open already proves the project_id matches this repo's. The
    // check stays as a defence against a future change that widens key scope.
    let expected_project = RepoConfig::load(root)
        .map(|cfg| cfg.project_id)
        .unwrap_or_else(|_| xavier::codebase::repo_identity::derive_project_id(root));
    if header.project_id != expected_project {
        bail!(
            "el paquete pertenece al project_id {:?} pero este repo es {:?}; no se restaura \
             sobre otro repo",
            header.project_id,
            expected_project
        );
    }

    let current_commit = read_indexed_commit(root);
    Ok(UnpackOutcome {
        repo_root: root.to_path_buf(),
        enc_path,
        db_path,
        commit_matches: header.indexed_commit == current_commit,
        project_id: header.project_id.clone(),
        indexed_commit: header.indexed_commit.clone(),
        source_file_name: header.source_file_name.clone(),
        plaintext_len: header.plaintext_len,
        created_at: header.created_at.clone(),
        current_commit,
    })
}

/// `encrypt`-side counterpart of the atomic restore: decrypt into
/// `<parent>/.code_graph.db.unpack-XXXX`, then `rename` onto the target.
///
/// `TempPath::persist` is a `rename(2)` on the same filesystem, which is atomic:
/// a reader of `code_graph.db` sees either the old file or the complete new one,
/// never a truncated mix. If persist fails, the temp path is dropped and removed
/// — nothing partial survives.
fn decrypt_to_temp_then_rename(
    enc_path: &Path,
    db_path: &Path,
    parent: &Path,
    kek: &[u8; 32],
) -> Result<PackageHeader> {
    std::fs::create_dir_all(parent)
        .with_context(|| format!("no se pudo crear {}", parent.display()))?;

    let temp = tempfile::Builder::new()
        .prefix(".code_graph.db.unpack-")
        .tempfile_in(parent)
        .with_context(|| format!("no se pudo crear un temporal en {}", parent.display()))?;
    // Take the path and drop the handle so decrypt_file can write it itself
    // (through `keystore::write_private_file`, i.e. 0600 from the first syscall).
    let temp_path = temp.into_temp_path();

    let header = match xavier::codebase::repo_package_crypto::decrypt_file(
        enc_path,
        temp_path.as_ref(),
        kek,
    ) {
        Ok(header) => header,
        Err(err) => {
            // Explicit cleanup + message: the temp file must not be left behind
            // and the user must learn *why* (corrupt vs wrong key).
            drop(temp_path);
            return Err(err.context(format!(
                "no se pudo descifrar {} (puede estar corrupto o cifrado con otra master key)",
                enc_path.display()
            )));
        }
    };

    temp_path.persist(db_path).map_err(|e| {
        anyhow!(
            "el paquete se descifro pero no se pudo renombrar el temporal a {}: {}",
            db_path.display(),
            e.error
        )
    })?;

    // SQLite sidecars from an interrupted run would be stale next to the
    // freshly restored database.
    for suffix in ["-wal", "-shm"] {
        let sidecar = db_path.with_file_name(format!("{PACKED_FILE_NAME}{suffix}"));
        if sidecar.exists() {
            let _ = std::fs::remove_file(&sidecar);
        }
    }

    Ok(header)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use xavier::codebase::repo_identity::SYNC_CHECKPOINT_FILE;
    use xavier::codebase::repo_package_crypto::derive_package_kek_from_master;

    /// A test-only master key. Production goes through
    /// `MasterKeyManager::load_or_init`; the KEK derivation under test is the
    /// same HKDF-SHA256 the manager performs internally.
    const TEST_MASTER: [u8; 32] = [0x7A; 32];

    fn test_kek(project_id: &str) -> [u8; 32] {
        derive_package_kek_from_master(&TEST_MASTER, project_id).expect("derive test kek")
    }

    /// A repo directory with `.git` + `.xavier/`, and a real (queryable) SQLite
    /// `code_graph.db` inside it.
    fn repo_with_db(name: &str, dir: &Path) -> PathBuf {
        let repo = dir.join(name);
        std::fs::create_dir_all(repo.join(".git")).expect("mkdir .git");
        std::fs::create_dir_all(repo.join(".xavier")).expect("mkdir .xavier");
        let db = code_graph_db_path_for_root(&repo);
        let conn = rusqlite::Connection::open(&db).expect("open db");
        conn.execute_batch(
            "CREATE TABLE symbols (id INTEGER PRIMARY KEY, name TEXT);
             INSERT INTO symbols (name) VALUES ('alpha_fn'), ('beta_fn'), ('gamma_fn');",
        )
        .expect("seed db");
        repo
    }

    fn read_db_names(db: &Path) -> Vec<String> {
        let conn = rusqlite::Connection::open(db).expect("open restored db");
        let mut stmt = conn
            .prepare("SELECT name FROM symbols ORDER BY name")
            .expect("prep");
        let names: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .expect("query")
            .map(|r| r.expect("row"))
            .collect();
        names
    }

    /// The headline guarantee: pack → unpack returns the database byte for byte,
    /// and the sealed header carries the repo's real identity and commit.
    #[test]
    fn pack_then_unpack_restores_identical_bytes_and_a_correct_header() {
        let tmp = tempdir().expect("tempdir");
        let repo = repo_with_db("roundtrip-repo", tmp.path());
        let db = code_graph_db_path_for_root(&repo);
        let original = std::fs::read(&db).expect("read original db");

        std::fs::write(
            repo.join(".xavier").join(SYNC_CHECKPOINT_FILE),
            "cafebabe12345678\n",
        )
        .expect("write checkpoint");

        let project_id = RepoConfig::load(&repo).expect("load").project_id;
        assert_eq!(project_id, "roundtrip-repo", "id derived from the dir name");

        let kek = test_kek(&project_id);
        let packed = pack_with_kek(&repo, &project_id, &kek).expect("pack");
        assert_eq!(
            packed.indexed_commit, "cafebabe12345678",
            "from the checkpoint"
        );
        assert!(
            !packed.overwritten,
            "first pack must not report an overwrite"
        );
        assert_eq!(
            packed.enc_path,
            repo.join(".xavier").join("code_graph.db.enc")
        );
        assert_eq!(packed.plaintext_len, original.len() as u64);

        // The artifact is not a SQLite file and does not leak the commit.
        let raw = std::fs::read(&packed.enc_path).expect("read enc");
        assert!(!raw.starts_with(b"SQLite format 3"));
        assert!(
            !raw.windows(16).any(|w| w == b"cafebabe12345678"),
            "the commit leaked outside the AEAD"
        );

        // Delete the plaintext DB: the whole point is restoring it from the
        // committed package.
        std::fs::remove_file(&db).expect("remove db");
        assert!(!db.exists());

        let restored = unpack_with_kek(&repo, &kek).expect("unpack");
        assert_eq!(restored.project_id, project_id);
        assert_eq!(restored.indexed_commit, "cafebabe12345678");
        assert_eq!(restored.source_file_name, "code_graph.db");
        assert_eq!(restored.plaintext_len, original.len() as u64);
        assert!(restored.commit_matches, "repo is still at the checkpoint");

        assert_eq!(
            std::fs::read(&db).expect("read restored db"),
            original,
            "the restored database must be byte-identical"
        );
        assert_eq!(
            read_db_names(&db),
            vec!["alpha_fn", "beta_fn", "gamma_fn"],
            "and still a queryable SQLite database"
        );
        assert!(
            !dir_has_leftovers(tmp.path(), "unpack-"),
            "unpack left a temp file behind"
        );
    }

    #[test]
    fn pack_overwrites_an_existing_package_and_says_so() {
        let tmp = tempdir().expect("tempdir");
        let repo = repo_with_db("overwrite-repo", tmp.path());
        let project_id = RepoConfig::load(&repo).expect("load").project_id;
        let kek = test_kek(&project_id);

        let first = pack_with_kek(&repo, &project_id, &kek).expect("first pack");
        assert!(!first.overwritten);

        // Change the DB so the second package is genuinely a new one.
        let db = code_graph_db_path_for_root(&repo);
        let conn = rusqlite::Connection::open(&db).expect("open");
        conn.execute_batch("INSERT INTO symbols (name) VALUES ('delta_fn');")
            .expect("insert");

        let second = pack_with_kek(&repo, &project_id, &kek).expect("re-pack");
        assert!(
            second.overwritten,
            "an existing .enc must be reported, not fail"
        );
        assert_eq!(second.enc_path, first.enc_path);

        let restored_db = repo.join(".xavier").join("restored.db");
        std::fs::rename(&db, &restored_db).expect("stash");
        unpack_with_kek(&repo, &kek).expect("unpack over a missing db");
        assert_eq!(
            read_db_names(&db),
            vec!["alpha_fn", "beta_fn", "delta_fn", "gamma_fn"]
        );
    }

    #[test]
    fn pack_without_a_database_fails_clearly() {
        let tmp = tempdir().expect("tempdir");
        let repo = tmp.path().join("empty-repo");
        std::fs::create_dir_all(repo.join(".git")).expect("mkdir .git");
        std::fs::create_dir_all(repo.join(".xavier")).expect("mkdir .xavier");

        let project_id = RepoConfig::load(&repo).expect("load").project_id;
        let err = pack_with_kek(&repo, &project_id, &test_kek(&project_id))
            .expect_err("pack without a database must fail");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("code_graph.db"),
            "error must name the file: {msg}"
        );
        assert!(
            msg.contains("xavier code scan"),
            "error must say how to fix it: {msg}"
        );
        assert!(
            !package_path_for(&repo, PACKED_FILE_NAME).exists(),
            "a failed pack must not leave an empty .enc behind"
        );
    }

    #[test]
    fn unpack_without_a_package_fails_clearly() {
        let tmp = tempdir().expect("tempdir");
        let repo = repo_with_db("no-package-repo", tmp.path());
        let project_id = RepoConfig::load(&repo).expect("load").project_id;

        let err = unpack_with_kek(&repo, &test_kek(&project_id))
            .expect_err("unpack without an .enc must fail");
        let msg = err.to_string();
        assert!(
            msg.contains("code_graph.db.enc"),
            "error must name the file: {msg}"
        );
        assert!(
            msg.contains("repo pack"),
            "error must say how to fix it: {msg}"
        );
    }

    /// A corrupted container must fail AND leave no database and no temp file:
    /// the rename only happens after the AEAD tag verified.
    #[test]
    fn unpack_of_a_corrupt_package_fails_without_a_partial_db() {
        let tmp = tempdir().expect("tempdir");
        let repo = repo_with_db("corrupt-repo", tmp.path());
        let db = code_graph_db_path_for_root(&repo);
        let project_id = RepoConfig::load(&repo).expect("load").project_id;
        let kek = test_kek(&project_id);

        let packed = pack_with_kek(&repo, &project_id, &kek).expect("pack");
        std::fs::remove_file(&db).expect("remove db");

        // Flip one bit of the AEAD ciphertext (last byte = the GCM tag).
        let mut raw = std::fs::read(&packed.enc_path).expect("read enc");
        let last = raw.len() - 1;
        raw[last] ^= 0x01;
        std::fs::write(&packed.enc_path, &raw).expect("write corrupt");

        let err = unpack_with_kek(&repo, &kek).expect_err("a corrupt package must not unpack");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("authentication failed") || msg.contains("unwrap failed"),
            "error must say the AEAD check failed: {msg}"
        );
        assert!(
            !db.exists(),
            "a failed unpack must not leave a half-written code_graph.db"
        );
        assert!(
            !dir_has_leftovers(tmp.path(), "unpack-"),
            "a failed unpack must not leave its temp file behind"
        );

        // And a truncated container is rejected the same way.
        std::fs::write(&packed.enc_path, &raw[..raw.len() / 2]).expect("truncate");
        let err = unpack_with_kek(&repo, &kek).expect_err("truncated package must not unpack");
        assert!(format!("{err:#}").contains("no se pudo descifrar"));
        assert!(!db.exists(), "truncated package must not leave a db");
    }

    /// A package sealed for another repo cannot be opened with this repo's KEK,
    /// and the failure still leaves nothing behind.
    #[test]
    fn unpack_with_the_wrong_repo_key_fails_and_writes_nothing() {
        let tmp = tempdir().expect("tempdir");
        let repo_a = repo_with_db("repo-alpha", tmp.path());
        let repo_b = repo_with_db("repo-beta", tmp.path());
        let id_a = RepoConfig::load(&repo_a).expect("load a").project_id;
        let id_b = RepoConfig::load(&repo_b).expect("load b").project_id;

        pack_with_kek(&repo_a, &id_a, &test_kek(&id_a)).expect("pack A");
        std::fs::remove_file(code_graph_db_path_for_root(&repo_a)).expect("rm A db");

        // B's KEK against A's package: isolation is the property being tested.
        let err = unpack_with_kek(&repo_a, &test_kek(&id_b)).expect_err("cross-key open must fail");
        assert!(format!("{err:#}").contains("unwrap failed"), "{err:#}");
        assert!(!code_graph_db_path_for_root(&repo_a).exists());
    }

    /// The `indexed_commit` policy, all three branches, verified through the
    /// sealed header rather than through a helper: checkpoint → HEAD → unknown.
    #[test]
    fn indexed_commit_comes_from_the_checkpoint_then_head_then_unknown() {
        let tmp = tempdir().expect("tempdir");

        // 1. Checkpoint wins over a real git HEAD.
        let with_checkpoint = repo_with_db("commit-policy-a", tmp.path());
        let id = RepoConfig::load(&with_checkpoint).expect("load").project_id;
        std::fs::write(
            with_checkpoint.join(".xavier").join(SYNC_CHECKPOINT_FILE),
            "1111111111111111\n",
        )
        .expect("write checkpoint");
        let kek = test_kek(&id);
        assert_eq!(
            pack_with_kek(&with_checkpoint, &id, &kek)
                .expect("pack")
                .indexed_commit,
            "1111111111111111"
        );
        std::fs::remove_file(code_graph_db_path_for_root(&with_checkpoint)).expect("rm db");
        assert_eq!(
            unpack_with_kek(&with_checkpoint, &kek)
                .expect("unpack")
                .indexed_commit,
            "1111111111111111"
        );

        // 2. No checkpoint: a genuine `git init` + one commit resolves a HEAD.
        //
        // Deliberately a REAL checkout, not a `mkdir .git` stub: this machine's
        // TMPDIR lives inside the `~/.hermes` repo, and git ignores an empty
        // `.git` directory, so `rev-parse HEAD` walks UP to that repo and
        // returns its commit — which would silently turn the "unknown" branch
        // below into a phantom HEAD. A real `git init` with zero commits is the
        // only reliable way to have no HEAD at all.
        let real = tmp.path().join("commit-policy-d");
        let head = git_commit_tmp_repo(&real);
        assert!(
            head.len() >= 7 && head.chars().all(|c| c.is_ascii_hexdigit()),
            "git must be available for the HEAD branch, got {head:?}"
        );
        std::fs::create_dir_all(real.join(".xavier")).expect("mkdir .xavier");
        let db = code_graph_db_path_for_root(&real);
        rusqlite::Connection::open(&db)
            .expect("open")
            .execute_batch("CREATE TABLE t (a);")
            .expect("ddl");
        let id_real = RepoConfig::load(&real).expect("load").project_id;
        let kek_real = test_kek(&id_real);
        let packed = pack_with_kek(&real, &id_real, &kek_real).expect("pack");
        assert_eq!(
            packed.indexed_commit, head,
            "without a checkpoint the commit must come from git HEAD"
        );
        std::fs::remove_file(&db).expect("rm db");
        let restored = unpack_with_kek(&real, &kek_real).expect("unpack");
        assert_eq!(restored.indexed_commit, head);
        assert!(
            restored.commit_matches,
            "a freshly packed package must match the repo's HEAD"
        );

        // 3. Neither checkpoint nor HEAD: `git init` with no commit at all.
        // The value must be the literal "unknown" — never an invented hash.
        let no_history = tmp.path().join("commit-policy-e");
        let no_history = git_init_empty_repo(&no_history);
        std::fs::create_dir_all(no_history.join(".xavier")).expect("mkdir .xavier");
        let db = code_graph_db_path_for_root(&no_history);
        rusqlite::Connection::open(&db)
            .expect("open")
            .execute_batch("CREATE TABLE t (a);")
            .expect("ddl");
        let id_unknown = RepoConfig::load(&no_history).expect("load").project_id;
        let kek_unknown = test_kek(&id_unknown);
        let packed = pack_with_kek(&no_history, &id_unknown, &kek_unknown).expect("pack");
        assert_eq!(
            packed.indexed_commit, UNKNOWN_COMMIT,
            "no checkpoint and no HEAD must seal the literal 'unknown'"
        );
        std::fs::remove_file(&db).expect("rm db");
        let restored = unpack_with_kek(&no_history, &kek_unknown).expect("unpack");
        assert_eq!(
            restored.indexed_commit, UNKNOWN_COMMIT,
            "the header must round-trip 'unknown' unchanged"
        );
    }

    /// A package describing a *different* commit than the repo is at now must
    /// unpack fine and be reported as drift (not as a failure).
    #[test]
    fn unpack_reports_drift_when_the_commit_no_longer_matches() {
        let tmp = tempdir().expect("tempdir");
        let repo = repo_with_db("drift-repo", tmp.path());
        let db = code_graph_db_path_for_root(&repo);
        let project_id = RepoConfig::load(&repo).expect("load").project_id;
        let kek = test_kek(&project_id);

        std::fs::write(
            repo.join(".xavier").join(SYNC_CHECKPOINT_FILE),
            "aaaaaaaaaaaaaaaa\n",
        )
        .expect("write checkpoint");
        pack_with_kek(&repo, &project_id, &kek).expect("pack");
        std::fs::remove_file(&db).expect("rm db");

        // The repo moved on.
        std::fs::write(
            repo.join(".xavier").join(SYNC_CHECKPOINT_FILE),
            "bbbbbbbbbbbbbbbb\n",
        )
        .expect("write new checkpoint");

        let outcome = unpack_with_kek(&repo, &kek).expect("unpack still succeeds");
        assert_eq!(outcome.indexed_commit, "aaaaaaaaaaaaaaaa");
        assert_eq!(outcome.current_commit, "bbbbbbbbbbbbbbbb");
        assert!(
            !outcome.commit_matches,
            "drift must be visible in the report"
        );
        assert!(db.exists(), "an authentic package is still restored");
    }

    /// Two repos in one checkout get two independent packages: a package from
    /// repo A must not restore into repo B.
    #[test]
    fn packages_are_isolated_per_repo() {
        let tmp = tempdir().expect("tempdir");
        let a = repo_with_db("iso-alpha", tmp.path());
        let b = repo_with_db("iso-beta", tmp.path());
        let id_a = RepoConfig::load(&a).expect("a").project_id;
        let id_b = RepoConfig::load(&b).expect("b").project_id;
        assert_ne!(id_a, id_b);

        let packed_a = pack_with_kek(&a, &id_a, &test_kek(&id_a)).expect("pack A");
        let packed_b = pack_with_kek(&b, &id_b, &test_kek(&id_b)).expect("pack B");
        assert_ne!(packed_a.enc_path, packed_b.enc_path);
        assert_ne!(
            std::fs::read(&packed_a.enc_path).expect("A enc"),
            std::fs::read(&packed_b.enc_path).expect("B enc"),
            "same DB bytes under two repo KEKs must not collide"
        );

        // A's package opened with B's KEK (the wrong repo's key) fails, and B's
        // own database is untouched.
        let b_db_bytes = std::fs::read(code_graph_db_path_for_root(&b)).expect("B db");
        assert!(unpack_with_kek(&b, &test_kek(&id_a)).is_err());
        assert_eq!(
            std::fs::read(code_graph_db_path_for_root(&b)).expect("B db"),
            b_db_bytes,
            "a failed cross-repo unpack must not touch the other repo"
        );
    }

    /// A declared `project_id` in config.toml scopes the KEK, so the same
    /// directory yields different packages under different declared identities.
    #[test]
    fn declared_project_id_scopes_the_package_key() {
        let tmp = tempdir().expect("tempdir");
        let repo = repo_with_db("declared-repo", tmp.path());
        std::fs::write(
            repo.join(".xavier").join("config.toml"),
            "project_id = \"declared-id\"\n",
        )
        .expect("write config");

        let id = RepoConfig::load(&repo).expect("load").project_id;
        assert_eq!(id, "declared-id");

        let packed = pack_with_kek(&repo, &id, &test_kek(&id)).expect("pack");
        std::fs::remove_file(code_graph_db_path_for_root(&repo)).expect("rm db");
        let restored = unpack_with_kek(&repo, &test_kek(&id)).expect("unpack");
        assert_eq!(restored.project_id, "declared-id");

        // The directory-derived key no longer opens it.
        let derived = xavier::codebase::repo_identity::derive_project_id(&repo);
        assert_eq!(derived, "declared-repo");
        std::fs::remove_file(code_graph_db_path_for_root(&repo)).expect("rm db 2");
        assert!(
            unpack_with_kek(&repo, &test_kek(&derived)).is_err(),
            "the directory-derived id must not open a declared-id package"
        );
        assert!(!code_graph_db_path_for_root(&repo).exists());
    }

    /// `-C` must address the product it names, not the monorepo git root.
    #[test]
    fn explicit_root_targets_the_named_directory() {
        let tmp = tempdir().expect("tempdir");
        let monorepo = tmp.path().join("monorepo-pack");
        let product = monorepo.join("apps").join("duque-mvp");
        std::fs::create_dir_all(product.join(".git")).expect("mkdir product");
        std::fs::create_dir_all(monorepo.join(".git")).expect("mkdir root");
        // A config at the git root must NOT capture the product.
        std::fs::create_dir_all(monorepo.join(".xavier")).expect("mkdir root .xavier");
        std::fs::write(
            monorepo.join(".xavier").join("config.toml"),
            "project_id = \"monorepo-root\"\n",
        )
        .expect("write root config");

        let resolved = resolve_pack_root(Some(product.clone())).expect("resolve");
        assert_eq!(resolved, product.canonicalize().unwrap_or(product));

        // And a non-existent directory is rejected rather than silently walked up.
        let err = resolve_pack_root(Some(tmp.path().join("nope-pack"))).expect_err("must fail");
        assert!(err.to_string().contains("no es un directorio"), "{err}");
    }

    // --- helpers -----------------------------------------------------------

    fn dir_has_leftovers(dir: &Path, needle: &str) -> bool {
        fn walk(dir: &Path, needle: &str) -> bool {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return false;
            };
            entries.flatten().any(|e| {
                e.file_name().to_string_lossy().contains(needle)
                    || (e.path().is_dir() && walk(&e.path(), needle))
            })
        }
        walk(dir, needle)
    }

    /// A real `git init` in `repo` with **no commit**, so `rev-parse HEAD`
    /// genuinely fails. The only reliable way to produce the "unknown" branch.
    fn git_init_empty_repo(repo: &Path) -> PathBuf {
        std::fs::create_dir_all(repo).expect("mkdir repo");
        let ok = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(repo)
            .output()
            .expect("git init")
            .status
            .success();
        assert!(ok, "git init must succeed for the unknown-commit branch");
        repo.to_path_buf()
    }

    /// `git init` + one commit in `repo`; returns the commit sha. Returns an
    /// empty string when git is unavailable (the caller asserts on it).
    fn git_commit_tmp_repo(repo: &Path) -> String {
        std::fs::create_dir_all(repo).expect("mkdir repo");
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(repo)
                .output()
                .expect("git")
        };
        if !run(&["init", "-q"]).status.success() {
            return String::new();
        }
        run(&["config", "user.email", "t@t.t"]);
        run(&["config", "user.name", "t"]);
        std::fs::write(repo.join("f.txt"), "x").expect("write f");
        run(&["add", "f.txt"]);
        run(&["commit", "-q", "-m", "c"]);
        let out = run(&["rev-parse", "HEAD"]);
        if !out.status.success() {
            return String::new();
        }
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }
}
