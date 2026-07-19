use std::sync::Arc;
use parking_lot::Mutex;
use std::sync::OnceLock;
use parking_lot::RwLock;
use crate::agents::provider::hardware;

#[derive(Clone)]
pub struct ManagedLlamaServer {
    child: Arc<Mutex<Option<tokio::process::Child>>>,
    port: u16,
}

pub fn build_args(model_path: &str, port: u16, gpu_info: &hardware::GpuInfo) -> Vec<String> {
    let mut args = vec![
        "--model".to_string(),
        model_path.to_string(),
        "--port".to_string(),
        port.to_string(),
        "--ctx-size".to_string(),
        "4096".to_string(),
    ];
    if gpu_info.vendor == hardware::GpuVendor::Nvidia || gpu_info.vendor == hardware::GpuVendor::Amd {
        args.push("--n-gpu-layers".to_string());
        args.push("35".to_string());
    }
    args
}

impl ManagedLlamaServer {
    pub async fn start_server(model_path: &str, gpu_info: &hardware::GpuInfo) -> Result<Self, std::io::Error> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        drop(listener);

        let exe = if cfg!(target_os = "windows") {
            "llama-server.exe"
        } else {
            "llama-server"
        };

        let args = build_args(model_path, port, gpu_info);
        let mut cmd = tokio::process::Command::new(exe);
        cmd.args(&args);

        let child = cmd.spawn()?;

        Ok(Self {
            child: Arc::new(Mutex::new(Some(child))),
            port,
        })
    }

    pub async fn stop_server(&self) -> Result<(), std::io::Error> {
        let mut guard = self.child.lock();
        if let Some(mut child) = guard.take() {
            child.kill().await?;
        }
        Ok(())
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

static GLOBAL_LLAMA_SERVER: OnceLock<RwLock<Option<ManagedLlamaServer>>> = OnceLock::new();

pub fn get_global_llama_server() -> &'static RwLock<Option<ManagedLlamaServer>> {
    GLOBAL_LLAMA_SERVER.get_or_init(|| RwLock::new(None))
}

pub fn get_managed_server_port() -> Option<u16> {
    get_global_llama_server().read().as_ref().map(|s| s.port())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::provider::hardware::{GpuInfo, GpuVendor};

    #[test]
    fn test_build_args_nvidia() {
        let gpu_info = GpuInfo {
            vendor: GpuVendor::Nvidia,
            vram_bytes: 8 * 1024 * 1024 * 1024,
        };
        let args = build_args("my_model.gguf", 8080, &gpu_info);
        assert_eq!(args, vec![
            "--model", "my_model.gguf",
            "--port", "8080",
            "--ctx-size", "4096",
            "--n-gpu-layers", "35"
        ]);
    }

    #[test]
    fn test_build_args_amd() {
        let gpu_info = GpuInfo {
            vendor: GpuVendor::Amd,
            vram_bytes: 8 * 1024 * 1024 * 1024,
        };
        let args = build_args("my_model.gguf", 8080, &gpu_info);
        assert_eq!(args, vec![
            "--model", "my_model.gguf",
            "--port", "8080",
            "--ctx-size", "4096",
            "--n-gpu-layers", "35"
        ]);
    }

    #[test]
    fn test_build_args_unknown() {
        let gpu_info = GpuInfo {
            vendor: GpuVendor::Unknown,
            vram_bytes: 0,
        };
        let args = build_args("my_model.gguf", 8080, &gpu_info);
        assert_eq!(args, vec![
            "--model", "my_model.gguf",
            "--port", "8080",
            "--ctx-size", "4096"
        ]);
    }
}
