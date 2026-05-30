//! System and provider capability scanner
//!
//! Detects CPU features, RAM, disk space, GPU availability,
//! and probes external embedding providers.

use std::fmt;
use std::sync::LazyLock;
use std::time::Duration;

/// Detected system capabilities
#[derive(Debug, Clone)]
pub struct SystemCapabilities {
    /// Number of logical CPU cores
    pub cpu_cores: usize,
    /// Total system RAM in GB
    pub ram_gb: f64,
    /// Available disk space in GB
    pub disk_free_gb: f64,
    /// CPU supports AVX2 (needed for some embedding models)
    pub has_avx2: bool,
    /// CPU supports AVX-512
    pub has_avx512: bool,
    /// CUDA available (NVIDIA GPU)
    pub has_cuda: bool,
    /// Vulkan available (GPU compute)
    pub has_vulkan: bool,
    /// Metal available (Apple GPU, check at build)
    pub has_metal: bool,
    /// NVIDIA GPU present (via nvidia-smi)
    pub has_nvidia_gpu: bool,
    /// System is Windows
    pub is_windows: bool,
    /// System is Linux
    pub is_linux: bool,
    /// System is macOS
    pub is_macos: bool,
}

impl fmt::Display for SystemCapabilities {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "💻 System:")?;
        writeln!(f, "  CPU Cores: {}", self.cpu_cores)?;
        writeln!(f, "  RAM: {:.1} GB", self.ram_gb)?;
        writeln!(f, "  Disk Free: {:.1} GB", self.disk_free_gb)?;
        writeln!(f, "  AVX2: {}", if self.has_avx2 { "✅" } else { "❌" })?;
        writeln!(
            f,
            "  AVX-512: {}",
            if self.has_avx512 { "✅" } else { "❌" }
        )?;
        writeln!(f, "  CUDA: {}", if self.has_cuda { "✅" } else { "❌" })?;
        writeln!(
            f,
            "  NVIDIA GPU: {}",
            if self.has_nvidia_gpu { "✅" } else { "❌" }
        )?;
        writeln!(f, "  Vulkan: {}", if self.has_vulkan { "✅" } else { "❌" })?;
        writeln!(f, "  Metal: {}", if self.has_metal { "✅" } else { "❌" })?;
        writeln!(
            f,
            "  OS: {}",
            if self.is_windows {
                "Windows"
            } else if self.is_linux {
                "Linux"
            } else if self.is_macos {
                "macOS"
            } else {
                "Unknown"
            }
        )
    }
}

// Helper module for CPU feature detection on Windows
#[cfg(target_os = "windows")]
mod cpu_features {
    pub fn has_avx2() -> bool {
        true // Safe default — most modern x86_64 have AVX2
    }

    pub fn has_avx512() -> bool {
        false // Rare on consumer CPUs
    }

    pub fn has_vulkan_gpu() -> bool {
        // Probe vulkan-1.dll via LoadLibraryW
        #[link(name = "kernel32")]
        extern "system" {
            fn LoadLibraryW(lpLibFileName: *const u16) -> isize;
            fn FreeLibrary(hLibModule: isize) -> i32;
        }
        unsafe {
            let name: Vec<u16> = "vulkan-1.dll\0".encode_utf16().collect();
            let lib = LoadLibraryW(name.as_ptr());
            if lib != 0 {
                FreeLibrary(lib);
                true
            } else {
                false
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod cpu_features {
    pub fn has_avx2() -> bool {
        // Read /proc/cpuinfo flags
        std::fs::read_to_string("/proc/cpuinfo")
            .map(|s| s.contains("avx2"))
            .unwrap_or(false)
    }

    pub fn has_avx512() -> bool {
        std::fs::read_to_string("/proc/cpuinfo")
            .map(|s| s.contains("avx512"))
            .unwrap_or(false)
    }

    pub fn has_vulkan_gpu() -> bool {
        // Check if vulkan-info or libvulkan is available
        std::process::Command::new("which")
            .arg("vulkaninfo")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

#[cfg(target_os = "macos")]
mod cpu_features {
    pub fn has_avx2() -> bool {
        // sysctl machdep.cpu.features
        std::process::Command::new("sysctl")
            .arg("-n")
            .arg("machdep.cpu.features")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.contains("AVX2"))
            .unwrap_or(false)
    }

    pub fn has_avx512() -> bool {
        false // Apple Silicon doesn't have AVX-512
    }

    pub fn has_vulkan_gpu() -> bool {
        false // Metal is the primary API on macOS
    }
}

// Fallback for unknown targets
#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
mod cpu_features {
    pub fn has_avx2() -> bool {
        false
    }
    pub fn has_avx512() -> bool {
        false
    }
    pub fn has_vulkan_gpu() -> bool {
        false
    }
}

/// Detect CUDA availability
fn check_cuda() -> bool {
    // Check nvidia-smi
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("nvidia-smi")
            .arg("--query-gpu=name")
            .arg("--format=csv,noheader")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new("nvidia-smi")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

/// Detect Metal availability
fn check_metal() -> bool {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("metal")
            .arg("-v")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Probe system capabilities
pub fn scan_system() -> Result<SystemCapabilities, String> {
    let cpu_cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);

    let ram_gb = detect_ram_gb();
    let disk_free_gb = detect_disk_free_gb();

    Ok(SystemCapabilities {
        cpu_cores,
        ram_gb,
        disk_free_gb,
        has_avx2: cpu_features::has_avx2(),
        has_avx512: cpu_features::has_avx512(),
        has_cuda: check_cuda(),
        has_vulkan: cpu_features::has_vulkan_gpu(),
        has_metal: check_metal(),
        has_nvidia_gpu: check_cuda(),
        is_windows: cfg!(target_os = "windows"),
        is_linux: cfg!(target_os = "linux"),
        is_macos: cfg!(target_os = "macos"),
    })
}

/// Detect RAM using OS-specific APIs (no external dependencies)
#[cfg(target_os = "windows")]
fn detect_ram_gb() -> f64 {
    #[repr(C)]
    struct MEMORYSTATUSEX {
        dwLength: u32,
        dwMemoryLoad: u32,
        ullTotalPhys: u64,
        ullAvailPhys: u64,
        ullTotalPageFile: u64,
        ullAvailPageFile: u64,
        ullTotalVirtual: u64,
        ullAvailVirtual: u64,
        ullAvailExtendedVirtual: u64,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalMemoryStatusEx(lpBuffer: *mut MEMORYSTATUSEX) -> i32;
    }

    unsafe {
        let mut status = MEMORYSTATUSEX {
            dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            dwMemoryLoad: 0,
            ullTotalPhys: 0,
            ullAvailPhys: 0,
            ullTotalPageFile: 0,
            ullAvailPageFile: 0,
            ullTotalVirtual: 0,
            ullAvailVirtual: 0,
            ullAvailExtendedVirtual: 0,
        };
        if GlobalMemoryStatusEx(&mut status) != 0 {
            status.ullTotalPhys as f64 / (1024.0 * 1024.0 * 1024.0)
        } else {
            8.0
        }
    }
}

#[cfg(target_os = "linux")]
fn detect_ram_gb() -> f64 {
    std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("MemTotal:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse::<f64>().ok())
                .map(|kb| kb / (1024.0 * 1024.0))
        })
        .unwrap_or(8.0)
}

#[cfg(target_os = "macos")]
fn detect_ram_gb() -> f64 {
    std::process::Command::new("sysctl")
        .args(["-n", "hw.memsize"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse::<f64>().ok())
        .map(|b| b / (1024.0 * 1024.0 * 1024.0))
        .unwrap_or(8.0)
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn detect_ram_gb() -> f64 {
    8.0
}

/// Detect free disk space using OS-specific APIs
#[cfg(target_os = "windows")]
fn detect_disk_free_gb() -> f64 {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetDiskFreeSpaceExW(
            lpDirectoryName: *const u16,
            lpFreeBytesAvailableToCaller: *mut u64,
            lpTotalNumberOfBytes: *mut u64,
            lpTotalNumberOfFreeBytes: *mut u64,
        ) -> i32;
    }

    let current_dir = std::env::current_dir().unwrap_or_default();
    let path: Vec<u16> = current_dir
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let mut free_bytes: u64 = 0;
        let mut _total_bytes: u64 = 0;
        let mut _total_free: u64 = 0;
        if GetDiskFreeSpaceExW(
            path.as_ptr(),
            &mut free_bytes,
            &mut _total_bytes,
            &mut _total_free,
        ) != 0
        {
            free_bytes as f64 / (1024.0 * 1024.0 * 1024.0)
        } else {
            10.0
        }
    }
}

#[cfg(target_os = "linux")]
fn detect_disk_free_gb() -> f64 {
    std::fs::read_to_string("/proc/mounts")
        .ok()
        .and_then(|_| {
            // Use statvfs on current dir
            let current = std::env::current_dir().unwrap_or_default();
            let cstr = std::ffi::CString::new(current.to_string_lossy().as_bytes()).ok()?;
            unsafe {
                let mut stat: libc::statvfs = std::mem::zeroed();
                if libc::statvfs(cstr.as_ptr(), &mut stat) == 0 {
                    Some((stat.f_bsize * stat.f_bavail) as f64 / (1024.0 * 1024.0 * 1024.0))
                } else {
                    None
                }
            }
        })
        .unwrap_or(10.0)
}

#[cfg(target_os = "macos")]
fn detect_disk_free_gb() -> f64 {
    std::process::Command::new("df")
        .args(["-k", "."])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| {
            s.lines()
                .nth(1)
                .and_then(|l| l.split_whitespace().nth(3))
                .and_then(|v| v.parse::<f64>().ok())
        })
        .map(|kb| kb / (1024.0 * 1024.0))
        .unwrap_or(10.0)
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn detect_disk_free_gb() -> f64 {
    10.0
}

/// Provider status after probing
#[derive(Debug, Clone)]
pub struct ProviderStatus {
    pub name: String,
    pub available: bool,
    pub reason: String,
}

static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .expect("Failed to build HTTP client for provider probing")
});

/// Probe external embedding/LLM providers
pub async fn probe_providers() -> Vec<ProviderStatus> {
    let mut providers = Vec::new();

    // 1. Check GLLM local (always available if compiled with feature)
    #[cfg(feature = "local-gllm")]
    {
        providers.push(ProviderStatus {
            name: "gllm-local".into(),
            available: true,
            reason: "Compiled with local-gllm feature".into(),
        });
    }
    #[cfg(not(feature = "local-gllm"))]
    {
        providers.push(ProviderStatus {
            name: "gllm-local".into(),
            available: false,
            reason: "Not compiled with local-gllm feature".into(),
        });
    }

    // 2. Check Ollama (local)
    let ollama = HTTP_CLIENT
        .get("http://localhost:11434/api/tags")
        .send()
        .await;
    providers.push(match ollama {
        Ok(r) if r.status().is_success() => ProviderStatus {
            name: "ollama".into(),
            available: true,
            reason: "http://localhost:11434 reachable".into(),
        },
        Ok(_) => ProviderStatus {
            name: "ollama".into(),
            available: false,
            reason: "Ollama API returned non-success".into(),
        },
        Err(e) => ProviderStatus {
            name: "ollama".into(),
            available: false,
            reason: format!("Ollama unreachable: {e}"),
        },
    });

    // 3. Check OpenAI API key in env
    let openai_key = std::env::var("OPENAI_API_KEY").ok();
    providers.push(if openai_key.is_some() {
        ProviderStatus {
            name: "openai".into(),
            available: true,
            reason: "OPENAI_API_KEY environment variable set".into(),
        }
    } else {
        ProviderStatus {
            name: "openai".into(),
            available: false,
            reason: "No OPENAI_API_KEY in environment".into(),
        }
    });

    // 4. Check Google (Gemini) API key in env
    let google_key = std::env::var("GOOGLE_API_KEY").ok();
    providers.push(if google_key.is_some() {
        ProviderStatus {
            name: "google-gemini".into(),
            available: true,
            reason: "GOOGLE_API_KEY environment variable set".into(),
        }
    } else {
        ProviderStatus {
            name: "google-gemini".into(),
            available: false,
            reason: "No GOOGLE_API_KEY in environment".into(),
        }
    });

    // 5. Check local embedding server (e.g., text-embeddings-inference)
    let local_embed = HTTP_CLIENT.get("http://localhost:8080/health").send().await;
    providers.push(match local_embed {
        Ok(r) if r.status().is_success() => ProviderStatus {
            name: "local-embed-server".into(),
            available: true,
            reason: "http://localhost:8080/health OK".into(),
        },
        Ok(_) => ProviderStatus {
            name: "local-embed-server".into(),
            available: false,
            reason: "Non-success response".into(),
        },
        Err(e) => ProviderStatus {
            name: "local-embed-server".into(),
            available: false,
            reason: format!("Not reachable: {e}"),
        },
    });

    providers
}
