use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuInfo {
    pub vendor: GpuVendor,
    pub vram_bytes: u64,
}

/// Helper function to check if the VRAM is around 8GB (between 7GB and 9GB approx)
pub fn is_8gb_vram_class(vram: u64) -> bool {
    // 7 GB to 9 GB approx.
    // 7 GB is 7,000,000,000 bytes. 9 GB is 9,000,000,000 bytes.
    // 7 GiB is 7,516,192,768 bytes. 9 GiB is 9,663,676,416 bytes.
    // Let's use 6.8 GB to 9.8 GB in bytes as the range.
    let lower_limit = 6_800_000_000;
    let upper_limit = 9_800_000_000;
    vram >= lower_limit && vram <= upper_limit
}

/// Detects the GPU vendor and VRAM.
pub async fn detect_gpu() -> GpuInfo {
    // 1. Try nvidia-smi first (works on Windows and Linux)
    if let Some(vram) = detect_nvidia_via_smi() {
        return GpuInfo {
            vendor: GpuVendor::Nvidia,
            vram_bytes: vram,
        };
    }

    // 2. Try Windows-specific lightweight command wmic
    #[cfg(target_os = "windows")]
    {
        if let Some((vendor, vram)) = detect_windows_wmic() {
            return GpuInfo {
                vendor,
                vram_bytes: vram,
            };
        }
    }

    // 3. Try Linux AMD GPU detection (sysfs)
    #[cfg(target_os = "linux")]
    {
        if let Some(vram) = detect_linux_amd_sysfs() {
            return GpuInfo {
                vendor: GpuVendor::Amd,
                vram_bytes: vram,
            };
        }
    }

    // 4. Try generic AMD GPU detection (rocm-smi)
    if let Some(vram) = detect_amd_via_rocm() {
        return GpuInfo {
            vendor: GpuVendor::Amd,
            vram_bytes: vram,
        };
    }

    GpuInfo {
        vendor: GpuVendor::Unknown,
        vram_bytes: 0,
    }
}

fn detect_nvidia_via_smi() -> Option<u64> {
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=memory.total",
            "--format=csv,noheader",
        ])
        .output()
        .ok()?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_nvidia_smi_output(&stdout)
    } else {
        None
    }
}

fn parse_nvidia_smi_output(stdout: &str) -> Option<u64> {
    for line in stdout.lines() {
        let line_clean = line
            .replace("MiB", "")
            .replace("MB", "")
            .replace("mib", "")
            .replace("mb", "");
        if let Ok(mib) = line_clean.trim().parse::<u64>() {
            return Some(mib * 1024 * 1024);
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn detect_windows_wmic() -> Option<(GpuVendor, u64)> {
    let output = Command::new("wmic")
        .args(["path", "win32_VideoController", "get", "name,AdapterRAM"])
        .output()
        .ok()?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_wmic_output(&stdout)
    } else {
        None
    }
}

#[cfg(any(target_os = "windows", test))]
fn parse_wmic_output(stdout: &str) -> Option<(GpuVendor, u64)> {
    let mut best_vendor = GpuVendor::Unknown;
    let mut best_vram = 0;

    for line in stdout.lines() {
        let line_lower = line.to_lowercase();
        if line_lower.contains("adapterram") || line_lower.contains("name") {
            continue; // Skip header
        }

        let mut found_vram = None;
        for token in line.split_whitespace() {
            if let Ok(val) = token.parse::<u64>() {
                // Ignore small values (e.g. less than 10MB)
                if val > 10_000_000 {
                    found_vram = Some(val);
                    break;
                }
            }
        }

        let vendor = if line_lower.contains("nvidia")
            || line_lower.contains("geforce")
            || line_lower.contains("rtx")
            || line_lower.contains("gtx")
        {
            GpuVendor::Nvidia
        } else if line_lower.contains("amd") || line_lower.contains("radeon") {
            GpuVendor::Amd
        } else {
            GpuVendor::Unknown
        };

        if vendor != GpuVendor::Unknown {
            if let Some(vram) = found_vram {
                return Some((vendor, vram));
            } else {
                best_vendor = vendor;
            }
        } else if let Some(vram) = found_vram {
            if best_vram == 0 {
                best_vram = vram;
            }
        }
    }

    if best_vendor != GpuVendor::Unknown {
        Some((best_vendor, best_vram))
    } else if best_vram > 0 {
        Some((GpuVendor::Unknown, best_vram))
    } else {
        None
    }
}

#[cfg(target_os = "linux")]
fn detect_linux_amd_sysfs() -> Option<u64> {
    // Read from /sys/class/drm/card*/device/mem_info_vram_total
    // If there are multiple cards, find the first one that has VRAM info
    let paths = std::fs::read_dir("/sys/class/drm").ok()?;
    for entry in paths.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with("card") && !name_str.contains('-') {
            let vram_path = entry.path().join("device").join("mem_info_vram_total");
            if let Ok(vram_str) = std::fs::read_to_string(vram_path) {
                if let Ok(bytes) = vram_str.trim().parse::<u64>() {
                    if bytes > 10_000_000 {
                        return Some(bytes);
                    }
                }
            }
        }
    }
    None
}

fn detect_amd_via_rocm() -> Option<u64> {
    // Try rocm-smi if available
    let output = Command::new("rocm-smi")
        .args(["--showmeminfo", "vram"])
        .output()
        .ok()?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        // Typical line: "GPU[0] : VRAM Total Memory (B): 8573157376" or similar
        for line in stdout.lines() {
            if line.contains("VRAM Total") || line.contains("Total Memory") {
                for token in line.split_whitespace() {
                    if let Ok(bytes) = token.parse::<u64>() {
                        if bytes > 10_000_000 {
                            return Some(bytes);
                        }
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_8gb_vram_class() {
        // Values that should be true
        assert!(is_8gb_vram_class(8 * 1024 * 1024 * 1024)); // 8 GiB
        assert!(is_8gb_vram_class(8_000_000_000)); // 8 GB decimal
        assert!(is_8gb_vram_class(7 * 1024 * 1024 * 1024)); // 7 GiB
        assert!(is_8gb_vram_class(9 * 1024 * 1024 * 1024)); // 9 GiB
        assert!(is_8gb_vram_class(7_000_000_000)); // 7 GB decimal
        assert!(is_8gb_vram_class(9_000_000_000)); // 9 GB decimal

        // Values that should be false
        assert!(!is_8gb_vram_class(4 * 1024 * 1024 * 1024)); // 4 GiB
        assert!(!is_8gb_vram_class(16 * 1024 * 1024 * 1024)); // 16 GiB
        assert!(!is_8gb_vram_class(0));
    }

    #[test]
    fn test_parse_nvidia_smi_output() {
        let sample = "8192 MiB\n";
        assert_eq!(parse_nvidia_smi_output(sample), Some(8192 * 1024 * 1024));

        let sample_no_unit = "16128\n";
        assert_eq!(parse_nvidia_smi_output(sample_no_unit), Some(16128 * 1024 * 1024));
    }

    #[test]
    fn test_parse_wmic_output() {
        let sample = "AdapterRAM  Name\r\n4293918720  NVIDIA GeForce RTX 3060 Laptop GPU\r\n";
        let res = parse_wmic_output(sample).unwrap();
        assert_eq!(res.0, GpuVendor::Nvidia);
        assert_eq!(res.1, 4293918720);

        let sample_amd = "AdapterRAM  Name\r\n8589934592  AMD Radeon RX 6600\r\n";
        let res_amd = parse_wmic_output(sample_amd).unwrap();
        assert_eq!(res_amd.0, GpuVendor::Amd);
        assert_eq!(res_amd.1, 8589934592);
    }
}
