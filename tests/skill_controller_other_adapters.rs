//! Offline projection and native adapter catalog checks for #2621.
//! Ignored probes are unsupported, never native discovery passes.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use xavier::context::skill_controller::{
    manifest::Manifest, plan::ResolvedRoots, planner, policy::FsPolicy, projector::apply_plan,
    state::Journal,
};

xavier::isolate_test_process!();

struct Fixture {
    temp: tempfile::TempDir,
    home: PathBuf,
    project: PathBuf,
    source: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let project = temp.path().join("project");
        let source = temp.path().join("canonical");
        for dir in [&home, &project, &source] {
            fs::create_dir_all(dir).unwrap();
        }
        for relative in [
            ".codex",
            ".config",
            ".cache",
            ".local/share",
            ".local/state",
        ] {
            fs::create_dir_all(home.join(relative)).unwrap();
        }
        for name in ["alpha-visible", "beta-visible", "gamma-hidden"] {
            let dir = source.join(name);
            fs::create_dir_all(dir.join("references")).unwrap();
            fs::write(
                dir.join("SKILL.md"),
                format!(
                    "---\nname: {name}\ndescription: Isolated adapter fixture\n---\n# {name}\n"
                ),
            )
            .unwrap();
            fs::write(
                dir.join("references/source.txt"),
                b"readable without Xavier\n",
            )
            .unwrap();
        }
        Self {
            temp,
            home,
            project,
            source,
        }
    }

    fn project_into(&self, root: &Path, tool: &str, names: &[&str]) {
        fs::create_dir_all(root).unwrap();
        let mut text = format!(
            "version = 1\n[sources]\ncanonical = \"$STORE\"\n\
             [[targets]]\nid = \"view\"\ntools = [\"{tool}\"]\nroot = \"$VIEW\"\nmode = \"symlink\"\n"
        );
        for name in names {
            text.push_str(&format!(
                "[[placements]]\nsource = \"canonical\"\npath = \"{name}\"\nname = \"{name}\"\ntargets = [\"view\"]\n"
            ));
        }
        let manifest = Manifest::from_toml(&text).unwrap();
        let roots = ResolvedRoots {
            sources: BTreeMap::from([("canonical".into(), self.source.clone())]),
            targets: BTreeMap::from([("view".into(), root.to_path_buf())]),
        };
        let policy =
            FsPolicy::new(std::slice::from_ref(&self.source), &[root.to_path_buf()]).unwrap();
        let journal = Journal::new(self.temp.path().join("journal")).unwrap();
        let plan = planner::build_plan(&manifest, &policy, &roots).unwrap();
        let dry = apply_plan(&plan, &manifest, &policy, &roots, &journal, "dry", true).unwrap();
        assert!(dry.dry_run);
        assert_eq!(fs::read_dir(root).unwrap().count(), 0);
        let applied =
            apply_plan(&plan, &manifest, &policy, &roots, &journal, "apply", false).unwrap();
        assert!(!applied.dry_run);
        assert_eq!(applied.applied.len(), names.len());
    }

    fn assert_view(&self, root: &Path, expected: &[&str]) {
        let mut visible: Vec<_> = fs::read_dir(root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        visible.sort();
        let mut expected = expected.to_vec();
        expected.sort();
        assert_eq!(visible, expected);
        assert!(!root.join("gamma-hidden").exists());
        for name in expected {
            let path = root.join(name);
            assert!(fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink());
            assert_eq!(fs::canonicalize(&path).unwrap(), self.source.join(name));
            for member in ["SKILL.md", "references/source.txt"] {
                assert_eq!(
                    fs::read(path.join(member)).unwrap(),
                    fs::read(self.source.join(name).join(member)).unwrap()
                );
            }
        }
    }
}

// Block all child socket creation and connections, including localhost:8006.
// Unlike proxy variables, seccomp is inherited across exec and descendants.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn deny_network(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    unsafe {
        cmd.pre_exec(|| {
            let mut filter = [
                libc::sock_filter {
                    code: 0x20,
                    jt: 0,
                    jf: 0,
                    k: 0,
                },
                libc::sock_filter {
                    code: 0x15,
                    jt: 2,
                    jf: 0,
                    k: libc::SYS_socket as u32,
                },
                libc::sock_filter {
                    code: 0x15,
                    jt: 1,
                    jf: 0,
                    k: libc::SYS_connect as u32,
                },
                libc::sock_filter {
                    code: 0x06,
                    jt: 0,
                    jf: 0,
                    k: libc::SECCOMP_RET_ALLOW,
                },
                libc::sock_filter {
                    code: 0x06,
                    jt: 0,
                    jf: 0,
                    k: libc::SECCOMP_RET_ERRNO | libc::EPERM as u32,
                },
            ];
            let program = libc::sock_fprog {
                len: filter.len() as u16,
                filter: filter.as_mut_ptr(),
            };
            if libc::setpgid(0, 0) != 0
                || libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0
                || libc::prctl(libc::PR_SET_SECCOMP, libc::SECCOMP_MODE_FILTER, &program) != 0
            {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

fn offline_catalog(fixture: &Fixture, cli: &str, args: &[&str], expected: &[&str]) {
    let Some(executable) = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(cli))
            .find(|path| path.is_file())
    }) else {
        panic!("UNAVAILABLE: {cli} not on PATH; native discovery NOT VERIFIED");
    };
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        panic!("UNSUPPORTED: network sandbox requires Linux x86_64");
    }
    let output_path = fixture.temp.path().join("catalog-output");
    let output = fs::File::create(&output_path).unwrap();
    let mut cmd = Command::new(executable);
    cmd.args(args)
        .current_dir(&fixture.project)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", &fixture.home)
        .env("XDG_CONFIG_HOME", fixture.home.join(".config"))
        .env("XDG_DATA_HOME", fixture.home.join(".local/share"))
        .env("XDG_STATE_HOME", fixture.home.join(".local/state"))
        .env("XDG_CACHE_HOME", fixture.home.join(".cache"))
        .env("CODEX_HOME", fixture.home.join(".codex"))
        .env("GEMINI_HOME", fixture.home.join(".gemini"))
        .env("HERMES_HOME", fixture.home.join(".hermes"))
        .env("OPENCLAW_STATE_DIR", fixture.home.join(".openclaw"))
        .env("TMPDIR", fixture.temp.path())
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(output.try_clone().unwrap())
        .stderr(output);
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    deny_network(&mut cmd);
    let mut child = cmd.spawn().unwrap();
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if start.elapsed() > Duration::from_secs(30)
            || fs::metadata(&output_path).unwrap().len() > 1_048_576
        {
            #[cfg(unix)]
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            child.kill().ok();
            child.wait().unwrap();
            panic!("UNSUPPORTED {cli}: offline startup exceeded time/output bound");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(fs::metadata(&output_path).unwrap().len() <= 1_048_576);
    // Do not print foreign output: it may contain unrelated sensitive material.
    assert!(
        status.success(),
        "UNSUPPORTED {cli}: offline catalog command exited {status}"
    );
    let output = fs::read_to_string(output_path).unwrap();
    let mut observed: Vec<_> = ["alpha-visible", "beta-visible", "gamma-hidden"]
        .into_iter()
        .filter(|name| output.contains(name))
        .collect();
    observed.sort();
    let mut expected = expected.to_vec();
    expected.sort();
    assert_eq!(observed, expected, "{cli}: exact fixture catalog differs");
    eprintln!("VERIFIED {cli}: fresh offline catalog; fixture names={observed:?}; symlink source readable");
}

const HERMES_GLOBAL: &str = ".hermes/skills";
const OPENCLAW_GLOBAL: &str = ".openclaw/skills";

#[test]
fn hermes_global_projection_without_project_leakage() {
    xavier::isolate_test_process!();
    let f = Fixture::new();
    f.project_into(&f.home.join(HERMES_GLOBAL), "hermes", &["alpha-visible"]);
    f.project_into(
        &f.project.join(".agents/skills"),
        "hermes",
        &["beta-visible"],
    );
    f.assert_view(&f.home.join(".hermes/skills"), &["alpha-visible"]);
    f.assert_view(&f.project.join(".agents/skills"), &["beta-visible"]);
    // Untrusted project discovery is not enabled by modifying CLI configuration.
}

#[test]
fn openclaw_global_discovery_without_project_leakage() {
    xavier::isolate_test_process!();
    let f = Fixture::new();
    f.project_into(
        &f.home.join(OPENCLAW_GLOBAL),
        "openclaw",
        &["alpha-visible"],
    );
    f.project_into(
        &f.project.join(".agents/skills"),
        "openclaw",
        &["beta-visible"],
    );
    f.assert_view(&f.home.join(".openclaw/skills"), &["alpha-visible"]);
    f.assert_view(&f.project.join(".agents/skills"), &["beta-visible"]);
    offline_catalog(
        &f,
        "openclaw",
        &["skills", "list", "--json"],
        &["alpha-visible"],
    );
}

#[test]
fn gemini_global_and_project_projection() {
    xavier::isolate_test_process!();
    let f = Fixture::new();
    f.project_into(&f.home.join(".gemini/skills"), "gemini", &["alpha-visible"]);
    f.project_into(
        &f.project.join(".agents/skills"),
        "gemini",
        &["beta-visible"],
    );
    f.assert_view(&f.home.join(".gemini/skills"), &["alpha-visible"]);
    f.assert_view(&f.project.join(".agents/skills"), &["beta-visible"]);
}

#[test]
#[ignore = "#2621: gemini not installed; run with --ignored where available"]
fn gemini_native_startup_global_and_project_catalog() {
    xavier::isolate_test_process!();
    let f = Fixture::new();
    f.project_into(&f.home.join(".gemini/skills"), "gemini", &["alpha-visible"]);
    f.project_into(
        &f.project.join(".agents/skills"),
        "gemini",
        &["beta-visible"],
    );
    offline_catalog(
        &f,
        "gemini",
        &["skills", "list"],
        &["alpha-visible", "beta-visible"],
    );
}

#[test]
fn agy_global_and_project_projection() {
    xavier::isolate_test_process!();
    let f = Fixture::new();
    f.project_into(
        &f.home.join(".gemini/config/skills"),
        "agy",
        &["alpha-visible"],
    );
    f.project_into(&f.project.join(".agents/skills"), "agy", &["beta-visible"]);
    f.assert_view(&f.home.join(".gemini/config/skills"), &["alpha-visible"]);
    f.assert_view(&f.project.join(".agents/skills"), &["beta-visible"]);
}

#[test]
#[ignore = "#2621: installed agy wrapper ignores HOME and writes real account cache; unsafe to launch hermetically"]
fn agy_native_startup_unsupported() {
    xavier::isolate_test_process!();
    panic!("UNSUPPORTED: agy wrapper must honor isolated HOME before native discovery is verified");
}

#[test]
#[ignore = "#2621: installed Hermes launcher attempts dependency downloads in offline startup; native discovery unverified"]
fn hermes_native_startup_unsupported() {
    xavier::isolate_test_process!();
    let f = Fixture::new();
    f.project_into(&f.home.join(HERMES_GLOBAL), "hermes", &["alpha-visible"]);
    f.project_into(
        &f.project.join(".agents/skills"),
        "hermes",
        &["beta-visible"],
    );
    offline_catalog(&f, "hermes", &["skills", "list"], &["alpha-visible"]);
}
