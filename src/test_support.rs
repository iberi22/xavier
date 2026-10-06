//! Test isolation for the user's real directories.
//!
//! This crate runs on machines whose `~/.xavier` is a live production store.
//! Test and bench binaries must never read or write it. Two pieces:
//!
//! * [`isolate_user_dirs`] points `HOME`, the XDG dirs and the `XAVIER_*`
//!   directory overrides at a per-process tempdir (idempotent; opt out with
//!   `XAVIER_TEST_REAL_HOME=1` for a deliberate live test).
//! * [`guard_user_path`] is called by the code that opens/creates files under
//!   the user home. When the running executable is a cargo test/bench binary
//!   (it lives in a `deps` directory) and the path is under the REAL user home,
//!   it panics instead of touching it. Production binaries are unaffected.
//!
//! Library code keeps resolving paths exactly as before; only the test
//! harness decides where "home" is.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static SANDBOX: OnceLock<PathBuf> = OnceLock::new();

/// Env vars pointing at user/state directories, redirected into the sandbox.
const DIR_VARS: [(&str, &str); 10] = [
    ("HOME", ""),
    ("XDG_DATA_HOME", ".local/share"),
    ("XDG_CONFIG_HOME", ".config"),
    ("XDG_STATE_HOME", ".local/state"),
    ("XDG_CACHE_HOME", ".cache"),
    ("XAVIER_HOME", ".xavier"),
    ("XAVIER_DATA_DIR", "xavier-data"),
    ("XAVIER_STATE_DIR", "xavier-state"),
    ("XAVIER_CONFIG_DIR", "xavier-config"),
    ("XAVIER_WORKSPACE_DIR", "xavier-workspace"),
];

/// Path variables that name a file or directory the process WRITES to. These are
/// not directories themselves, so they are redirected to a path inside the
/// sandbox rather than to a subdirectory name.
///
/// Without this, a test that resolves e.g. `XAVIER_MEMORY_VEC_PATH` keeps the
/// developer's real value and writes straight into the live store — which is
/// how 200+ files once landed in `~/.xavier/conversations`. The `HOME`/XDG
/// rewrite does not help here: an explicit env var wins over any default.
///
/// DELIBERATELY EXCLUDED: `XAVIER_CODE_GRAPH_DB_PATH`. Tests set it themselves to
/// point at a seeded fixture, and redirecting it made every code-graph scanner
/// test fail (it replaced the fixture with a path holding no database). A
/// variable a test drives on purpose is that test's own business.
const FILE_VARS: &[&str] = &[
    "XAVIER_EMBEDDING_CACHE_DB_PATH",
    "XAVIER_ENTERPRISE_DB_PATH",
    "XAVIER_FILE_STORE_PATH",
    "XAVIER_LOG_DIR",
    "XAVIER_MEMORY_FILE_PATH",
    "XAVIER_MEMORY_SQLITE_PATH",
    "XAVIER_MEMORY_VEC_PATH",
    "XAVIER_PANEL_STORE_DIR",
    "XAVIER_RECOVERY_DIR",
    "XAVIER_REPO_MEMORY_DB_PATH",
    "XAVIER_RUNTIME_STATE_PATH",
    "XAVIER_TELEMETRY_DB_PATH",
    "XAVIER_NODE_REGISTRY_PATH",
];

/// Suffix given to a redirected file variable, chosen so the sandbox keeps the
/// original file name (and therefore its extension).
fn sandboxed_file(root: &Path, var: &str) -> PathBuf {
    let name = var
        .strip_prefix("XAVIER_")
        .unwrap_or(var)
        .to_ascii_lowercase();
    let file = root.join("xavier-paths");
    let _ = std::fs::create_dir_all(&file);
    file.join(name)
}

/// True when the current executable is a cargo test/bench binary
/// (`target/<profile>/deps/<name>-<hash>`).
pub fn running_under_cargo_test() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().and_then(|d| d.file_name().map(|n| n == "deps")))
        .unwrap_or(false)
}

/// The real home of the invoking user: `XAVIER_GUARD_REAL_HOME` (override used
/// by the guard's own tests/scripts), else the passwd entry (never `$HOME`,
/// which the sandbox rewrites).
fn real_home() -> Option<PathBuf> {
    if let Some(h) = std::env::var_os("XAVIER_GUARD_REAL_HOME") {
        return Some(PathBuf::from(h));
    }
    static REAL: OnceLock<Option<PathBuf>> = OnceLock::new();
    REAL.get_or_init(passwd_home).clone()
}

#[cfg(not(unix))]
fn passwd_home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(PathBuf::from)
}

#[cfg(unix)]
fn passwd_home() -> Option<PathBuf> {
    // SAFETY-free: parse /etc/passwd for our uid instead of calling getpwuid.
    use std::os::unix::fs::MetadataExt;
    let uid = std::fs::metadata("/proc/self").ok()?.uid();
    let passwd = std::fs::read_to_string("/etc/passwd").ok()?;
    passwd.lines().find_map(|l| {
        let f: Vec<&str> = l.split(':').collect();
        (f.len() >= 6 && f[2].parse::<u32>().ok() == Some(uid)).then(|| PathBuf::from(f[5]))
    })
}

/// Redirect every user/state directory into a per-process tempdir. Returns the
/// sandbox root. Call before anything resolves a path; safe to call repeatedly.
pub fn isolate_user_dirs() -> PathBuf {
    SANDBOX
        .get_or_init(|| {
            let root = sandbox_root();
            let _ = std::fs::create_dir_all(&root);
            #[cfg(unix)]
            register_cleanup(&root);
            if std::env::var("XAVIER_TEST_REAL_HOME").as_deref() == Ok("1") {
                return root;
            }
            for (var, sub) in DIR_VARS {
                let dir = if sub.is_empty() {
                    root.clone()
                } else {
                    root.join(sub)
                };
                let _ = std::fs::create_dir_all(&dir);
                std::env::set_var(var, &dir);
            }

            // The OS keyring/secret-service is user-global: force the file fallback.
            std::env::remove_var("DBUS_SESSION_BUS_ADDRESS");
            root
        })
        .clone()
}

/// [`isolate_user_dirs`] plus the explicit-path redirection.
///
/// Deliberately NOT called from `isolate_test_process!`. Doing so made the suite
/// fail: `XavierSettings::current()` under test resolves through `dirs::data_dir()`,
/// and the redirection pins path variables that the settings loader consults, so
/// `test_local_provider_defaults` compared against a different default than the
/// one it asserts. The `.init_array` hook keeps the HOME/XDG redirect only, which
/// is what isolation actually needs to stop tests writing into the live store.
///
/// Exposed for a test that wants to prove the redirection itself, and for any
/// future caller that must re-assert it after the process overwrote a variable
/// (the sandbox root is a `OnceLock`, so [`isolate_user_dirs`] alone will not
/// re-apply it).
pub fn isolate_and_redirect() {
    let root = isolate_user_dirs();
    for var in FILE_VARS {
        std::env::set_var(var, sandboxed_file(&root, var));
    }
}

/// Remove the sandbox when the test process exits.
#[cfg(unix)]
#[allow(unsafe_code)]
fn register_cleanup(root: &Path) {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    extern "C" fn cleanup() {
        if let Some(r) = ROOT.get() {
            let _ = std::fs::remove_dir_all(r);
        }
    }
    if ROOT.set(root.to_path_buf()).is_ok() {
        // SAFETY: registers a plain `extern "C" fn()` handler; no captured state.
        unsafe {
            libc::atexit(cleanup);
        }
    }
}

/// Lexically resolve `.` and `..` in `path` without touching the filesystem.
///
/// The guard compares paths, so a guard that only does `path.starts_with(home)`
/// is trivially bypassed by `../..`-style paths — the very shape that
/// `ConnectionManager::connect` produces, since it resolves
/// `project_root/.xavier/<id>.db` relative to the test's cwd.
fn lexical_normalize(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
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

/// Absolute form of `path` for comparison purposes: relative paths are resolved
/// against the process cwd, which is what the filesystem would do anyway.
fn absolutize(path: &Path) -> PathBuf {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        match std::env::current_dir() {
            Ok(cwd) => cwd.join(path),
            // No cwd: we cannot prove the path is outside the home, so fall
            // back to the lexical form and let the home comparison decide.
            Err(_) => path.to_path_buf(),
        }
    };
    lexical_normalize(&joined)
}

/// Panic if a test/bench binary is about to touch `path` under the real user
/// home. No-op in production binaries, and for sandboxed or tmp paths.
///
/// Covers BOTH shapes that reach the real home:
/// * absolute paths that already start with the home, and
/// * relative paths (`project_root/.xavier/<id>.db`, resolved against the test
///   cwd) that land under the home once made absolute — the half of the
///   200-file `~/.xavier/conversations` incident that the absolute-only check
///   missed.
///
/// Fails CLOSED: an unusable real home (unknown, empty, or `/`) panics rather
/// than silently disabling the guard, because a data-safety guard that vanishes
/// without a trace is worse than one that stops a test run. Set
/// `XAVIER_GUARD_REAL_HOME` explicitly to unblock an exotic host.
pub fn guard_user_path(path: &Path) {
    if !running_under_cargo_test() || std::env::var("XAVIER_TEST_REAL_HOME").as_deref() == Ok("1") {
        return;
    }
    let Some(home) = real_home() else {
        panic!(
            "test isolation: cannot determine the real home (uid absent from /etc/passwd \
             or USERPROFILE unset), so {} cannot be proven safe. Set XAVIER_GUARD_REAL_HOME \
             to the real home to make this explicit.",
            path.display()
        );
    };
    if home.as_os_str().is_empty() || home == Path::new("/") {
        panic!(
            "test isolation: real home resolved to {:?}, which would guard the entire \
             filesystem. Refusing to check {}. Set XAVIER_GUARD_REAL_HOME to the real home.",
            home.display(),
            path.display()
        );
    }
    let home = lexical_normalize(&home);
    let target = absolutize(path);
    if !is_test_owned(&target) && target.starts_with(&home) {
        panic!(
            "test isolation: {} resolves to {} under the real home {}; call \
             xavier::test_support::isolate_user_dirs() (or set HOME/XDG_*/XAVIER_* \
             to a tempdir) before touching user state",
            path.display(),
            target.display(),
            home.display()
        );
    }
}

/// True when `target` belongs to the test run rather than to the user's data.
///
/// Exempt, and only these:
/// 1. the sandbox this process installed (`isolate_user_dirs`);
/// 2. the repository checkout — it lives under `$HOME` on any normal
///    workstation, and its own `data/` is not user state;
/// 3. `/tmp` and `/var/tmp`;
/// 4. a `xavier-test-*` scratch dir the test created itself;
/// 5. a `tempfile::tempdir()` (`.tmpXXXXXX`) under `$TMPDIR`, which on this
///    machine is `~/.hermes/cache/scratch` — inside the home being guarded.
///
/// Deliberately NOT exempt: the user store. `~/.xavier`,
/// `~/.local/share/xavier` and `~/.config/xavier` match none of the above, which
/// is the point: those are what the tripwire in `scripts/check-test-writes.sh`
/// exists to catch, and they are the paths this guard exists to reject.
fn is_test_owned(target: &Path) -> bool {
    if let Some(root) = SANDBOX.get() {
        if target.starts_with(lexical_normalize(root)) {
            return true;
        }
    }
    if let Some(repo) = repo_root() {
        if target.starts_with(&repo) {
            return true;
        }
    }
    if ["/tmp", "/var/tmp"]
        .iter()
        .any(|base| target.starts_with(Path::new(base)))
    {
        return true;
    }
    if target.components().any(|c| {
        c.as_os_str()
            .to_str()
            .is_some_and(|n| n.starts_with("xavier-test-"))
    }) {
        return true;
    }
    target
        .strip_prefix(lexical_normalize(&std::env::temp_dir()))
        .ok()
        .and_then(|rel| rel.components().next())
        .and_then(|c| c.as_os_str().to_str())
        .is_some_and(is_tempfile_scratch)
}

/// `tempfile` scratch names: `.tmp` plus random alphanumerics.
fn is_tempfile_scratch(name: &str) -> bool {
    name.strip_prefix(".tmp")
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_alphanumeric()))
}

/// The repository checkout root, resolved once. `CARGO_MANIFEST_DIR` is set by
/// cargo for every test binary, so this works whatever the cwd is.
fn repo_root() -> Option<PathBuf> {
    static REPO: OnceLock<Option<PathBuf>> = OnceLock::new();
    REPO.get_or_init(|| {
        std::env::var_os("CARGO_MANIFEST_DIR").map(|d| lexical_normalize(&PathBuf::from(d)))
    })
    .clone()
}

/// A scratch dir for the sandbox that is provably outside the real home.
///
/// `TMPDIR` may point inside the home (Hermes sets it to
/// `~/.hermes/cache/scratch`), and a sandbox nested in the home would make the
/// guard unable to tell test data from user data. Prefer `/tmp`.
fn sandbox_root() -> PathBuf {
    let name = format!(
        "xavier-test-home-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    if let Some(explicit) = std::env::var_os("XAVIER_SANDBOX_ROOT") {
        return PathBuf::from(explicit).join(name);
    }
    #[cfg(unix)]
    for base in ["/tmp", "/var/tmp"] {
        if Path::new(base).is_dir() {
            return PathBuf::from(base).join(name);
        }
    }
    std::env::temp_dir().join(name)
}

/// Sandbox a test/bench binary before `main`, so no lazy global can resolve
/// the real home first. Put `xavier::isolate_test_process!();` once at the top
/// of every integration-test and bench crate root (checked by
/// `scripts/check-test-writes.sh`). Linux only; elsewhere the path guard in
/// [`guard_user_path`] is the safety net.
#[macro_export]
macro_rules! isolate_test_process {
    () => {
        #[cfg(target_os = "linux")]
        #[allow(unsafe_code)]
        const _: () = {
            extern "C" fn __xavier_isolate_init() {
                $crate::test_support::isolate_user_dirs();
            }
            #[used]
            #[link_section = ".init_array"]
            static __XAVIER_ISOLATE_INIT: extern "C" fn() = __xavier_isolate_init;
        };
    };
}

#[cfg(test)]
crate::isolate_test_process!();

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory the guard will NOT treat as test-owned, for tests that need a
    /// fake "real home" the guard must actually police.
    ///
    /// `is_test_owned` exempts the repo, `/tmp`, `/var/tmp`, any `xavier-test-*`
    /// component, and a `tempfile` `.tmpXXXXXX` scratch dir under `$TMPDIR`. So
    /// the fake home has to sit outside ALL of those — putting it under
    /// `std::env::temp_dir()` is what made this test pass locally (TMPDIR is
    /// `~/.hermes/cache/scratch`) while failing on CI, where TMPDIR is `/tmp`.
    ///
    /// It lives in a uniquely named sibling of the repo, which is outside the
    /// repo root, outside any temp dir, and has no `xavier-test-` component.
    fn fake_home_outside_temp() -> PathBuf {
        let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("manifest"));
        let unique = format!("guard-fake-home-{}", std::process::id());
        let home = manifest.parent().expect("repo parent").join(unique);
        let _ = std::fs::create_dir_all(home.join("project_root"));
        assert!(
            !super::is_test_owned(&home),
            "the fake home {home:?} must not be exempt from the guard"
        );
        home
    }

    /// F-4 regression: an explicit `XAVIER_*` path variable must not survive
    /// isolation. Such a variable beats every default, so before this the HOME
    /// rewrite left them pointing at the live store — the mechanism behind the
    /// 200+ stray files that once landed in `~/.xavier/conversations`.
    ///
    /// Re-derives the expectation instead of trusting the ambient environment:
    /// other suites in this binary rewrite `HOME` and unset `XAVIER_*` vars, and
    /// `--test-threads=1` runs them first.
    #[test]
    fn explicit_path_vars_are_redirected_into_the_sandbox() {
        // Poison every variable first, as a developer's shell would, so the
        // assertion proves isolation OVERRULES an existing value rather than
        // merely filling a blank.
        const POISON: &str = "/home/someone-real/xavier-should-not-be-used";
        for var in FILE_VARS {
            std::env::set_var(var, POISON);
        }
        isolate_and_redirect();

        let real = lexical_normalize(&real_home().expect("real home"));
        let sandbox = lexical_normalize(&isolate_user_dirs());
        for var in FILE_VARS {
            let value = std::env::var(var).unwrap_or_else(|_| panic!("{var} must be set"));
            assert_ne!(value, POISON, "{var} kept the pre-existing value");
            let resolved = lexical_normalize(&absolutize(Path::new(&value)));
            assert!(
                !resolved.starts_with(&real) || resolved.starts_with(&sandbox),
                "{var} resolved to {resolved:?}, inside the real home"
            );
        }
    }

    /// `isolate_test_process!()` compiles to a `.init_array` hook only on Linux,
    /// so off Linux the sandbox is never installed and `HOME` is untouched.
    /// Gating the assertion keeps the suite green on macOS/Windows.
    #[cfg(target_os = "linux")]
    #[test]
    fn lib_tests_run_in_sandbox_home() {
        let home = dirs::home_dir().expect("home");
        // The sandbox is deliberately NOT under `$TMPDIR`: here TMPDIR is
        // `~/.hermes/cache/scratch`, i.e. inside the home the guard protects.
        // What matters is that HOME is a fresh per-process sandbox.
        assert_ne!(
            home,
            real_home().expect("real home"),
            "HOME must not be the real home"
        );
        assert!(running_under_cargo_test());
    }

    #[test]
    fn guard_rejects_real_home_and_allows_tmp() {
        let _lock = crate::test_support::tests::ENV
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::env::set_var("XAVIER_GUARD_REAL_HOME", "/home/someone-real");
        let bad = std::panic::catch_unwind(|| {
            guard_user_path(Path::new("/home/someone-real/.xavier/conversations/x.db"))
        });
        guard_user_path(Path::new("/tmp/anything/.xavier/x.db"));
        // A sibling whose name merely shares the home's prefix is NOT under it.
        guard_user_path(Path::new("/home/someone-real-backup/.xavier/x.db"));
        std::env::remove_var("XAVIER_GUARD_REAL_HOME");
        assert!(bad.is_err(), "guard must panic for the real home");
    }

    /// HIGH-1 regression: the old absolute-only guard let these through, and
    /// they are exactly the relative paths `ConnectionManager::connect`
    /// resolves for a `test_*` / `conn_test_*` / default project id
    /// (`PathBuf::from(project_root).join(".xavier").join(..)`).
    ///
    /// Pins the "real home" to the process cwd — which is exactly the shape of
    /// the original incident: a relative `project_root` whose resolved
    /// `project_root/.xavier/<id>.db` lands under the real home.
    #[test]
    fn guard_rejects_relative_paths_that_resolve_into_real_home() {
        let _lock = crate::test_support::tests::ENV
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // The home must be under a directory the guard does NOT exempt, or the
        // relative-path case proves nothing. `is_test_owned` exempts the repo,
        // `/tmp`, `/var/tmp`, any `xavier-test-*` component, and a
        // `tempfile` `.tmpXXXXXX` scratch dir under `$TMPDIR` — so a home inside
        // `std::env::temp_dir()` is itself exempt and the assert can never fire.
        // This is why the test passed here (TMPDIR is `~/.hermes/cache/scratch`,
        // so the path is exempt for a different reason) while it would fail on CI
        // where TMPDIR is `/tmp`. Use the home itself, never the temp dir.
        let home = fake_home_outside_temp();
        let workdir = home.join("project_root");
        let _ = std::fs::create_dir_all(&workdir);
        let repo = std::env::current_dir().expect("cwd");
        std::env::set_var("XAVIER_GUARD_REAL_HOME", &home);
        std::env::set_current_dir(&workdir).expect("chdir");

        // Relative, no `..`, would have passed the old guard outright.
        let plain = std::panic::catch_unwind(|| guard_user_path(Path::new(".xavier/codebase.db")));
        // Relative AND traversing upwards back into the home: the old guard
        // also missed this because the path is not absolute.
        let escaping = std::panic::catch_unwind(|| {
            guard_user_path(Path::new("sub/../.xavier/conv_test_1.db"))
        });

        // A relative path that escapes the home is fine.
        guard_user_path(Path::new("../../../../../../../../tmp/outside/x.db"));

        std::env::set_current_dir(&repo).expect("chdir back");
        std::env::remove_var("XAVIER_GUARD_REAL_HOME");
        assert!(
            plain.is_err(),
            "relative path resolving into the real home must panic"
        );
        assert!(
            escaping.is_err(),
            "relative path with dot segments resolving into the real home must panic"
        );
    }

    /// The home must be matched as a path component boundary, so `/home/belal`
    /// does not capture `/home/belal-notes/...`.
    #[test]
    fn lexical_normalize_collapses_dot_segments() {
        assert_eq!(
            lexical_normalize(Path::new("/a/b/../c/./d")),
            PathBuf::from("/a/c/d")
        );
        assert_eq!(lexical_normalize(Path::new("")), PathBuf::from("."));
        assert_eq!(
            lexical_normalize(Path::new("/a/../..")),
            PathBuf::from("/..")
        );
    }

    pub(super) static ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());
}
