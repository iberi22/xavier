//! SWAL Benchmark Suite for Xavier
//!
//! Dogfooding benchmarks that measure Xavier's real-world performance
//! as an agent memory system. Tests cover the four SWAL maturity pillars:
//! Memory, Search, Embedding, Context.
//!
//! Run: cargo test --release --test swal_benchmarks -- --nocapture

use std::process::Command;
use std::time::{Duration, Instant};

// ─── Helpers ───────────────────────────────────────────────────────────────

fn xavier_binary() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_xavier"));
    cmd.env("XAVIER_MCP_PORT", "0");
    cmd
}

/// Run CLI command, measure wall time
fn bench_cli(args: &[&str], label: &str) -> Duration {
    let start = Instant::now();
    let output = xavier_binary().args(args).output().unwrap_or_else(|_| {
        panic!("failed to run xavier {} {}", label, args.join(" "))
    });
    let elapsed = start.elapsed();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    println!(
        "  [{:40}] {:.2?} | exit={} | out={}B",
        label,
        elapsed,
        output.status.code().unwrap_or(-1),
        stdout.len()
    );

    if !output.status.success() && !stderr.contains("help") {
        eprintln!("  ⚠ STDERR: {}", stderr.trim());
    }

    elapsed
}

fn ensure_binary() {
    // Verify binary exists
    let status = xavier_binary()
        .arg("--help")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    assert!(
        status.is_ok() && status.unwrap().success(),
        "xavier binary must be compiled first (cargo build --release)"
    );
}

// ─── Help test (baseline sanity) ────────────────────────────────────────────

#[test]
fn swal_bench_help() {
    ensure_binary();
    let output = xavier_binary()
        .arg("--help")
        .output()
        .expect("failed to run xavier --help");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    println!("── SWAL: help output check ──");
    println!(
        "stdout: {}B, stderr: {}B",
        stdout.len(),
        output.stderr.len()
    );
}

// ─── BENCHMARKS ────────────────────────────────────────────────────────────

/// Benchmark #1: CLI startup latency (cold, no args)
#[test]
fn swal_bench_cli_startup() {
    ensure_binary();
    println!("\n── SWAL Benchmark 1: CLI Startup Latency ──");
    let mut times = Vec::new();
    for i in 0..5 {
        let t = bench_cli(&["--help"], &format!("run-{}", i));
        times.push(t);
    }
    let avg = times.iter().sum::<Duration>() / times.len() as u32;
    println!("──── avg startup: {:.2?}", avg);
    println!("──── result: {}ms avg (target: <500ms)", avg.as_millis());
    assert!(avg.as_millis() < 5000, "Startup too slow: {:.2?}", avg);
}

/// Benchmark #2: Memory add throughput (100 rapid inserts via CLI)
#[test]
fn swal_bench_memory_add_throughput() {
    ensure_binary();
    println!("\n── SWAL Benchmark 2: Memory Add Throughput (100x) ──");
    let mut total = Duration::ZERO;
    for i in 0..100 {
        let content = format!("SWAL benchmark memory entry #{}: AI agents need fast contextual recall without external dependencies. Xavier provides this through vector embeddings over a SQLite store.", i);
        let path = format!("swal-bench/entry-{}", i);
        let t = bench_cli(&["add", &content, &path], &format!("add-{}", i));
        total += t;
    }
    let avg = total / 100;
    println!("──── total: {:.2?}, avg/op: {:.2?}", total, avg);
    println!(
        "──── throughput: {:.1} ops/sec",
        100.0 / total.as_secs_f64()
    );
}

/// Benchmark #3: Memory add batch (one large entry)
#[test]
fn swal_bench_memory_add_large() {
    ensure_binary();
    println!("\n── SWAL Benchmark 3: Large Memory Add ──");
    let large_content = "SWAL benchmark LARGE entry. ".repeat(100);
    let t = bench_cli(
        &["add", &large_content, "swal-bench/large-entry"],
        "large-add",
    );
    println!("──── size: {} chars, time: {:.2?}", large_content.len(), t);
}

/// Benchmark #4: Search latency (after adding 100 entries)
#[test]
fn swal_bench_search() {
    ensure_binary();
    println!("\n── SWAL Benchmark 4: Search Latency ──");
    // First ensure some data exists — add test entries if needed
    for i in 0..20 {
        let content = format!(
            "SWAL searchable content item number {} that mentions vector memory and agent context",
            i
        );
        let _ = xavier_binary()
            .args(["add", &content, &format!("swal-bench/search-{}", i)])
            .output();
    }

    let queries = vec![
        "SWAL",
        "vector",
        "memory",
        "agent",
        "context",
        "benchmark",
        "something_that_does_not_exist_xyz",
    ];
    for q in &queries {
        let t = bench_cli(&["search", q], &format!("search-{}", q));
        println!("  ──── query '{}': {:.2?}", q, t);
    }
}

/// Benchmark #5: Help/error path validation
#[test]
fn swal_bench_edge_cases() {
    ensure_binary();
    println!("\n── SWAL Benchmark 5: Edge Cases ──");

    // Empty query
    bench_cli(&["search", ""], "search-empty");

    // Very long query
    let long_q = "a".repeat(10000);
    bench_cli(&["search", &long_q], "search-long");

    // Missing required args
    let output = xavier_binary()
        .arg("add")
        .output()
        .expect("add without args");
    println!(
        "  ──── add (no args): exit={}, stderr={}B",
        output.status.code().unwrap_or(-1),
        output.stderr.len()
    );
}

/// Benchmark #6: STREAMING — check concurrent path via stats
#[test]
fn swal_bench_stats() {
    ensure_binary();
    println!("\n── SWAL Benchmark 6: Stats Command ──");
    let t = bench_cli(&["stats"], "stats");
    println!("  ──── stats latency: {:.2?}", t);
}

/// Benchmark #7: RECALL command
#[test]
fn swal_bench_recall() {
    ensure_binary();
    println!("\n── SWAL Benchmark 7: Recall Command ──");
    let t = bench_cli(&["recall", "SWAL"], "recall");
    println!("  ──── recall latency: {:.2?}", t);
}
