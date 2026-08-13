//! Self-managed runtime — Xavier como guardian del nodo (P0, 2026-08-08)
//!
//! Monitoreo read-only del HOST donde corre Xavier: PSI, swap, load average,
//! top procesos por RSS, conteo D-state y alertas derivadas de umbrales.
//! Sin dependencias externas (solo std + serde) — lectura directa de /proc.
//!
//! Diseño validado con Kimi k3 (docs/research/SELF-MANAGEMENT-RUNTIME.md §9-12).
//! Siguientes fases: log_scan (P1), env_status (P1), ticket_create (P2).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;

/// Una línea `some`/`full` de /proc/pressure/*.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PsiLine {
    pub avg10: f64,
    pub avg60: f64,
    pub avg300: f64,
}

/// Muestra PSI completa de un recurso (cpu/memory/io).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PsiSample {
    pub some: PsiLine,
    pub full: PsiLine,
}

/// Estado del swap del host.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SwapInfo {
    pub total_mb: u64,
    pub used_mb: u64,
    pub used_percent: f64,
    pub devices: Vec<String>,
}

/// Proceso del host (top por RSS).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcInfo {
    pub pid: i32,
    pub name: String,
    pub state: char,
    pub rss_mb: u64,
    pub vmswap_mb: u64,
}

/// Alerta derivada de los umbrales (tabla §5 del análisis).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub severity: String, // "critical" | "warn"
    pub metric: String,
    pub value: String,
    pub threshold: String,
}

/// Snapshot completo del host (P0).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SystemSnapshot {
    pub psi: HashMap<String, PsiSample>,
    pub swap: SwapInfo,
    pub load_avg: [f64; 3],
    pub top_rss: Vec<ProcInfo>,
    pub d_state_count: usize,
    pub alerts: Vec<Alert>,
    pub overall: String, // "healthy" | "degraded" | "critical"
}

fn parse_psi_file(path: &str) -> Option<PsiSample> {
    let content = fs::read_to_string(path).ok()?;
    let mut sample = PsiSample::default();
    for line in content.lines() {
        let mut parts = line.split_whitespace();
        let kind = parts.next()?;
        let mut avg10 = 0.0;
        let mut avg60 = 0.0;
        let mut avg300 = 0.0;
        for part in parts {
            if let Some((k, v)) = part.split_once('=') {
                let val: f64 = v.parse().unwrap_or(0.0);
                match k {
                    "avg10" => avg10 = val,
                    "avg60" => avg60 = val,
                    "avg300" => avg300 = val,
                    _ => {}
                }
            }
        }
        let line = PsiLine {
            avg10,
            avg60,
            avg300,
        };
        match kind {
            "some" => sample.some = line,
            "full" => sample.full = line,
            _ => {}
        }
    }
    Some(sample)
}

fn read_psi() -> HashMap<String, PsiSample> {
    let mut out = HashMap::new();
    for resource in ["cpu", "memory", "io"] {
        if let Some(sample) = parse_psi_file(&format!("/proc/pressure/{resource}")) {
            out.insert(resource.to_string(), sample);
        }
    }
    out
}

fn read_swap() -> SwapInfo {
    let mut info = SwapInfo::default();
    if let Ok(meminfo) = fs::read_to_string("/proc/meminfo") {
        let mut total_kb = 0u64;
        let mut free_kb = 0u64;
        for line in meminfo.lines() {
            if let Some(rest) = line.strip_prefix("SwapTotal:") {
                total_kb = rest
                    .trim()
                    .split_whitespace()
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
            } else if let Some(rest) = line.strip_prefix("SwapFree:") {
                free_kb = rest
                    .trim()
                    .split_whitespace()
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
            }
        }
        info.total_mb = total_kb / 1024;
        info.used_mb = total_kb.saturating_sub(free_kb) / 1024;
        info.used_percent = if total_kb > 0 {
            (total_kb - free_kb) as f64 / total_kb as f64 * 100.0
        } else {
            0.0
        };
    }
    if let Ok(swaps) = fs::read_to_string("/proc/swaps") {
        for line in swaps.lines().skip(1) {
            if let Some(dev) = line.split_whitespace().next() {
                info.devices.push(dev.to_string());
            }
        }
    }
    info
}

fn read_load_avg() -> [f64; 3] {
    if let Ok(content) = fs::read_to_string("/proc/loadavg") {
        let vals: Vec<f64> = content
            .split_whitespace()
            .take(3)
            .filter_map(|v| v.parse().ok())
            .collect();
        if vals.len() == 3 {
            return [vals[0], vals[1], vals[2]];
        }
    }
    [0.0, 0.0, 0.0]
}

fn read_processes() -> (Vec<ProcInfo>, usize) {
    let mut procs: Vec<ProcInfo> = Vec::new();
    let mut d_state = 0usize;
    let entries = match fs::read_dir("/proc") {
        Ok(e) => e,
        Err(_) => return (procs, d_state),
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy().to_string();
        let pid: i32 = match name.parse() {
            Ok(pid) => pid,
            Err(_) => continue,
        };
        let status_path = format!("/proc/{pid}/status");
        let Ok(status) = fs::read_to_string(&status_path) else {
            continue;
        };
        let mut pname = String::from("?");
        let mut rss_kb = 0u64;
        let mut vmswap_kb = 0u64;
        let mut state = '?';
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("Name:") {
                pname = rest.trim().to_string();
            } else if let Some(rest) = line.strip_prefix("State:") {
                state = rest.trim().chars().next().unwrap_or('?');
            } else if let Some(rest) = line.strip_prefix("VmRSS:") {
                rss_kb = rest
                    .trim()
                    .split_whitespace()
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
            } else if let Some(rest) = line.strip_prefix("VmSwap:") {
                vmswap_kb = rest
                    .trim()
                    .split_whitespace()
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
            }
        }
        if state == 'D' {
            d_state += 1;
        }
        procs.push(ProcInfo {
            pid,
            name: pname,
            state,
            rss_mb: rss_kb / 1024,
            vmswap_mb: vmswap_kb / 1024,
        });
    }
    procs.sort_by(|a, b| b.rss_mb.cmp(&a.rss_mb));
    procs.truncate(10);
    (procs, d_state)
}

/// Umbrales iniciales (análisis §5, validados contra el caso 2026-08-08).
fn evaluate_alerts(
    psi: &HashMap<String, PsiSample>,
    swap: &SwapInfo,
    top: &[ProcInfo],
) -> Vec<Alert> {
    let mut alerts = Vec::new();
    if let Some(io) = psi.get("io") {
        let full10 = io.full.avg10;
        if full10 > 50.0 {
            alerts.push(Alert {
                severity: "critical".into(),
                metric: "psi.io.full.avg10".into(),
                value: format!("{full10:.1}%"),
                threshold: ">50% critical (thrashing)".into(),
            });
        } else if full10 > 30.0 {
            alerts.push(Alert {
                severity: "warn".into(),
                metric: "psi.io.full.avg10".into(),
                value: format!("{full10:.1}%"),
                threshold: ">30% warn".into(),
            });
        }
    }
    if swap.used_percent > 80.0 {
        alerts.push(Alert {
            severity: "critical".into(),
            metric: "swap.used_percent".into(),
            value: format!("{:.1}%", swap.used_percent),
            threshold: ">80% critical".into(),
        });
    } else if swap.used_percent > 60.0 {
        alerts.push(Alert {
            severity: "warn".into(),
            metric: "swap.used_percent".into(),
            value: format!("{:.1}%", swap.used_percent),
            threshold: ">60% warn".into(),
        });
    }
    for p in top {
        if p.vmswap_mb > 4096 {
            alerts.push(Alert {
                severity: "warn".into(),
                metric: "process.vmswap".into(),
                value: format!("{} pid={} {}MB", p.name, p.pid, p.vmswap_mb),
                threshold: ">4GB VmSwap warn".into(),
            });
        }
    }
    alerts
}

/// Recopila el snapshot completo del host.
pub fn collect_system_snapshot() -> SystemSnapshot {
    let psi = read_psi();
    let swap = read_swap();
    let load_avg = read_load_avg();
    let (top_rss, d_state_count) = read_processes();
    let alerts = evaluate_alerts(&psi, &swap, &top_rss);
    let overall = if alerts.iter().any(|a| a.severity == "critical") {
        "critical".to_string()
    } else if !alerts.is_empty() {
        "degraded".to_string()
    } else {
        "healthy".to_string()
    };
    SystemSnapshot {
        psi,
        swap,
        load_avg,
        top_rss,
        d_state_count,
        alerts,
        overall,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sys_health_snapshot_has_expected_fields() {
        let s = collect_system_snapshot();
        // PSI: los tres recursos deben estar presentes
        assert!(s.psi.contains_key("cpu"), "psi cpu");
        assert!(s.psi.contains_key("memory"), "psi memory");
        assert!(s.psi.contains_key("io"), "psi io");
        // Rango válido de PSI
        for sample in s.psi.values() {
            assert!((0.0..=100.0).contains(&sample.some.avg10), "avg10 0-100");
            assert!(
                (0.0..=100.0).contains(&sample.full.avg10),
                "full avg10 0-100"
            );
        }
        // Swap: total > 0 (zram o disco)
        assert!(
            s.swap.total_mb > 0,
            "swap total > 0 (era {} MB)",
            s.swap.total_mb
        );
        assert!(s.swap.used_percent >= 0.0 && s.swap.used_percent <= 100.0);
        // Load average: 3 valores finitos
        assert_eq!(s.load_avg.len(), 3);
        // Top procesos: máx 10
        assert!(s.top_rss.len() <= 10);
        // overall siempre uno de los tres estados
        assert!(
            matches!(s.overall.as_str(), "healthy" | "degraded" | "critical"),
            "overall={}",
            s.overall
        );
    }

    #[test]
    fn sys_health_alerts_fire_on_high_swap() {
        // Umbrales: swap >80% critical, >60% warn
        let mut snap = SystemSnapshot::default();
        snap.swap.total_mb = 1000;
        snap.swap.used_mb = 850;
        snap.swap.used_percent = 85.0;
        let alerts = evaluate_alerts(&HashMap::new(), &snap.swap, &[]);
        assert!(
            alerts
                .iter()
                .any(|a| a.severity == "critical" && a.metric == "swap.used_percent"),
            "swap 85% debe alertar critical: {:?}",
            alerts
        );
    }

    #[test]
    fn sys_health_alerts_fire_on_high_psi_io() {
        let mut psi = HashMap::new();
        let mut sample = PsiSample::default();
        sample.full.avg10 = 60.0; // >50 critical
        psi.insert("io".to_string(), sample);
        let alerts = evaluate_alerts(&psi, &SwapInfo::default(), &[]);
        assert!(
            alerts
                .iter()
                .any(|a| a.severity == "critical" && a.metric == "psi.io.full.avg10"),
            "PSI io 60% debe alertar critical: {:?}",
            alerts
        );
    }

    #[test]
    fn sys_health_no_false_alerts_on_healthy_host() {
        let mut psi = HashMap::new();
        let mut sample = PsiSample::default();
        sample.full.avg10 = 5.0;
        psi.insert("io".to_string(), sample);
        let mut swap = SwapInfo::default();
        swap.total_mb = 1000;
        swap.used_mb = 100;
        swap.used_percent = 10.0;
        let alerts = evaluate_alerts(&psi, &swap, &[]);
        assert!(alerts.is_empty(), "host sano no debe alertar: {:?}", alerts);
    }
}
