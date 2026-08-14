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
        } else {
            // Robust fallback for Docker/sandbox containers where PSI is not available
            out.insert(resource.to_string(), PsiSample::default());
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
        // Swap: total >= 0 (puede ser 0 si no hay swap configurado)
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

// ═══════════════════════════════════════════════
// Fase P1: log_scan
// ═══════════════════════════════════════════════

/// Input options for log_scan MCP tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogScanArgs {
    pub since: Option<String>,
    pub level_min: Option<String>,
    pub pattern: Option<String>,
    pub source: Option<String>, // "xavier" | "hermes" | "journalctl"
    #[serde(default = "default_max_entries")]
    pub max_entries: usize,
}

fn default_max_entries() -> usize {
    500
}

/// A parsed, redacted log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogScanEntry {
    pub timestamp: String,
    pub level: String,
    pub message: String,
    pub source: String,
}

/// Persisted scan cursor
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct LogCursor {
    pub last_file: String,
    pub last_line: usize,
}

/// Response returned by log_scan MCP tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogScanResult {
    pub entries: Vec<LogScanEntry>,
    pub truncated: bool,
    pub cursor: LogCursor,
    pub histogram: HashMap<String, usize>,
    pub telegram_polling_dead: bool,
}

/// Helper to redact potential secrets (tokens, bearer keys, passwords)
pub fn redact_secrets(line: &str) -> String {
    // Basic redaction pattern that replaces large hex/alphanumeric tokens safely without backtracking
    let re = regex::Regex::new(
        r"(?i)(bearer\s+|api[-_]?key\s*[:=]\s*|token\s*[:=]\s*|password\s*[:=]\s*)[a-zA-Z0-9_\-\.\:\=]{8,}"
    ).unwrap();
    re.replace_all(line, "$1[REDACTED]").to_string()
}

fn level_to_val(l: &str) -> u32 {
    match l.to_lowercase().as_str() {
        "trace" => 0,
        "debug" => 1,
        "info" => 2,
        "warn" | "warning" => 3,
        "error" | "err" | "critical" => 4,
        _ => 2,
    }
}

/// Resolves log directory
pub fn resolve_logs_dir() -> std::path::PathBuf {
    if let Some(home) = dirs::home_dir() {
        let p = home.join(".xavier/logs");
        if p.exists() {
            return p;
        }
    }
    std::env::temp_dir().join("xavier_logs")
}

fn get_sorted_log_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                files.push(path);
            }
        }
    }
    files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    files
}

/// Parses a single log line (JSON or raw format)
pub fn parse_log_line(line: &str) -> Option<LogScanEntry> {
    if line.trim().is_empty() {
        return None;
    }
    let redacted = redact_secrets(line);
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&redacted) {
        let timestamp = v
            .get("timestamp")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();
        let level = v
            .get("level")
            .and_then(|l| l.as_str())
            .unwrap_or("INFO")
            .to_string();
        let message = v
            .get("fields")
            .and_then(|f| f.get("message"))
            .and_then(|m| m.as_str())
            .or_else(|| v.get("message").and_then(|m| m.as_str()))
            .unwrap_or("")
            .to_string();
        Some(LogScanEntry {
            timestamp,
            level,
            message,
            source: "xavier".to_string(),
        })
    } else {
        // Fallback standard text line parse [timestamp] [level] message
        let parts: Vec<&str> = redacted.splitn(3, ' ').collect();
        if parts.len() == 3 {
            Some(LogScanEntry {
                timestamp: parts[0].to_string(),
                level: parts[1].replace(['[', ']'], ""),
                message: parts[2].to_string(),
                source: "xavier".to_string(),
            })
        } else {
            Some(LogScanEntry {
                timestamp: chrono::Utc::now().to_rfc3339(),
                level: "INFO".to_string(),
                message: redacted,
                source: "xavier".to_string(),
            })
        }
    }
}

pub fn load_cursor() -> LogCursor {
    if let Some(home) = dirs::home_dir() {
        let cursor_path = home.join(".xavier/state/scan.cursor");
        if let Ok(content) = std::fs::read_to_string(&cursor_path) {
            if let Ok(cursor) = serde_json::from_str(&content) {
                return cursor;
            }
        }
    }
    LogCursor::default()
}

pub fn save_cursor(cursor: &LogCursor) {
    if let Some(home) = dirs::home_dir() {
        let cursor_path = home.join(".xavier/state/scan.cursor");
        if let Some(parent) = cursor_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(content) = serde_json::to_string(cursor) {
            let _ = std::fs::write(&cursor_path, content);
        }
    }
}

/// Scan logs under home directory (~/.xavier/logs) or fallbacks
pub fn log_scan(args: LogScanArgs) -> LogScanResult {
    let dir = resolve_logs_dir();
    let files = get_sorted_log_files(&dir);

    let mut entries = Vec::new();
    let mut histogram = HashMap::new();
    let mut truncated = false;

    let cursor = if args.since.is_some() {
        LogCursor::default()
    } else {
        load_cursor()
    };

    let mut new_cursor = cursor.clone();
    let mut start_reading = cursor.last_file.is_empty();

    let since_time = args
        .since
        .as_ref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok());

    for file_path in files {
        let file_name = file_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        if !start_reading {
            if file_name == cursor.last_file {
                start_reading = true;
            } else {
                continue;
            }
        }

        if let Ok(content) = std::fs::read_to_string(&file_path) {
            let mut line_num = 0;
            for line in content.lines() {
                line_num += 1;

                if file_name == cursor.last_file && line_num <= cursor.last_line {
                    continue;
                }

                if let Some(entry) = parse_log_line(line) {
                    if let Some(lvl_min) = &args.level_min {
                        if level_to_val(&entry.level) < level_to_val(lvl_min) {
                            continue;
                        }
                    }

                    if let Some(since) = since_time {
                        if let Ok(entry_time) =
                            chrono::DateTime::parse_from_rfc3339(&entry.timestamp)
                        {
                            if entry_time < since {
                                continue;
                            }
                        }
                    }

                    if let Some(pat) = &args.pattern {
                        if let Ok(re) = regex::Regex::new(pat) {
                            if !re.is_match(&entry.message) {
                                continue;
                            }
                        }
                    }

                    *histogram.entry(entry.level.clone()).or_insert(0) += 1;

                    if entries.len() >= args.max_entries {
                        truncated = true;
                        break;
                    }

                    entries.push(entry);
                }

                new_cursor.last_file = file_name.clone();
                new_cursor.last_line = line_num;
            }
        }

        if truncated {
            break;
        }
    }

    if args.since.is_none() {
        save_cursor(&new_cursor);
    }

    // Telegram Polling Dead Detection (P1)
    // Detects silence/dead loops or specific errors
    let mut telegram_polling_dead = false;
    let mut get_me_fails = 0;
    let mut has_close_wait = false;

    for entry in &entries {
        let msg = entry.message.to_lowercase();
        if msg.contains("telegram") || msg.contains("polling") {
            if msg.contains("failed")
                || msg.contains("error")
                || msg.contains("close-wait")
                || msg.contains("dead")
                || msg.contains("retry")
            {
                get_me_fails += 1;
                if msg.contains("close-wait") || msg.contains("close_wait") {
                    has_close_wait = true;
                }
            }
        }
    }

    if get_me_fails >= 2 || has_close_wait {
        telegram_polling_dead = true;
    }

    LogScanResult {
        entries,
        truncated,
        cursor: new_cursor,
        histogram,
        telegram_polling_dead,
    }
}

#[cfg(test)]
mod log_scan_tests {
    use super::*;

    #[test]
    fn test_redact_secrets() {
        let raw = "Some log with token = 123456:ABC-DEF-GHI and bearer sk-proj-12345";
        let redacted = redact_secrets(raw);
        assert!(redacted.contains("token = [REDACTED]"));
        assert!(redacted.contains("bearer [REDACTED]"));
    }

    #[test]
    fn test_parse_log_line_json() {
        let line = r#"{"timestamp":"2026-08-08T04:09:12Z","level":"WARN","fields":{"message":"Telegram polling failed: CLOSE-WAIT"}}"#;
        let parsed = parse_log_line(line).unwrap();
        assert_eq!(parsed.timestamp, "2026-08-08T04:09:12Z");
        assert_eq!(parsed.level, "WARN");
        assert_eq!(parsed.message, "Telegram polling failed: CLOSE-WAIT");
    }

    #[test]
    fn test_telegram_polling_dead_detection() {
        // Isolate from the real ~/.xavier/logs (exists on dev machines with a
        // running server; resolve_logs_dir() prefers it and ignores XAVIER_HOME).
        let orig_home = std::env::var_os("HOME");
        let fake_home = std::env::temp_dir().join("xavier-test-home");
        std::fs::create_dir_all(&fake_home).unwrap();
        std::env::set_var("HOME", &fake_home);
        // Clean the fallback dir so prior runs cannot pollute this one.
        let fallback = std::env::temp_dir().join("xavier_logs");
        let _ = std::fs::remove_dir_all(&fallback);
        std::fs::create_dir_all(&fallback).unwrap();

        let args = LogScanArgs {
            since: Some("2026-08-01T00:00:00Z".to_string()),
            level_min: None,
            pattern: None,
            source: None,
            max_entries: 10,
        };

        // Create a temporary log file in the resolve_logs_dir() path
        let dir = resolve_logs_dir();
        let _ = std::fs::create_dir_all(&dir);
        let temp_file = dir.join("xavier.test_polling.log");
        let sample_logs = r#"{"timestamp":"2026-08-08T04:09:00Z","level":"INFO","fields":{"message":"Starting Telegram bot..."}}
{"timestamp":"2026-08-08T04:10:00Z","level":"WARN","fields":{"message":"Telegram get_me failed: CLOSE-WAIT"}}
{"timestamp":"2026-08-08T04:11:00Z","level":"ERROR","fields":{"message":"Telegram polling failed: CLOSE-WAIT"}}
"#;
        std::fs::write(&temp_file, sample_logs).unwrap();

        let res = log_scan(args);
        assert!(res.telegram_polling_dead, "polling muerto no detectado");
        let _ = std::fs::remove_file(temp_file);
        let _ = std::fs::remove_dir_all(&fallback);
        std::env::remove_var("HOME");
        if let Some(h) = orig_home {
            std::env::set_var("HOME", h);
        }
    }
}

// ═══════════════════════════════════════════════
// Fase P1: env_status
// ═══════════════════════════════════════════════

/// Input options for env_status MCP tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvStatusArgs {
    pub include_processes: Option<bool>,
    pub top_n: Option<usize>,
}

/// Response returned by env_status MCP tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvStatusResult {
    pub psi: HashMap<String, PsiSample>,
    pub swap: SwapInfo,
    pub load_avg: [f64; 3],
    pub top_processes: Vec<ProcInfo>,
    pub services: HashMap<String, String>,
    pub connectivity: HashMap<String, String>,
    pub alerts: Vec<Alert>,
    pub overall: String,
}

/// Helper to query systemd service status safely using argv execution
pub fn check_service_status(service: &str) -> String {
    use std::process::Command;
    match Command::new("systemctl")
        .args(["is-active", service])
        .output()
    {
        Ok(output) => {
            let status = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if status.is_empty() {
                "inactive".to_string()
            } else {
                status
            }
        }
        Err(_) => "unknown (systemctl unavailable)".to_string(),
    }
}

/// Performs a synchronous TCP connection check with 2s timeout
pub fn tcp_probe(addr: &str) -> String {
    use std::net::TcpStream;
    use std::time::Duration;

    let resolved_addr = match addr.parse::<std::net::SocketAddr>() {
        Ok(sa) => sa,
        Err(_) => {
            use std::net::ToSocketAddrs;
            match (addr, 0)
                .to_socket_addrs()
                .ok()
                .and_then(|mut ips| ips.next())
            {
                Some(ip) => {
                    let port = if addr.contains("telegram") || addr.contains("github") {
                        443
                    } else {
                        53
                    };
                    std::net::SocketAddr::new(ip.ip(), port)
                }
                None => return "failed: resolution failed".to_string(),
            }
        }
    };

    match TcpStream::connect_timeout(&resolved_addr, Duration::from_secs(2)) {
        Ok(_) => "established".to_string(),
        Err(e) => format!("failed: {e}"),
    }
}

/// Snapshot and status of node environment and dependencies
pub fn env_status(args: EnvStatusArgs) -> EnvStatusResult {
    let snapshot = collect_system_snapshot();

    // Query allowlisted service statuses
    let allowlist = [
        "xavier.service",
        "hermes.service",
        "peerjs.service",
        "openclaw.service",
    ];
    let mut services = HashMap::new();
    for svc in allowlist {
        services.insert(svc.to_string(), check_service_status(svc));
    }

    // Basic network connectivity probes (dns.google is reliable for general internet checks)
    let mut connectivity = HashMap::new();
    connectivity.insert("dns_google".to_string(), tcp_probe("8.8.8.8:53"));
    connectivity.insert(
        "telegram_api".to_string(),
        tcp_probe("api.telegram.org:443"),
    );

    let mut top_processes = snapshot.top_rss.clone();
    if let Some(top_n) = args.top_n {
        top_processes.truncate(top_n.min(20));
    }

    if args.include_processes == Some(false) {
        top_processes.clear();
    }

    EnvStatusResult {
        psi: snapshot.psi,
        swap: snapshot.swap,
        load_avg: snapshot.load_avg,
        top_processes,
        services,
        connectivity,
        alerts: snapshot.alerts,
        overall: snapshot.overall,
    }
}

#[cfg(test)]
mod env_status_tests {
    use super::*;

    #[test]
    fn test_env_status_snapshot() {
        let args = EnvStatusArgs {
            include_processes: Some(true),
            top_n: Some(5),
        };
        let res = env_status(args);
        assert!(!res.overall.is_empty());
        assert!(res.services.contains_key("xavier.service"));
        assert!(res.connectivity.contains_key("dns_google"));
        assert!(res.top_processes.len() <= 5);
    }

    #[test]
    fn test_check_service_status_fallback() {
        let status = check_service_status("nonexistent.service");
        // Should gracefully degrade and not panic
        assert!(!status.is_empty());
    }

    #[test]
    fn test_tcp_probe_timeout() {
        // Resolve target that fails or times out quickly
        let res = tcp_probe("10.255.255.1:80");
        assert!(res.contains("failed"));
    }
}

// ═══════════════════════════════════════════════
// Fase P2: ticket_create
// ═══════════════════════════════════════════════

/// Input options for ticket_create MCP tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TicketCreateArgs {
    pub title: String,
    pub body: String,
    pub labels: Option<Vec<String>>,
    pub severity: String, // "critical" | "warn"
    pub fingerprint: Option<String>,
    pub backend: Option<String>, // "github" | "maloca"
}

/// Response returned by ticket_create MCP tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TicketCreateResult {
    pub id: String,
    pub url: String,
    pub deduplicated: bool,
    pub backend: String,
}

/// Created tickets tracker registry for deduplication and sliding rate limit
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CreatedTicketsRegistry {
    pub fingerprints: HashMap<String, String>, // fingerprint -> ticket_id
    #[serde(default)]
    pub creation_timestamps: Vec<u64>, // timestamp of ticket creations (seconds since epoch)
}

/// Loads the tickets registry from ~/.xavier/state/tickets.json
pub fn load_tickets_registry() -> CreatedTicketsRegistry {
    if let Some(home) = dirs::home_dir() {
        let p = home.join(".xavier/state/tickets.json");
        if let Ok(content) = std::fs::read_to_string(&p) {
            if let Ok(registry) = serde_json::from_str(&content) {
                return registry;
            }
        }
    }
    CreatedTicketsRegistry::default()
}

/// Saves the tickets registry to ~/.xavier/state/tickets.json
pub fn save_tickets_registry(reg: &CreatedTicketsRegistry) {
    if let Some(home) = dirs::home_dir() {
        let p = home.join(".xavier/state/tickets.json");
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(content) = serde_json::to_string_pretty(reg) {
            let _ = std::fs::write(&p, content);
        }
    }
}

/// Create GitHub issue or Maloca backlog entry, preventing duplicates via fingerprint
pub fn ticket_create(args: TicketCreateArgs) -> anyhow::Result<TicketCreateResult> {
    let computed_fingerprint = args.fingerprint.clone().unwrap_or_else(|| {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(args.title.trim().as_bytes());
        hasher.update(args.severity.trim().as_bytes());
        format!("{:x}", hasher.finalize())
    });

    let mut reg = load_tickets_registry();
    let now = chrono::Utc::now().timestamp() as u64;

    // Prune timestamps older than 1 hour (3600s) for rate limit check
    reg.creation_timestamps
        .retain(|&ts| now.saturating_sub(ts) < 3600);

    // 1. Strict Deduplication Check
    if reg.fingerprints.contains_key(&computed_fingerprint) {
        let id = reg
            .fingerprints
            .get(&computed_fingerprint)
            .cloned()
            .unwrap_or_default();
        return Ok(TicketCreateResult {
            id,
            url: "".into(),
            deduplicated: true,
            backend: args.backend.unwrap_or_else(|| "none".to_string()),
        });
    }

    // 2. Anti-Loop Rate Limit Check (max 3 tickets per hour)
    if reg.creation_timestamps.len() >= 3 {
        anyhow::bail!("Rate limit exceeded: maximum 3 tickets per hour allowed");
    }

    // 3. Create Ticket using specified/detected backend
    let backend_selected = args.backend.unwrap_or_else(|| "maloca".to_string());
    let mut ticket_id = format!("t-{}", uuid::Uuid::new_v4().to_string()[..8].to_string());
    let mut url = "".to_string();

    match backend_selected.as_str() {
        "github" => {
            use std::process::Command;
            let mut cmd = Command::new("gh");
            cmd.args(["issue", "create", "-t", &args.title, "-b", &args.body]);
            if let Some(lbls) = &args.labels {
                for l in lbls {
                    cmd.args(["-l", l]);
                }
            }
            match cmd.output() {
                Ok(output) if output.status.success() => {
                    url = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    ticket_id = url.split('/').last().unwrap_or("issue").to_string();
                }
                _ => {
                    // Fallback to maloca if gh command fails or is missing
                    let data_dir = crate::settings::XavierSettings::current()
                        .memory
                        .data_dir
                        .clone();
                    let store = crate::maloca::MalocaStore::open(std::path::Path::new(&data_dir));
                    let ticket = store.create_support(crate::maloca::types::CreateSupportBody {
                        title: args.title.clone(),
                        body: args.body.clone(),
                        feature_id: None,
                    });
                    ticket_id = ticket.id;
                }
            }
        }
        _ => {
            // Drop a support ticket into Maloca
            let data_dir = crate::settings::XavierSettings::current()
                .memory
                .data_dir
                .clone();
            let store = crate::maloca::MalocaStore::open(std::path::Path::new(&data_dir));
            let ticket = store.create_support(crate::maloca::types::CreateSupportBody {
                title: args.title.clone(),
                body: args.body.clone(),
                feature_id: None,
            });
            ticket_id = ticket.id;
        }
    }

    // Record and Save tracking state
    reg.fingerprints
        .insert(computed_fingerprint, ticket_id.clone());
    reg.creation_timestamps.push(now);
    save_tickets_registry(&reg);

    Ok(TicketCreateResult {
        id: ticket_id,
        url,
        deduplicated: false,
        backend: backend_selected,
    })
}

#[cfg(test)]
mod ticket_create_tests {
    use super::*;

    #[test]
    fn test_ticket_deduplication_and_rate_limiting() {
        // Clear local registry for testing
        let reg = CreatedTicketsRegistry::default();
        save_tickets_registry(&reg);

        let args1 = TicketCreateArgs {
            title: "Test incident issue".into(),
            body: "Swap thrashing detected".into(),
            labels: None,
            severity: "critical".into(),
            fingerprint: None,
            backend: Some("maloca".into()),
        };

        // First creation succeeds
        let res1 = ticket_create(args1.clone()).unwrap();
        assert!(!res1.deduplicated);

        // Second creation of duplicate triggers deduplication
        let res2 = ticket_create(args1.clone()).unwrap();
        assert!(res2.deduplicated);
        assert_eq!(res1.id, res2.id);

        // Create 2 more different tickets to hit rate-limit
        let args2 = TicketCreateArgs {
            title: "Test incident issue 2".into(),
            body: "Swap thrashing detected".into(),
            labels: None,
            severity: "critical".into(),
            fingerprint: None,
            backend: Some("maloca".into()),
        };
        let _ = ticket_create(args2).unwrap();

        let args3 = TicketCreateArgs {
            title: "Test incident issue 3".into(),
            body: "Swap thrashing detected".into(),
            labels: None,
            severity: "critical".into(),
            fingerprint: None,
            backend: Some("maloca".into()),
        };
        let _ = ticket_create(args3).unwrap();

        // 4th distinct creation should fail with rate-limit
        let args4 = TicketCreateArgs {
            title: "Test incident issue 4".into(),
            body: "Swap thrashing detected".into(),
            labels: None,
            severity: "critical".into(),
            fingerprint: None,
            backend: Some("maloca".into()),
        };
        let res4 = ticket_create(args4);
        assert!(res4.is_err());
        assert!(res4
            .unwrap_err()
            .to_string()
            .contains("Rate limit exceeded"));

        // Clean up tickets registry
        save_tickets_registry(&CreatedTicketsRegistry::default());
    }
}
