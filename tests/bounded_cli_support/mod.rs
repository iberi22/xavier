#![allow(dead_code)]
mod mock;
pub use mock::MockServer;
use std::fs::{self, File};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub struct Sandbox(pub tempfile::TempDir);
impl Sandbox {
    pub fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("xavier-test-bounded-")
            .tempdir()
            .unwrap();
        for name in [
            "home", "data", "config", "state", "cache", "share", "run", "bin",
        ] {
            fs::create_dir_all(dir.path().join(name)).unwrap();
        }
        Self(dir)
    }
    pub fn command(&self, url: &str) -> Command {
        let root = self.0.path();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_xavier"));
        cmd.env_clear().current_dir(root).stdin(Stdio::null());
        for (key, path) in [
            ("HOME", "home"),
            ("USERPROFILE", "home"),
            ("PATH", "bin"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_STATE_HOME", "state"),
            ("XDG_DATA_HOME", "share"),
            ("XDG_CACHE_HOME", "cache"),
            ("XDG_RUNTIME_DIR", "run"),
            ("XAVIER_HOME", "home/.xavier"),
            ("XAVIER_DATA_DIR", "data"),
            ("XAVIER_STATE_DIR", "state"),
            ("XAVIER_CONFIG_DIR", "config"),
            ("XAVIER_WORKSPACE_DIR", "data/workspace"),
            ("XAVIER_LOG_DIR", "logs"),
            ("XAVIER_MEMORY_VEC_PATH", "data/memory.db"),
            ("XAVIER_CODE_GRAPH_DB_PATH", "data/code.db"),
            ("XAVIER_EMBEDDING_CACHE_DB_PATH", "data/cache.db"),
        ] {
            cmd.env(key, root.join(path));
        }
        cmd.env("XAVIER_TOKEN", "bounded-test-token")
            .env("XAVIER_URL", url)
            .env("OLLAMA_HOST", url)
            .env("XAVIER_EMBEDDER", "openai-compatible")
            .env("XAVIER_EMBEDDING_PROVIDER_MODE", "local")
            .env("XAVIER_EMBEDDING_URL", format!("{url}/v1/embeddings"))
            .env("XAVIER_EMBEDDING_LOCAL_URL", format!("{url}/v1/embeddings"))
            .env("XAVIER_LOCAL_LLM_URL", format!("{url}/v1"))
            .env("XAVIER_EMBEDDING_MODEL", "nomic-embed-text")
            .env("NO_PROXY", "127.0.0.1");
        cmd
    }
    pub fn run(&self, cmd: &mut Command, bound: Duration) -> Run {
        let out = self.0.path().join("stdout");
        let err = self.0.path().join("stderr");
        cmd.stdout(File::create(&out).unwrap())
            .stderr(File::create(&err).unwrap());
        let start = Instant::now();
        let mut child = ChildGuard(cmd.spawn().expect("spawn CLI binary"));
        let status = loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                break status;
            }
            if start.elapsed() >= bound {
                child.0.kill().unwrap();
                child.0.wait().unwrap();
                panic!(
                    "CLI exceeded {bound:?}\n{}\n{}",
                    fs::read_to_string(&out).unwrap(),
                    fs::read_to_string(&err).unwrap()
                );
            }
            thread::sleep(Duration::from_millis(10));
        };
        Run {
            status,
            elapsed: start.elapsed(),
            stdout: fs::read_to_string(out).unwrap(),
            stderr: fs::read_to_string(err).unwrap(),
        }
    }
}
struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[derive(Debug)]
pub struct Run {
    pub status: ExitStatus,
    pub elapsed: Duration,
    pub stdout: String,
    pub stderr: String,
}
