//! CLI for the per-repo Xavier memory package (`<repo>/.xavier/config.toml`).
//!
//! Everything here runs locally: managing a repo's declared identity and
//! privacy scope must work without the HTTP server, exactly like the memory
//! index file lives in the repo. All I/O goes through
//! [`xavier::codebase::repo_config`], which owns validation, so a bad value is
//! rejected before it can reach disk.

use std::path::PathBuf;

use crate::cli::commands::enums::{RepoConfigArgs, RepoConfigCommand};
use anyhow::{bail, Result};
use xavier::codebase::repo_config::{
    init_config, repo_config_path, set_key, ProjectIdOrigin, RepoConfig, SETTABLE_KEYS,
};
use xavier::codebase::repo_identity::{find_repo_root, sanitize_project_id};

/// Repo package subcommands.
pub fn run_repo_config_command(cmd: RepoConfigCommand) -> Result<()> {
    let (root, json) = match &cmd {
        RepoConfigCommand::Init { args, .. }
        | RepoConfigCommand::Show { args }
        | RepoConfigCommand::Set { args, .. } => (args.root.clone(), args.json),
    };

    match cmd {
        RepoConfigCommand::Init { force, .. } => {
            let root = resolve_root_for_init(root.clone())?;
            let path = init_config(&root, force)?;
            let cfg = RepoConfig::load(&root)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report(&root, &cfg))?);
            } else {
                println!("Config por repo creado: {}", path.display());
                print_human(&root, &cfg);
            }
            Ok(())
        }
        RepoConfigCommand::Show { .. } => {
            let root = resolve_root(root.clone())?;
            // A broken config must be reported, never papered over: the derived
            // identity still applies, but the user has to know their file is bad.
            let (cfg, broken) = match RepoConfig::load(&root) {
                Ok(cfg) => (cfg, None),
                Err(err) => (RepoConfig::defaults_for(&root), Some(err)),
            };
            if let Some(err) = &broken {
                eprintln!("[warn] {}", err);
            }
            if json {
                let mut value = report(&root, &cfg);
                if let Some(obj) = value.as_object_mut() {
                    obj.insert(
                        "valid".to_string(),
                        serde_json::Value::Bool(broken.is_none()),
                    );
                }
                println!("{}", serde_json::to_string_pretty(&value)?);
            } else {
                if let Some(err) = &broken {
                    println!("estado: INVALIDO ({})", err);
                }
                print_human(&root, &cfg);
            }
            if broken.is_some() {
                bail!("el config por repo no es valido; se muestran los valores por defecto");
            }
            Ok(())
        }
        RepoConfigCommand::Set { key, value, .. } => {
            let root = resolve_root(root.clone())?;
            let cfg = set_key(&root, &key, &value)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report(&root, &cfg))?);
            } else {
                println!("{} = {}", key, value);
                print_human(&root, &cfg);
            }
            Ok(())
        }
    }
}

/// Target repo for the per-repo config on read/mutate commands (`show`, `set`).
///
/// Resolution order, most specific first, because a monorepo is exactly the
/// case where "the git root" is too coarse to identify a product:
///
/// 1. an explicit `--root`,
/// 2. the nearest **ancestor-or-self** directory that carries its own
///    `<dir>/.xavier/config.toml` (this is what makes `apps/duque-mvp` pick up
///    *its own* declared identity instead of inheriting the monorepo root's),
/// 3. the git root containing the cwd,
/// 4. the cwd itself.
fn resolve_root(root: Option<PathBuf>) -> Result<PathBuf> {
    let cwd = std::env::current_dir()?;
    let explicit = root.is_some();
    let start = root.unwrap_or_else(|| cwd.clone());
    let absolute = std::path::absolute(&start).unwrap_or(start);
    let canonical = absolute.canonicalize().unwrap_or(absolute);

    if explicit {
        // An explicit `--root` is authoritative: do not walk past it, or a
        // caller naming a monorepo product would silently get the repo root.
        if let Some(with_config) = nearest_config_ancestor(&canonical) {
            return Ok(with_config);
        }
        let git_root = find_repo_root(&canonical).ok_or_else(|| {
            anyhow::anyhow!(
                "no se pudo resolver la raiz del repo desde {}",
                canonical.display()
            )
        })?;
        return Ok(git_root.canonicalize().unwrap_or(git_root));
    }

    if let Some(with_config) = nearest_config_ancestor(&canonical) {
        return Ok(with_config);
    }
    let git_root = find_repo_root(&canonical).unwrap_or_else(|| canonical.clone());
    Ok(git_root.canonicalize().unwrap_or(git_root))
}

/// Target directory for `init`, which is deliberately NOT [`resolve_root`].
///
/// `init` creates the very config that `resolve_root` looks for, so walking up
/// to the nearest config ancestor is circular: a product that does not exist
/// yet as an instance (the normal first run) has no config, and the lookup
/// either finds nothing or finds the *monorepo root's* config and returns
/// it — writing both products' identities into one shared root file and
/// destroying the per-product isolation this whole feature exists to provide.
///
/// Rules, in order:
///
/// 1. an explicit `-C/--root` names the instance to create, so it wins and is
///    used verbatim (canonicalized). It can be a subdirectory of a git repo;
///    that is the whole point of a monorepo with several products.
/// 2. no `-C`: if the cwd is itself inside a config-carrying directory tree,
///    keep the current nearest-ancestor behaviour so re-initialising from
///    inside an existing instance stays idempotent.
/// 3. no `-C` and no config ancestor: the git root, which is what a
///    single-product repo wants.
fn resolve_root_for_init(root: Option<PathBuf>) -> Result<PathBuf> {
    let cwd = std::env::current_dir()?;
    let Some(start) = root else {
        let absolute = std::path::absolute(&cwd).unwrap_or(cwd);
        let canonical = absolute.canonicalize().unwrap_or(absolute);
        if let Some(with_config) = nearest_config_ancestor(&canonical) {
            return Ok(with_config);
        }
        let git_root = find_repo_root(&canonical).unwrap_or(canonical);
        return Ok(git_root.canonicalize().unwrap_or(git_root));
    };

    let absolute = std::path::absolute(&start).unwrap_or(start);
    Ok(absolute.canonicalize().unwrap_or(absolute))
}

/// Closest ancestor-or-self of `dir` that declares its own `.xavier/config.toml`.
fn nearest_config_ancestor(dir: &std::path::Path) -> Option<PathBuf> {
    let mut current = Some(dir);
    while let Some(candidate) = current {
        if repo_config_path(candidate).is_file() {
            return Some(candidate.to_path_buf());
        }
        current = candidate.parent();
    }
    None
}

fn report(root: &std::path::Path, cfg: &RepoConfig) -> serde_json::Value {
    serde_json::json!({
        "repo_root": root.to_string_lossy(),
        "config_path": cfg.source_path.as_ref().map(|p| p.to_string_lossy().into_owned()),
        "project_id": cfg.project_id,
        "project_id_origin": cfg.project_id_origin,
        "name": cfg.name,
        "default_clearance": cfg.default_clearance.as_str(),
        "sync_codegraph": cfg.sync_codegraph,
        "embedding_dimensions": cfg.embedding_dimensions,
        "retention_days": cfg.retention_days,
        "settable_keys": SETTABLE_KEYS,
    })
}

fn print_human(root: &std::path::Path, cfg: &RepoConfig) {
    let origin = match cfg.project_id_origin {
        ProjectIdOrigin::Directory => "derivado del directorio",
        ProjectIdOrigin::ConfigFile => "declarado en config.toml",
    };
    println!("  repo root:    {}", root.display());
    println!(
        "  config:       {}",
        cfg.source_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| format!("(ausente — se usan defaults)"))
    );
    println!("  project_id:   {} ({})", cfg.project_id, origin);
    println!("  name:         {}", cfg.name);
    println!(
        "  clearance:    {} (por defecto para memorias de este repo)",
        cfg.default_clearance.as_str()
    );
    println!(
        "  codegraph sync_to_repo: {}",
        if cfg.sync_codegraph { "si" } else { "no" }
    );
    println!(
        "  embedding_dimensions: {}",
        cfg.embedding_dimensions
            .map(|d| d.to_string())
            .unwrap_or_else(|| "hereda global".to_string())
    );
    println!(
        "  retention_days: {}",
        cfg.retention_days
            .map(|d| d.to_string())
            .unwrap_or_else(|| "sin retencion".to_string())
    );
}

/// Where a repo config lives, for `--help` text and shell completion.
pub fn config_path_hint(root: &std::path::Path) -> String {
    repo_config_path(root).display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::commands::enums::RepoConfigArgs;

    fn args(root: PathBuf, json: bool) -> RepoConfigArgs {
        RepoConfigArgs {
            root: Some(root),
            json,
        }
    }

    #[test]
    fn repo_config_cli_init_show_set_round_trip() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = tmp.path().join("cli-repo");
        std::fs::create_dir_all(repo.join(".git")).expect("mkdir .git");

        // show on a repo with no config must succeed (defaults, not an error)
        run_repo_config_command(RepoConfigCommand::Show {
            args: args(repo.clone(), true),
        })
        .expect("show without config must not fail");

        run_repo_config_command(RepoConfigCommand::Init {
            force: false,
            args: args(repo.clone(), false),
        })
        .expect("init");

        run_repo_config_command(RepoConfigCommand::Set {
            key: "project_id".to_string(),
            value: "duque mvp".to_string(),
            args: args(repo.clone(), false),
        })
        .expect("set");

        let cfg = RepoConfig::load(&repo).expect("load");
        assert_eq!(
            cfg.project_id, "duque_mvp",
            "sanitized like derive_project_id"
        );

        run_repo_config_command(RepoConfigCommand::Show {
            args: args(repo, false),
        })
        .expect("show");
    }

    #[test]
    fn repo_config_cli_set_bad_value_errors_clearly() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = tmp.path().join("cli-bad-repo");
        std::fs::create_dir_all(repo.join(".git")).expect("mkdir .git");

        let err = run_repo_config_command(RepoConfigCommand::Set {
            key: "clearance".to_string(),
            value: "abierto-todo".to_string(),
            args: args(repo, false),
        })
        .expect_err("invalid clearance must fail");
        assert!(
            err.to_string().contains("privacy.default_clearance"),
            "error must name the offending field, got: {err}"
        );
    }

    /// Regression: in a MONOREPO, naming a product with `-C` must resolve that
    /// product's own config, not the git root's. Without this the CLI happily
    /// reported the monorepo's derived id for every product — the exact gap
    /// this feature exists to close, reproduced at the CLI layer.
    #[test]
    fn repo_config_cli_resolves_a_monorepo_product_not_the_git_root() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = tmp.path().join("swal");
        let duque = repo.join("apps").join("duque-mvp");
        let tripro = repo.join("apps").join("tripro-web");
        for d in [&duque, &tripro] {
            std::fs::create_dir_all(d.join(".xavier")).expect("mkdir product .xavier");
        }
        std::fs::create_dir_all(repo.join(".git")).expect("mkdir .git");
        std::fs::write(
            duque.join(".xavier").join("config.toml"),
            "project_id = \"duque-mvp\"\n",
        )
        .expect("write duque config");
        std::fs::write(
            tripro.join(".xavier").join("config.toml"),
            "project_id = \"tripro-web\"\n",
        )
        .expect("write tripro config");

        // Each product, addressed by its own path, must report its own id…
        for (dir, expected) in [(&duque, "duque-mvp"), (&tripro, "tripro-web")] {
            let resolved = resolve_root(Some(dir.clone())).expect("resolve product root");
            assert_eq!(
                resolved,
                dir.canonicalize().unwrap_or(dir.clone()),
                "must NOT walk up to the monorepo git root"
            );
            let cfg = RepoConfig::load(&resolved).expect("load product config");
            assert_eq!(cfg.project_id, expected);
        }

        // …and the git root itself keeps its own (derived) identity, so the
        // three stay distinct inside ONE checkout.
        let root_cfg =
            RepoConfig::load(&resolve_root(Some(repo.clone())).expect("root")).expect("load root");
        assert_ne!(root_cfg.project_id, "duque-mvp");
        assert_ne!(root_cfg.project_id, "tripro-web");
    }

    /// Regression for the `init` half of the monorepo bug: asking to create the
    /// config **of a product** with `-C` must create it *there*, even when that
    /// product has no config yet (the normal first run) and even when the git
    /// root's config already exists.
    ///
    /// The old code routed `init` through the same nearest-config-ancestor
    /// lookup used by `show`/`set`. That lookup is circular for `init` — it is
    /// trying to find the very file `init` is about to create — so it either
    /// fell back to `find_repo_root` (monorepo root) or latched onto the root's
    /// config, and both products of the monorepo ended up sharing one file.
    #[test]
    fn repo_config_cli_init_creates_config_in_the_explicit_product_not_the_git_root() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = tmp.path().join("swal-init");
        let duque = repo.join("apps").join("duque-mvp");
        std::fs::create_dir_all(&duque).expect("mkdir product");
        std::fs::create_dir_all(repo.join(".git")).expect("mkdir .git");
        // Pre-existing config on the git root: the ancestor lookup must not
        // capture `init` and write the product's identity into it.
        std::fs::create_dir_all(repo.join(".xavier")).expect("mkdir root .xavier");
        std::fs::write(
            repo.join(".xavier").join("config.toml"),
            "project_id = \"monorepo-root\"\n",
        )
        .expect("write root config");

        let resolved = resolve_root_for_init(Some(duque.clone())).expect("resolve init target");
        assert_eq!(
            resolved,
            duque.canonicalize().unwrap_or(duque.clone()),
            "init -C <product> must target the product, never the git root"
        );

        let path = init_config(&resolved, false).expect("init product config");
        assert_eq!(
            path,
            duque
                .canonicalize()
                .unwrap_or(duque.clone())
                .join(".xavier")
                .join("config.toml"),
            "config file must be created inside the product"
        );
        assert!(
            duque.join(".xavier").join("config.toml").is_file(),
            "product must own its config"
        );

        let root_cfg = RepoConfig::load(&repo).expect("load root config");
        assert_eq!(
            root_cfg.project_id, "monorepo-root",
            "the git root's config must be left untouched by a product's init"
        );
    }

    /// Two products of ONE git repo, each `init -C`'d on its own directory while
    /// neither has a config yet, must end up as two distinct memory instances
    /// with two distinct `project_id`s. This is the actual requirement behind
    /// the monorepo isolation work.
    #[test]
    fn repo_config_cli_init_two_products_get_distinct_project_ids() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = tmp.path().join("swal-two-products");
        let duque = repo.join("apps").join("duque-mvp");
        let tripro = repo.join("apps").join("tripro-web");
        for d in [&duque, &tripro] {
            std::fs::create_dir_all(d).expect("mkdir product");
        }
        std::fs::create_dir_all(repo.join(".git")).expect("mkdir .git");

        // First product: nothing exists anywhere in the repo yet.
        let duque_root = resolve_root_for_init(Some(duque.clone())).expect("resolve duque");
        init_config(&duque_root, false).expect("init duque");

        // Second product: the FIRST product's config now exists as a sibling,
        // and its project dir is *not* an ancestor of this one — so a
        // nearest-ancestor walk must still land on tripro-web itself.
        let tripro_root = resolve_root_for_init(Some(tripro.clone())).expect("resolve tripro");
        init_config(&tripro_root, false).expect("init tripro");

        assert_eq!(
            duque_root,
            duque.canonicalize().unwrap_or(duque.clone()),
            "duque-mvp keeps its own identity"
        );
        assert_eq!(
            tripro_root,
            tripro.canonicalize().unwrap_or(tripro.clone()),
            "tripro-web keeps its own identity"
        );

        let duque_cfg = RepoConfig::load(&duque).expect("load duque config");
        let tripro_cfg = RepoConfig::load(&tripro).expect("load tripro config");
        // ONE policy for every project_id, whichever route produced it: the
        // value goes through `sanitize_project_id` (repo_identity.rs) — both
        // the directory-derived id written by `init` and a value declared in
        // config.toml by `set`. There is deliberately no second normalizer.
        //
        // A hyphen is NOT rewritten to an underscore: it is already inside the
        // legal `[A-Za-z0-9_-]` set, so `duque-mvp` survives as `duque-mvp`.
        // Sanitizing means stripping what is NOT legal (spaces, dots, slashes,
        // `..`), which is what keeps the id safe as a filesystem path. So these
        // two ids keep their hyphens and still differ.
        assert_eq!(duque_cfg.project_id, "duque-mvp");
        assert_eq!(tripro_cfg.project_id, "tripro-web");
        assert_ne!(
            duque_cfg.project_id, tripro_cfg.project_id,
            "two products in one git repo must be two memory instances"
        );
        assert!(
            !repo.join(".xavier").join("config.toml").exists(),
            "neither product's init may leak a config into the git root"
        );
    }

    /// Policy lock: ONE sanitizer for a `project_id`, whichever route produced it —
    /// `init` (directory name) or `set project_id` (declared in config.toml).
    ///
    /// Two tests once disagreed here (hyphen kept as `duque-mvp` vs rewritten to
    /// `duque_mvp`) because nothing made the single policy explicit. This one does.
    /// A hyphen is LEGAL (`[A-Za-z0-9_-]`), so it survives; sanitizing strips what
    /// is NOT legal, which is what keeps an id usable as a path.
    #[test]
    fn repo_project_id_has_one_sanitizer_for_init_and_set() {
        // Single path components only: `derive_project_id` reads `file_name()`, so
        // a raw containing `/` would nest directories and keep just the last one.
        for (raw, expected) in [
            ("duque-mvp", "duque-mvp"),
            ("tripro web", "tripro_web"),
            ("duque.mvp", "duque_mvp"),
            ("Mixed_Case-123", "Mixed_Case-123"),
        ] {
            // Route A: `init` derives the id from the directory name.
            let tmp = tempfile::tempdir().expect("tempdir");
            let repo = tmp.path().join(raw);
            std::fs::create_dir_all(repo.join(".git")).expect("mkdir .git");
            let via_init = xavier::codebase::repo_identity::derive_project_id(&repo);

            // Route B: `set project_id` declares the same raw value in the file.
            let tmp2 = tempfile::tempdir().expect("tempdir");
            let repo2 = tmp2.path().join("declared");
            std::fs::create_dir_all(repo2.join(".git")).expect("mkdir .git");
            std::fs::create_dir_all(repo2.join(".xavier")).expect("mkdir .xavier");
            std::fs::write(repo_config_path(&repo2), format!("project_id = {raw:?}\n"))
                .expect("write config");
            let via_set = RepoConfig::load(&repo2)
                .expect("declared config loads")
                .project_id;

            assert_eq!(via_init, expected, "init-derived id for {raw:?}");
            assert_eq!(
                via_set, expected,
                "set-declared id for {raw:?}: one sanitizer, not two policies"
            );
            // Both routes agree, so `init` then `set` cannot drift apart.
            assert_eq!(via_init, via_set);
            // Legal by construction, hence safe as a path — this also proves
            // sanitizing is idempotent.
            assert_eq!(sanitize_project_id(expected), expected);
        }

        // What a repo writes into config.toml is untrusted: separators and `..`
        // must not survive, or a project_id becomes a path outside the memory dir.
        // (`validate_project_id` only admits `[A-Za-z0-9_-]`, so passing it is proof.)
        for hostile in ["../etc/passwd", "a/b/c", "../../root"] {
            let out = sanitize_project_id(hostile);
            assert!(
                crate::codebase::validate_project_id(&out).is_ok(),
                "{hostile:?} sanitized to {out:?}, which is still a path escape"
            );
        }
    }

    #[test]
    fn repo_config_cli_show_reports_invalid_config() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = tmp.path().join("cli-broken-repo");
        std::fs::create_dir_all(repo.join(".git")).expect("mkdir .git");
        std::fs::create_dir_all(repo.join(".xavier")).expect("mkdir .xavier");
        std::fs::write(
            repo.join(".xavier").join("config.toml"),
            "project_id = = 1\n",
        )
        .expect("write broken config");

        let err = run_repo_config_command(RepoConfigCommand::Show {
            args: args(repo, false),
        })
        .expect_err("an invalid config must be reported as an error");
        assert!(
            err.to_string().contains("no es valido"),
            "error must be actionable, got: {err}"
        );
    }
}
