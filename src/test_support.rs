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
            let root = std::env::temp_dir().join(format!(
                "xavier-test-home-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            ));
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
    if target.starts_with(&home) {
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

    /// `isolate_test_process!()` compiles to a `.init_array` hook only on Linux,
    /// so off Linux the sandbox is never installed and `HOME` is untouched.
    /// Gating the assertion keeps the suite green on macOS/Windows.
    #[cfg(target_os = "linux")]
    #[test]
    fn lib_tests_run_in_sandbox_home() {
        let home = dirs::home_dir().expect("home");
        assert!(home.starts_with(std::env::temp_dir()), "HOME={home:?}");
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
        let cwd = std::env::current_dir().expect("cwd");
        std::env::set_var("XAVIER_GUARD_REAL_HOME", &cwd);

        // Relative, no `..`, would have passed the old guard outright.
        let plain = std::panic::catch_unwind(|| guard_user_path(Path::new(".xavier/codebase.db")));
        // Relative AND traversing upwards back into the home: the old guard
        // also missed this because the path is not absolute.
        let escaping = std::panic::catch_unwind(|| {
            guard_user_path(Path::new("sub/../.xavier/conv_test_1.db"))
        });

        // A relative path that escapes the home is fine.
        guard_user_path(Path::new("../../../../../../../../tmp/outside/x.db"));

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
