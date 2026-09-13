//! USB Removable Storage Device Detector
//!
//! Scans system mount points and block devices to discover removable media
//! (USB flash drives, external HDDs, SSDs, SD cards) available for
//! air-gap offline storage capsule transfers.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Information about a detected removable storage device
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UsbStorageDevice {
    /// Device node path (e.g. `/dev/sdb1`, `/dev/nvme1n1p1`)
    pub device_path: PathBuf,
    /// Directory where the device is mounted
    pub mount_point: PathBuf,
    /// Filesystem type (e.g. `vfat`, `ext4`, `exfat`, `ntfs`)
    pub fs_type: Option<String>,
    /// Volume label or partition name
    pub label: Option<String>,
    /// Total storage capacity in bytes
    pub total_bytes: Option<u64>,
    /// Free / available storage capacity in bytes
    pub available_bytes: Option<u64>,
    /// Whether the filesystem is mounted read-write
    pub is_writable: bool,
}

/// Discovers all currently mounted removable USB storage devices.
///
/// Inspects `/proc/mounts`, `/sys/block`, and standard Linux/macOS mount paths
/// (`/run/media/`, `/media/`, `/Volumes/`).
pub fn list_removable_devices() -> Vec<UsbStorageDevice> {
    #[cfg(target_os = "linux")]
    {
        list_linux_removable_devices()
    }

    #[cfg(target_os = "macos")]
    {
        list_macos_removable_devices()
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Vec::new()
    }
}

/// Linux implementation: reads `/proc/mounts` and filters by removable attribute in sysfs
#[cfg(target_os = "linux")]
fn list_linux_removable_devices() -> Vec<UsbStorageDevice> {
    let mut devices = Vec::new();

    let mounts_content = match fs::read_to_string("/proc/mounts") {
        Ok(c) => c,
        Err(_) => return devices,
    };

    for line in mounts_content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }

        let dev_spec = parts[0];
        let mount_point_str = parts[1];
        let fs_type = parts[2];
        let options = parts[3];

        // Only consider actual block device paths (/dev/sd*, /dev/mmcblk*, /dev/nvme*, etc.)
        if !dev_spec.starts_with("/dev/") {
            continue;
        }

        let mount_path = PathBuf::from(mount_point_str);

        // Skip root filesystem, boot, and internal system paths
        if mount_point_str == "/" || mount_point_str.starts_with("/boot") || mount_point_str.starts_with("/nix") {
            continue;
        }

        let is_removable = is_linux_block_dev_removable(dev_spec)
            || mount_point_str.starts_with("/run/media/")
            || mount_point_str.starts_with("/media/");

        if !is_removable {
            continue;
        }

        let is_writable = !options.split(',').any(|opt| opt == "ro");
        let (total_bytes, available_bytes) = probe_disk_space(&mount_path);
        let label = probe_mount_label(&mount_path, dev_spec);

        devices.push(UsbStorageDevice {
            device_path: PathBuf::from(dev_spec),
            mount_point: mount_path,
            fs_type: Some(fs_type.to_string()),
            label,
            total_bytes,
            available_bytes,
            is_writable,
        });
    }

    devices
}

/// Checks sysfs (`/sys/block/<dev>/removable`) to determine if a block device is removable
#[cfg(target_os = "linux")]
fn is_linux_block_dev_removable(dev_path: &str) -> bool {
    let dev_name = dev_path.trim_start_matches("/dev/");
    // Strip partition numbers (e.g. sdb1 -> sdb, nvme0n1p1 -> nvme0n1)
    let parent_dev = if let Some(idx) = dev_name.find(|c: char| c.is_ascii_digit()) {
        if dev_name.starts_with("nvme") || dev_name.starts_with("mmcblk") {
            // mmcblk0p1 -> mmcblk0
            if let Some(p_idx) = dev_name.rfind('p') {
                &dev_name[..p_idx]
            } else {
                dev_name
            }
        } else {
            &dev_name[..idx]
        }
    } else {
        dev_name
    };

    let sys_removable_path = format!("/sys/block/{}/removable", parent_dev);
    if let Ok(content) = fs::read_to_string(&sys_removable_path) {
        if content.trim() == "1" {
            return true;
        }
    }

    false
}

/// macOS implementation: scans `/Volumes`
#[cfg(target_os = "macos")]
fn list_macos_removable_devices() -> Vec<UsbStorageDevice> {
    let mut devices = Vec::new();
    let volumes_path = Path::new("/Volumes");

    if let Ok(entries) = fs::read_dir(volumes_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                // Ignore standard macOS root links or recovery volumes
                if name.is_empty() || name == "Macintosh HD" {
                    continue;
                }

                let (total_bytes, available_bytes) = probe_disk_space(&path);
                devices.push(UsbStorageDevice {
                    device_path: path.clone(),
                    mount_point: path.clone(),
                    fs_type: None,
                    label: Some(name.to_string()),
                    total_bytes,
                    available_bytes,
                    is_writable: true,
                });
            }
        }
    }

    devices
}

/// Probes total and available disk space in bytes using `sysinfo::Disks` (100% safe Rust)
pub fn probe_disk_space(path: &Path) -> (Option<u64>, Option<u64>) {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    // First attempt: exact or longest prefix match
    let mut best_match: Option<(&sysinfo::Disk, usize)> = None;

    for disk in disks.iter() {
        let mnt = disk.mount_point();
        if path.starts_with(mnt) {
            let mnt_len = mnt.as_os_str().len();
            if best_match.as_ref().map_or(true, |(_, len)| mnt_len > *len) {
                best_match = Some((disk, mnt_len));
            }
        }
    }

    if let Some((disk, _)) = best_match {
        return (Some(disk.total_space()), Some(disk.available_space()));
    }

    // Fallback: if path is "/" or query cannot find exact mount point, return first available disk
    if let Some(first_disk) = disks.iter().next() {
        return (Some(first_disk.total_space()), Some(first_disk.available_space()));
    }

    (None, None)
}

/// Derives volume label from mount point folder name or device node
fn probe_mount_label(mount_point: &Path, _dev_spec: &str) -> Option<String> {
    mount_point
        .file_name()
        .and_then(|f| f.to_str())
        .map(|s| s.to_string())
}

/// Formats byte count into human-readable representation (e.g. `14.8 GB`)
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(2048), "2.00 KB");
        assert_eq!(format_bytes(1048576 * 5), "5.00 MB");
        assert_eq!(format_bytes(1073741824 * 32), "32.00 GB");
    }

    #[test]
    fn test_probe_disk_space_root() {
        let (total, avail) = probe_disk_space(Path::new("/"));
        assert!(total.is_some());
        assert!(avail.is_some());
        assert!(total.unwrap() > 0);
    }
}