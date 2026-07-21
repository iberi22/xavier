// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! # Scanner Submodules — Multi-Layer Maturity Scanning
//!
//! Each module implements one scanning layer. The orchestrator runs them
//! sequentially (not parallel) with per-layer timeouts to guarantee
//! completion within a reasonable wall-clock budget.
//!
//! ## Modules
//!
//! - `old_types`: Original types (`CodeGraphScan`, `TestListScan`) + standalone fns — backward compat
//! - `code_graph`: Layer 1 — static code analysis (code graph DB → fallback grep)
//! - `test_scanner`: Layer 2 — test anchor analysis (grep-limited, 30s timeout)
//! - `memory_scanner`: Layer 3 — evidence from sessions & usages
//! - `conversations_scanner`: Layer 4 — project intelligence

pub mod code_graph;
pub mod conversations_scanner;
pub mod memory_scanner;
pub mod old_types;
pub mod test_scanner;

// Re-export backward-compatible types
pub use old_types::{CodeGraphScan, TestListScan};

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Timing info for each layer.
#[derive(Debug, Clone)]
pub struct LayerTiming {
    pub static_ms: u64,
    pub dynamic_ms: u64,
    pub memory_ms: u64,
    pub conversations_ms: u64,
    pub total_ms: u64,
}

/// Aggregated evidence from all layers.
#[derive(Debug, Clone)]
pub struct DeepScanEvidence {
    /// Static symbols found per feature id: (found, total)
    pub static_results: HashMap<String, (usize, usize)>,
    /// Test results per feature id: (passing_names, total_names)
    pub test_results: HashMap<String, (Vec<String>, Vec<String>)>,
    /// Memory evidence per feature id
    pub memory_evidence: HashMap<String, memory_scanner::MemoryEvidence>,
    /// Conversation evidence per feature id
    pub conversation_evidence: HashMap<String, conversations_scanner::ConversationEvidence>,
    /// Any scan errors
    pub errors: Vec<String>,
    /// Layer timing
    pub timing: LayerTiming,
}

/// Progress callback type: receives a JSON status update after each layer.
pub type ProgressCallback<'a> = Box<dyn Fn(&str) + Send + 'a>;

/// Run a closure with a hard timeout on a dedicated thread.
/// If the closure completes, returns `Ok(value)`. If it times out or panics,
/// returns `Err(error_msg)`.
fn run_with_timeout<T: Send + 'static>(
    label: &str,
    timeout: Duration,
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    let finished = Arc::new(AtomicBool::new(false));
    let finished_clone = finished.clone();
    let (tx, rx) = mpsc::sync_channel::<T>(1);

    std::thread::spawn(move || {
        let result = f();
        finished_clone.store(true, Ordering::SeqCst);
        // Ignore error if receiver dropped (timeout already passed)
        let _ = tx.send(result);
    });

    match rx.recv_timeout(timeout) {
        Ok(val) => {
            finished.store(true, Ordering::SeqCst);
            Ok(val)
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            Err(format!("Layer '{}' timed out after {:?}", label, timeout))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err(format!("Layer '{}' thread panicked/disconnected", label))
        }
    }
}

/// Run all four scanning layers **sequentially** with per-layer timeouts (30s each).
/// Layers that fail/timeout produce empty results. Errors are collected but non-fatal.
/// An optional progress callback is invoked after each layer completes.
pub fn scan_all(codebase_root: &str) -> DeepScanEvidence {
    scan_all_with_progress(codebase_root, None)
}

/// Like `scan_all`, but accepts a progress callback invoked after each layer finishes.
/// The callback receives a JSON string with layer name and timing.
pub fn scan_all_with_progress(
    codebase_root: &str,
    on_progress: Option<&dyn Fn(&str)>,
) -> DeepScanEvidence {
    let start = Instant::now();
    let mut errors: Vec<String> = Vec::new();
    // Clone root into owned String so closures can be 'static
    let root = codebase_root.to_string();

    // Layer 1: Static (code_graph)
    let (static_res, static_err) = {
        let root = root.clone();
        match run_with_timeout("static", Duration::from_secs(30), move || {
            code_graph::scan_code_graph(&root)
        }) {
            Ok(val) => (val, None),
            Err(e) => {
                let err_clone = e.clone();
                (
                    code_graph::CodeGraphScanResult {
                        feature_scans: HashMap::new(),
                        total_symbols: 0,
                        total_found: 0,
                        errors: vec![e],
                        timing_ms: 0,
                    },
                    Some(err_clone),
                )
            }
        }
    };
    if let Some(e) = static_err {
        errors.push(e);
    }
    emit_progress(
        on_progress,
        "static",
        &static_res.timing_ms,
        &static_res.errors,
    );

    // Layer 2: Test scanner
    let (test_res, test_err) = {
        let root = root.clone();
        match run_with_timeout("test", Duration::from_secs(30), move || {
            test_scanner::list_tests(&root)
        }) {
            Ok(val) => (val, None),
            Err(e) => {
                let err_clone = e.clone();
                (
                    test_scanner::TestListScanResult {
                        all_tests: Vec::new(),
                        feature_tests: HashMap::new(),
                        errors: vec![e],
                        timing_ms: 0,
                    },
                    Some(err_clone),
                )
            }
        }
    };
    if let Some(e) = test_err {
        errors.push(e);
    }
    emit_progress(on_progress, "test", &test_res.timing_ms, &test_res.errors);

    // Layer 3: Memory scanner
    let (mem_res, mem_err) = {
        let root = root.clone();
        match run_with_timeout("memory", Duration::from_secs(30), move || {
            memory_scanner::scan_memory(&root)
        }) {
            Ok(val) => (val, None),
            Err(e) => {
                let err_clone = e.clone();
                (
                    memory_scanner::MemoryScanResult {
                        feature_evidence: HashMap::new(),
                        errors: vec![e],
                        timing_ms: 0,
                    },
                    Some(err_clone),
                )
            }
        }
    };
    if let Some(e) = mem_err {
        errors.push(e);
    }
    emit_progress(on_progress, "memory", &mem_res.timing_ms, &mem_res.errors);

    // Layer 4: Conversations scanner — use root directly (no clone needed after last use)
    let (conv_res, conv_err) = {
        match run_with_timeout("conversations", Duration::from_secs(30), move || {
            conversations_scanner::scan_conversations(&root)
        }) {
            Ok(val) => (val, None),
            Err(e) => {
                let err_clone = e.clone();
                (
                    conversations_scanner::ConversationScanResult {
                        feature_evidence: HashMap::new(),
                        errors: vec![e],
                        timing_ms: 0,
                    },
                    Some(err_clone),
                )
            }
        }
    };
    if let Some(e) = conv_err {
        errors.push(e);
    }
    emit_progress(
        on_progress,
        "conversations",
        &conv_res.timing_ms,
        &conv_res.errors,
    );

    let elapsed = start.elapsed();

    errors.extend(static_res.errors.iter().cloned());
    errors.extend(test_res.errors.iter().cloned());
    errors.extend(mem_res.errors.iter().cloned());
    errors.extend(conv_res.errors.iter().cloned());

    let static_results = build_static_results(&static_res);
    let test_results = build_test_results(&test_res);
    let memory_evidence = build_memory_evidence(&mem_res);
    let conversation_evidence = build_conversation_evidence(&conv_res);

    DeepScanEvidence {
        static_results,
        test_results,
        memory_evidence,
        conversation_evidence,
        errors,
        timing: LayerTiming {
            static_ms: static_res.timing_ms,
            dynamic_ms: test_res.timing_ms,
            memory_ms: mem_res.timing_ms,
            conversations_ms: conv_res.timing_ms,
            total_ms: elapsed.as_millis() as u64,
        },
    }
}

fn emit_progress(cb: Option<&dyn Fn(&str)>, layer: &str, timing_ms: &u64, layer_errors: &[String]) {
    if let Some(cb) = cb {
        let progress = serde_json::json!({
            "event": "layer_complete",
            "layer": layer,
            "timing_ms": timing_ms,
            "errors": layer_errors,
        });
        cb(&progress.to_string());
    }
}

fn build_static_results(res: &code_graph::CodeGraphScanResult) -> HashMap<String, (usize, usize)> {
    let mut map = HashMap::new();
    for (feat_id, scan) in &res.feature_scans {
        map.insert(
            feat_id.clone(),
            (scan.found.len(), scan.missing.len() + scan.found.len()),
        );
    }
    map
}

fn build_test_results(
    res: &test_scanner::TestListScanResult,
) -> HashMap<String, (Vec<String>, Vec<String>)> {
    let mut map = HashMap::new();
    for (feat_id, (passing, total)) in &res.feature_tests {
        map.insert(feat_id.clone(), (passing.clone(), total.clone()));
    }
    map
}

fn build_memory_evidence(
    res: &memory_scanner::MemoryScanResult,
) -> HashMap<String, memory_scanner::MemoryEvidence> {
    let mut map = HashMap::new();
    for (feat_id, evidence) in &res.feature_evidence {
        map.insert(feat_id.clone(), evidence.clone());
    }
    map
}

fn build_conversation_evidence(
    res: &conversations_scanner::ConversationScanResult,
) -> HashMap<String, conversations_scanner::ConversationEvidence> {
    let mut map = HashMap::new();
    for (feat_id, evidence) in &res.feature_evidence {
        map.insert(feat_id.clone(), evidence.clone());
    }
    map
}
