//! Hardware resource scanner & RAG budget allocator
//!
//! Provides deterministic system scanning, grading hosts into resource tiers
//! (Constrained, Standard, Powerhouse), and calculating optimal vector cache/ingestion budgets.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::process::Command;
use sysinfo::{Disks, System};

/// Constants for memory calculations
const GB_BYTES: u64 = 1024 * 1024 * 1024;
const MB_BYTES: u64 = 1024 * 1024;

/// Resource tier assigned to host based on available hardware capabilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HostTier {
    /// Constrained environment (< 8GB RAM). Suitable for lightweight local processing.
    Constrained,
    /// Standard environment (8-32GB RAM, or >32GB without GPU). Balanced performance.
    Standard,
    /// Powerhouse environment (> 32GB RAM + discrete/accelerated GPU). Maximum parallelism & cache.
    Powerhouse,
}

/// Resource allocation profile recommended for local SLM and RAG ingestion workloads.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceAllocation {
    /// Maximum memory limit in MB reserved for SQLite-Vec / vector cache.
    pub max_vector_cache_mb: usize,
    /// Ingestion batch size for embeddings/chunking.
    pub batch_size: usize,
    /// Number of parallel text chunking threads.
    pub parallel_chunkers: usize,
    /// Whether quantized vector embeddings / models are strongly recommended.
    pub quantization_recommended: bool,
}

/// Metrics describing system hardware capabilities and current resource availability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareMetrics {
    /// Number of physical or logical CPU cores.
    pub cpu_cores: usize,
    /// Total RAM in bytes.
    pub total_ram_bytes: u64,
    /// Currently available RAM in bytes.
    pub available_ram_bytes: u64,
    /// Whether a usable discrete or integrated GPU was detected.
    pub has_gpu: bool,
    /// Detected GPU VRAM in bytes, if available.
    pub gpu_vram_bytes: Option<u64>,
    /// Available disk space in bytes on primary partition.
    pub disk_available_bytes: u64,
}

impl HardwareMetrics {
    /// Detect current host hardware metrics using sysinfo and cross-platform GPU probes.
    /// Infallible graceful fallback guarantees zero panics on virtualized or restricted container environments.
    pub fn detect_current() -> Result<Self> {
        let mut sys = System::new();
        sys.refresh_memory();

        let cpu_cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);

        let total_ram_bytes = sys.total_memory();
        let available_ram_bytes = sys.available_memory();

        let disks = Disks::new_with_refreshed_list();
        let disk_available_bytes = disks
            .iter()
            .next()
            .map(|d| d.available_space())
            .unwrap_or(0);

        let (has_gpu, gpu_vram_bytes) = Self::detect_gpu_graceful();

        Ok(Self {
            cpu_cores,
            total_ram_bytes,
            available_ram_bytes,
            has_gpu,
            gpu_vram_bytes,
            disk_available_bytes,
        })
    }

    /// Evaluates hardware classification tier.
    ///
    /// Rules:
    /// - Constrained: < 8 GiB RAM
    /// - Powerhouse: >= 32 GiB RAM AND GPU detected
    /// - Standard: All other configurations (8..32 GiB, or >= 32 GiB without GPU)
    pub fn host_tier(&self) -> HostTier {
        let ram_gb = self.total_ram_bytes as f64 / GB_BYTES as f64;
        if ram_gb < 8.0 {
            HostTier::Constrained
        } else if ram_gb >= 32.0 && self.has_gpu {
            HostTier::Powerhouse
        } else {
            HostTier::Standard
        }
    }

    /// Computes optimal budget allocations for vector caching and RAG chunking workloads.
    pub fn recommended_allocation(&self) -> ResourceAllocation {
        let tier = self.host_tier();
        let avail_ram_mb = (self.available_ram_bytes / MB_BYTES) as usize;

        match tier {
            HostTier::Constrained => {
                let cache_mb = (avail_ram_mb / 8).clamp(64, 256);
                ResourceAllocation {
                    max_vector_cache_mb: cache_mb,
                    batch_size: 16,
                    parallel_chunkers: 1.max(self.cpu_cores / 2),
                    quantization_recommended: true,
                }
            }
            HostTier::Standard => {
                let cache_mb = (avail_ram_mb / 6).clamp(256, 2048);
                ResourceAllocation {
                    max_vector_cache_mb: cache_mb,
                    batch_size: 64,
                    parallel_chunkers: 2.max(self.cpu_cores / 2),
                    quantization_recommended: self.gpu_vram_bytes.is_none_or(|v| v < 6 * GB_BYTES),
                }
            }
            HostTier::Powerhouse => {
                let cache_mb = (avail_ram_mb / 4).clamp(1024, 8192);
                let parallel = 4.max(self.cpu_cores);
                ResourceAllocation {
                    max_vector_cache_mb: cache_mb,
                    batch_size: 128,
                    parallel_chunkers: parallel,
                    quantization_recommended: false,
                }
            }
        }
    }

    /// Helper to probe GPU presence and VRAM without throwing panics in unprivileged environments.
    fn detect_gpu_graceful() -> (bool, Option<u64>) {
        // 1. Try nvidia-smi
        if let Ok(output) = Command::new("nvidia-smi")
            .args(["--query-gpu=memory.total", "--format=csv,noheader,nounits"])
            .output()
        {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if let Some(first_line) = stdout.lines().next() {
                    if let Ok(mib) = first_line.trim().parse::<u64>() {
                        let bytes = mib * MB_BYTES;
                        return (true, Some(bytes));
                    }
                }
                return (true, None);
            }
        }

        // 2. Try rocm-smi
        if let Ok(output) = Command::new("rocm-smi")
            .args(["--showmeminfo", "vram"])
            .output()
        {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    if line.contains("VRAM Total") || line.contains("Total Memory") {
                        for token in line.split_whitespace() {
                            if let Ok(bytes) = token.parse::<u64>() {
                                if bytes > 10_000_000 {
                                    return (true, Some(bytes));
                                }
                            }
                        }
                    }
                }
                return (true, None);
            }
        }

        // 3. Linux sysfs AMD GPU fallback
        #[cfg(target_os = "linux")]
        {
            if let Ok(paths) = std::fs::read_dir("/sys/class/drm") {
                for entry in paths.flatten() {
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();
                    if name_str.starts_with("card") && !name_str.contains('-') {
                        let vram_path = entry.path().join("device").join("mem_info_vram_total");
                        if let Ok(vram_str) = std::fs::read_to_string(vram_path) {
                            if let Ok(bytes) = vram_str.trim().parse::<u64>() {
                                if bytes > 10_000_000 {
                                    return (true, Some(bytes));
                                }
                            }
                        }
                    }
                }
            }
        }

        // 4. macOS Metal check
        #[cfg(target_os = "macos")]
        {
            if let Ok(output) = Command::new("system_profiler")
                .args(["SPDisplaysDataType", "-json"])
                .output()
            {
                if output.status.success() {
                    return (true, None);
                }
            }
        }

        (false, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_host_tier_classification() {
        // Constrained (< 8GB RAM)
        let constrained = HardwareMetrics {
            cpu_cores: 2,
            total_ram_bytes: 4 * GB_BYTES,
            available_ram_bytes: 2 * GB_BYTES,
            has_gpu: false,
            gpu_vram_bytes: None,
            disk_available_bytes: 50 * GB_BYTES,
        };
        assert_eq!(constrained.host_tier(), HostTier::Constrained);

        // Standard (16GB RAM, no GPU)
        let standard_no_gpu = HardwareMetrics {
            cpu_cores: 8,
            total_ram_bytes: 16 * GB_BYTES,
            available_ram_bytes: 8 * GB_BYTES,
            has_gpu: false,
            gpu_vram_bytes: None,
            disk_available_bytes: 100 * GB_BYTES,
        };
        assert_eq!(standard_no_gpu.host_tier(), HostTier::Standard);

        // Standard (64GB RAM, no GPU)
        let powerhouse_ram_no_gpu = HardwareMetrics {
            cpu_cores: 16,
            total_ram_bytes: 64 * GB_BYTES,
            available_ram_bytes: 32 * GB_BYTES,
            has_gpu: false,
            gpu_vram_bytes: None,
            disk_available_bytes: 500 * GB_BYTES,
        };
        assert_eq!(powerhouse_ram_no_gpu.host_tier(), HostTier::Standard);

        // Powerhouse (64GB RAM + GPU)
        let powerhouse = HardwareMetrics {
            cpu_cores: 16,
            total_ram_bytes: 64 * GB_BYTES,
            available_ram_bytes: 32 * GB_BYTES,
            has_gpu: true,
            gpu_vram_bytes: Some(16 * GB_BYTES),
            disk_available_bytes: 500 * GB_BYTES,
        };
        assert_eq!(powerhouse.host_tier(), HostTier::Powerhouse);
    }

    #[test]
    fn test_synthetic_allocation_curves() {
        let constrained = HardwareMetrics {
            cpu_cores: 4,
            total_ram_bytes: 4 * GB_BYTES,
            available_ram_bytes: GB_BYTES, // 1024 MB avail -> 1024 / 8 = 128 MB cache
            has_gpu: false,
            gpu_vram_bytes: None,
            disk_available_bytes: 20 * GB_BYTES,
        };
        let alloc_c = constrained.recommended_allocation();
        assert_eq!(alloc_c.max_vector_cache_mb, 128);
        assert_eq!(alloc_c.batch_size, 16);
        assert_eq!(alloc_c.parallel_chunkers, 2);
        assert!(alloc_c.quantization_recommended);

        let powerhouse = HardwareMetrics {
            cpu_cores: 12,
            total_ram_bytes: 64 * GB_BYTES,
            available_ram_bytes: 16 * GB_BYTES, // 16384 MB avail -> 16384 / 4 = 4096 MB cache
            has_gpu: true,
            gpu_vram_bytes: Some(12 * GB_BYTES),
            disk_available_bytes: 500 * GB_BYTES,
        };
        let alloc_p = powerhouse.recommended_allocation();
        assert_eq!(alloc_p.max_vector_cache_mb, 4096);
        assert_eq!(alloc_p.batch_size, 128);
        assert_eq!(alloc_p.parallel_chunkers, 12);
        assert!(!alloc_p.quantization_recommended);
    }

    #[test]
    fn test_edge_case_memory_bounds() {
        // Zero or near zero RAM
        let zero_ram = HardwareMetrics {
            cpu_cores: 1,
            total_ram_bytes: 0,
            available_ram_bytes: 0,
            has_gpu: false,
            gpu_vram_bytes: None,
            disk_available_bytes: 0,
        };
        assert_eq!(zero_ram.host_tier(), HostTier::Constrained);
        let alloc_zero = zero_ram.recommended_allocation();
        assert_eq!(alloc_zero.max_vector_cache_mb, 64); // clamped min

        // Exactly 8GB RAM boundary threshold
        let exact_8gb = HardwareMetrics {
            cpu_cores: 4,
            total_ram_bytes: 8 * GB_BYTES,
            available_ram_bytes: 4 * GB_BYTES,
            has_gpu: false,
            gpu_vram_bytes: None,
            disk_available_bytes: 50 * GB_BYTES,
        };
        assert_eq!(exact_8gb.host_tier(), HostTier::Standard);

        // Exactly 32GB RAM threshold with GPU
        let exact_32gb = HardwareMetrics {
            cpu_cores: 8,
            total_ram_bytes: 32 * GB_BYTES,
            available_ram_bytes: 16 * GB_BYTES,
            has_gpu: true,
            gpu_vram_bytes: Some(8 * GB_BYTES),
            disk_available_bytes: 100 * GB_BYTES,
        };
        assert_eq!(exact_32gb.host_tier(), HostTier::Powerhouse);
    }

    #[test]
    fn test_detect_current_infallible() {
        let detected = HardwareMetrics::detect_current();
        assert!(detected.is_ok());
        let metrics = detected.unwrap();
        assert!(metrics.cpu_cores >= 1);
    }
}
