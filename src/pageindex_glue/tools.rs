//! Protocol-agnostic PageIndex tools. Every call returns a JSON envelope
//! (`{ok:true,...}` or `{ok:false,error,hint}`) and never panics or throws.

use std::path::Path;
use std::sync::Arc;

use serde::Serialize;
use serde_json::{json, Map, Value};
use xavier_pageindex::store::SqliteStore;
use xavier_pageindex::{
    BrowseQuery, IngestOptions, PageIndex, PageIndexError, SearchHit, Source, StructureOpts,
};

use super::state::PageIndexState;

pub const TOOL_BROWSE: &str = "pageindex_browse_documents";
pub const TOOL_GET_DOCUMENT: &str = "pageindex_get_document";
pub const TOOL_GET_STRUCTURE: &str = "pageindex_get_document_structure";
pub const TOOL_GET_PAGES: &str = "pageindex_get_page_content";
pub const TOOL_INDEX: &str = "pageindex_index_document";
pub const TOOL_SEARCH: &str = "pageindex_search";

/// Largest file `path` ingest will read.
const MAX_INGEST_BYTES: u64 = 64 * 1024 * 1024;
const BROWSE_DEFAULT_LIMIT: usize = 20;
const BROWSE_MAX_LIMIT: usize = 100;
const SEARCH_MAX_LIMIT: usize = 30;

#[derive(Debug, Clone, Serialize)]
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
}

/// Caller identity handed in by the protocol adapter.
#[derive(Debug, Clone)]
pub struct ToolContext {
    pub workspace: String,
    pub can_write: bool,
}

pub fn tool_specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: TOOL_BROWSE,
            description: "List indexed documents (newest first); optional query matches names and section titles.",
            input_schema: json!({"type":"object","properties":{
                "query":{"type":"string"},
                "limit":{"type":"integer","minimum":1,"maximum":BROWSE_MAX_LIMIT},
                "offset":{"type":"integer","minimum":0}}}),
        },
        ToolSpec {
            name: TOOL_GET_DOCUMENT,
            description: "Document status, page count and whether to read its structure first.",
            input_schema: json!({"type":"object","properties":{
                "doc_name":{"type":"string"}},"required":["doc_name"]}),
        },
        ToolSpec {
            name: TOOL_GET_STRUCTURE,
            description: "Section tree of a document (titles and page ranges, no page text); use it to pick pages.",
            input_schema: json!({"type":"object","properties":{
                "doc_name":{"type":"string"},
                "max_depth":{"type":"integer","minimum":1},
                "node_id":{"type":"string","description":"Return only this subtree"}},
                "required":["doc_name"]}),
        },
        ToolSpec {
            name: TOOL_GET_PAGES,
            description: "Text of pages such as \"5-7,12\" (max 20 pages per call, 1-based).",
            input_schema: json!({"type":"object","properties":{
                "doc_name":{"type":"string"},
                "pages":{"type":"string","description":"e.g. \"5-7,12\""}},
                "required":["doc_name","pages"]}),
        },
        ToolSpec {
            name: TOOL_SEARCH,
            description: "BM25 search across documents (or in one if doc_name is given): ranked pages with section breadcrumb and a short snippet.",
            input_schema: json!({"type":"object","properties":{
                "doc_name":{"type":"string"},
                "query":{"type":"string"},
                "limit":{"type":"integer","minimum":1,"maximum":SEARCH_MAX_LIMIT}},
                "required":["query"]}),
        },
        ToolSpec {
            name: TOOL_INDEX,
            description: "Index a document from inline content or an allowed file path (write access required).",
            input_schema: json!({"type":"object","properties":{
                "doc_name":{"type":"string"},
                "content":{"type":"string"},
                "path":{"type":"string","description":"File under XAVIER_PAGEINDEX_INGEST_ROOTS (required for pdf)"},
                "format":{"type":"string","enum":["markdown","text","legal","pdf"]},
                "summarize":{"type":"boolean"},
                "optimize":{"type":"boolean"}},
                "required":["doc_name"]}),
        },
    ]
}

/// Value of the envelope `kind` field for storage failures (HTTP 500).
pub const ERROR_KIND_STORE: &str = "store";

fn fail(error: impl Into<String>, hint: impl Into<String>) -> Value {
    json!({"ok": false, "error": error.into(), "hint": hint.into()})
}

fn ok_with(value: impl Serialize) -> Value {
    let mut obj = match serde_json::to_value(value) {
        Ok(Value::Object(m)) => m,
        Ok(other) => {
            let mut m = Map::new();
            m.insert("result".into(), other);
            m
        }
        Err(e) => return fail(format!("serialization failed: {e}"), "report this bug"),
    };
    obj.insert("ok".into(), Value::Bool(true));
    Value::Object(obj)
}

pub(crate) fn from_error(e: &PageIndexError) -> Value {
    let hint = match e {
        PageIndexError::NotFound(msg) => {
            if msg.contains("node_id") {
                "Copy node_id from get_document_structure"
            } else {
                "Check the name with pageindex_browse_documents"
            }
        }
        PageIndexError::InvalidRange(_) => {
            "Fix the range; use pageindex_get_document for page_count"
        }
        PageIndexError::FeatureDisabled(_) => {
            "This build lacks that format (PDF needs the pageindex-pdf feature); use markdown, text or legal"
        }
        PageIndexError::PdfiumUnavailable(_) => {
            "Set XAVIER_PAGEINDEX_PDFIUM_LIB to a pdfium library"
        }
        PageIndexError::Encrypted => "Remove the PDF password and retry",
        PageIndexError::Store(_) => "Storage failed; check XAVIER_PAGEINDEX_DB is writable",
        PageIndexError::Build(_) => "Check document status with pageindex_get_document",
    };
    let mut envelope = fail(e.to_string(), hint);
    if let (PageIndexError::Store(_), Some(obj)) = (e, envelope.as_object_mut()) {
        obj.insert("kind".into(), json!(ERROR_KIND_STORE));
    }
    envelope
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn bounded_usize(args: &Value, key: &str, min: usize, max: usize) -> Result<Option<usize>, Value> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let bounds_str = if max == usize::MAX {
                format!(">= {min}")
            } else {
                format!("{min}..{max}")
            };
            let make_err = || {
                fail(
                    format!("{key} must be {bounds_str}"),
                    format!("Pass '{key}' as an integer in {bounds_str}"),
                )
            };

            if let Some(n) = v.as_i64() {
                if n >= min as i64 && (max == usize::MAX || n <= max as i64) {
                    Ok(Some(n as usize))
                } else {
                    Err(make_err())
                }
            } else if let Some(n) = v.as_u64() {
                if n >= min as u64 && (max == usize::MAX || n <= max as u64) {
                    Ok(Some(n as usize))
                } else {
                    Err(make_err())
                }
            } else {
                Err(make_err())
            }
        }
    }
}

fn validate_doc_name(name: &str) -> Result<(), Value> {
    if name.contains("..") {
        return Err(fail(
            "invalid doc_name: must not contain '..'",
            "Provide a clean document name without path traversal",
        ));
    }
    if name.starts_with('/') {
        return Err(fail(
            "invalid doc_name: must not start with '/'",
            "Provide a relative document name without leading '/'",
        ));
    }
    if name.chars().any(|c| c.is_control()) {
        return Err(fail(
            "invalid doc_name: must not contain control characters",
            "Provide a document name without control characters",
        ));
    }
    if name.chars().count() > 256 {
        return Err(fail(
            "invalid doc_name: must not exceed 256 characters",
            "Shorten the document name to 256 characters or fewer",
        ));
    }
    Ok(())
}

fn need_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, Value> {
    str_arg(args, key).ok_or_else(|| {
        fail(
            format!("missing required parameter '{key}'"),
            format!("Pass '{key}' as a non-empty string"),
        )
    })
}

/// Run one tool. Blocking store work happens on the blocking pool.
pub async fn call(
    state: &Arc<PageIndexState>,
    name: &str,
    args: &Value,
    ctx: &ToolContext,
) -> Value {
    let name = name.to_string();
    let args = args.clone();
    let ctx = ctx.clone();
    let st = Arc::clone(state);
    match tokio::task::spawn_blocking(move || dispatch(&st, &name, &args, &ctx)).await {
        Ok(v) => v,
        Err(e) => fail(
            format!("tool task failed: {e}"),
            "retry; report if it persists",
        ),
    }
}

fn dispatch(state: &PageIndexState, name: &str, args: &Value, ctx: &ToolContext) -> Value {
    let run = || -> Result<Value, Value> {
        match name {
            TOOL_INDEX => index_document(state, args, ctx),
            TOOL_BROWSE | TOOL_GET_DOCUMENT | TOOL_GET_STRUCTURE | TOOL_GET_PAGES | TOOL_SEARCH => {
                let idx = state.index().map_err(|e| from_error(&e))?;
                read_tool(&idx, name, args, ctx)
            }
            other => Err(fail(
                format!("unknown pageindex tool '{other}'"),
                "List the available tools with tool_specs()",
            )),
        }
    };
    // A panic inside the store or a builder must still yield an envelope.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)) {
        Ok(Ok(v)) | Ok(Err(v)) => v,
        Err(_) => fail(
            "internal error while running the tool",
            "retry; report if it persists",
        ),
    }
}

fn read_tool(
    idx: &PageIndex<SqliteStore>,
    name: &str,
    args: &Value,
    ctx: &ToolContext,
) -> Result<Value, Value> {
    let ws = ctx.workspace.as_str();
    let map = |e: PageIndexError| from_error(&e);
    match name {
        TOOL_BROWSE => {
            let limit =
                bounded_usize(args, "limit", 1, BROWSE_MAX_LIMIT)?.unwrap_or(BROWSE_DEFAULT_LIMIT);
            let offset = bounded_usize(args, "offset", 0, usize::MAX)?.unwrap_or(0);
            let q = BrowseQuery {
                query: str_arg(args, "query").map(str::to_string),
                offset,
                limit,
            };
            idx.browse(ws, q).map(ok_with).map_err(map)
        }
        TOOL_GET_DOCUMENT => {
            let doc = need_str(args, "doc_name")?;
            idx.get_document(ws, doc).map(ok_with).map_err(map)
        }
        TOOL_GET_STRUCTURE => {
            let doc = need_str(args, "doc_name")?;
            let max_depth = bounded_usize(args, "max_depth", 1, usize::MAX)?;
            let opts = StructureOpts {
                max_depth: max_depth.map(|n| n as u32),
                node_id: str_arg(args, "node_id").map(str::to_string),
                char_budget: None,
            };
            idx.get_structure(ws, doc, opts).map(ok_with).map_err(map)
        }
        TOOL_SEARCH => {
            let query = need_str(args, "query")?;
            let limit_opt = bounded_usize(args, "limit", 1, SEARCH_MAX_LIMIT)?;
            let doc_opt = match args.get("doc_name") {
                None | Some(Value::Null) => None,
                Some(v) => {
                    if let Some(s) = v.as_str().map(str::trim).filter(|s| !s.is_empty()) {
                        Some(s)
                    } else {
                        return Err(fail(
                            "parameter 'doc_name' must be a non-empty string",
                            "Pass 'doc_name' as a non-empty string or omit it to search all documents",
                        ));
                    }
                }
            };
            if let Some(doc) = doc_opt {
                let limit = limit_opt.unwrap_or(0);
                idx.search(ws, doc, query, limit)
                    .map(ok_with)
                    .map_err(|e| match e {
                        PageIndexError::InvalidRange(m) => {
                            fail(m, "Pass words of 2+ letters or digits as 'query'")
                        }
                        other => from_error(&other),
                    })
            } else {
                search_workspace(idx, ws, query, limit_opt.unwrap_or(0))
            }
        }
        _ => {
            let doc = need_str(args, "doc_name")?;
            let pages = need_str(args, "pages")?;
            idx.get_pages(ws, doc, pages, 0).map(ok_with).map_err(map)
        }
    }
}

fn search_workspace(
    idx: &PageIndex<SqliteStore>,
    ws: &str,
    query: &str,
    limit: usize,
) -> Result<Value, Value> {
    let terms = xavier_pageindex::eval::tokenize(query);
    if terms.is_empty() {
        return Err(fail(
            "query has no searchable terms; use words of 2+ letters or digits",
            "Pass words of 2+ letters or digits as 'query'",
        ));
    }
    let effective_limit = match limit {
        0 => xavier_pageindex::search::DEFAULT_SEARCH_LIMIT,
        n => n.min(SEARCH_MAX_LIMIT),
    };

    let ready_docs = idx.ready_document_ids(ws).map_err(|e| from_error(&e))?;

    struct HitAcrossDocs {
        doc_name: String,
        hit: SearchHit,
    }

    let mut all_hits: Vec<HitAcrossDocs> = Vec::new();
    let mut total_matches = 0usize;

    for (name, _) in ready_docs {
        match idx.search(ws, &name, query, effective_limit) {
            Ok(res) => {
                total_matches += res.total_matches;
                for hit in res.hits {
                    all_hits.push(HitAcrossDocs {
                        doc_name: name.clone(),
                        hit,
                    });
                }
            }
            Err(PageIndexError::InvalidRange(m)) => {
                return Err(fail(m, "Pass words of 2+ letters or digits as 'query'"));
            }
            Err(_) => {}
        }
    }

    let max_score = all_hits.iter().map(|h| h.hit.score).fold(0.0f64, f64::max);

    for h in &mut all_hits {
        if max_score > 0.0 {
            h.hit.score = ((h.hit.score / max_score) * 1000.0).round() / 1000.0;
        }
    }

    all_hits.sort_by(|a, b| {
        b.hit
            .score
            .total_cmp(&a.hit.score)
            .then_with(|| a.doc_name.cmp(&b.doc_name))
            .then_with(|| a.hit.page_no.cmp(&b.hit.page_no))
    });

    let truncated = total_matches > effective_limit;
    let selected_hits: Vec<HitAcrossDocs> = all_hits.into_iter().take(effective_limit).collect();

    let hits_json: Vec<Value> = selected_hits
        .into_iter()
        .map(|h| {
            json!({
                "doc_name": h.doc_name,
                "page_no": h.hit.page_no,
                "node_id": h.hit.node_id,
                "breadcrumb": h.hit.breadcrumb,
                "score": h.hit.score,
                "snippet": h.hit.snippet,
            })
        })
        .collect();

    let next_steps: Vec<String> = if hits_json.is_empty() {
        vec!["No page matched; try other or fewer terms, or browse documents with pageindex_browse_documents".into()]
    } else {
        vec!["Fetch the top pages with get_page_content() before answering".into()]
    };

    Ok(json!({
        "ok": true,
        "query": query.trim(),
        "hits": hits_json,
        "total_matches": total_matches,
        "truncated": truncated,
        "next_steps": next_steps,
    }))
}

fn index_document(state: &PageIndexState, args: &Value, ctx: &ToolContext) -> Result<Value, Value> {
    if !ctx.can_write {
        return Err(fail(
            "permission denied: indexing documents requires write access",
            "Use a role that may write memory",
        ));
    }
    let doc_name = need_str(args, "doc_name")?;
    validate_doc_name(doc_name)?;
    let content = args.get("content").and_then(Value::as_str);
    let path = str_arg(args, "path");
    let (bytes, ext_format) = match (content, path) {
        (Some(_), Some(_)) | (None, None) => {
            return Err(fail(
                "pass exactly one of 'content' or 'path'",
                "Use 'content' for inline text or 'path' for an allowed file",
            ))
        }
        (Some(c), None) => (c.as_bytes().to_vec(), None),
        (None, Some(p)) => {
            let resolved = resolve_ingest_path(&state.settings.ingest_roots, p)?;
            let bytes = std::fs::read(&resolved).map_err(|e| {
                fail(
                    format!("cannot read file: {e}"),
                    "Check the path and permissions",
                )
            })?;
            (bytes, format_from_extension(&resolved))
        }
    };
    let format = str_arg(args, "format")
        .or(ext_format)
        .unwrap_or("text")
        .to_ascii_lowercase();
    if format == "pdf" && path.is_none() {
        return Err(fail(
            "PDF ingest needs 'path' (binary files cannot travel as inline content)",
            "Pass 'path' to a .pdf under XAVIER_PAGEINDEX_INGEST_ROOTS",
        ));
    }
    // Detected secret values are replaced before anything reaches the index.
    let text = |b: &[u8]| {
        let decoded = std::str::from_utf8(b).map_err(|_| {
            fail(
                "document is not valid UTF-8 text",
                "Convert it to UTF-8 or use format=pdf",
            )
        })?;
        Ok::<String, Value>(crate::security::ingest_guard::redact_secrets(decoded).0)
    };
    let owned;
    let source = match format.as_str() {
        "pdf" => Source::Pdf(&bytes),
        "markdown" | "md" => {
            owned = text(&bytes)?;
            Source::Markdown(&owned)
        }
        "text" | "txt" => {
            owned = text(&bytes)?;
            Source::PlainText(&owned)
        }
        "legal" => {
            owned = text(&bytes)?;
            Source::Legal(&owned)
        }
        other => {
            return Err(fail(
                format!("unknown format '{other}'"),
                "Use one of markdown, text, legal, pdf",
            ))
        }
    };
    let opts = IngestOptions {
        summarize: args
            .get("summarize")
            .and_then(Value::as_bool)
            .unwrap_or(state.settings.summarize),
        optimize: args
            .get("optimize")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        lines_per_page: state.settings.lines_per_page,
    };
    let idx = state.index().map_err(|e| from_error(&e))?;
    let ws = ctx.workspace.as_str();
    let before = idx.get_document(ws, doc_name).ok().map(|d| d.doc_id);
    let doc = idx
        .ingest(ws, doc_name, source, opts)
        .map_err(|e| from_error(&e))?;
    let node_count = idx
        .get_document(ws, doc_name)
        .map(|d| d.node_count)
        .unwrap_or(0);
    Ok(json!({
        "ok": true,
        "name": doc.name,
        "page_count": doc.page_count,
        "node_count": node_count,
        "builder": doc.builder,
        "unchanged": before.as_deref() == Some(doc.doc_id.as_str()),
    }))
}

fn format_from_extension(p: &Path) -> Option<&'static str> {
    match p.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "md" | "markdown" => Some("markdown"),
        "txt" => Some("text"),
        "pdf" => Some("pdf"),
        _ => None,
    }
}

/// Canonicalize `raw` and require it to sit under one of the allowed roots.
fn resolve_ingest_path(
    roots: &[std::path::PathBuf],
    raw: &str,
) -> Result<std::path::PathBuf, Value> {
    if roots.is_empty() {
        return Err(fail(
            "path ingest is disabled",
            "Set XAVIER_PAGEINDEX_INGEST_ROOTS or pass 'content' instead",
        ));
    }
    let denied = || {
        fail(
            "path is outside the allowed ingest roots",
            "Use a file under XAVIER_PAGEINDEX_INGEST_ROOTS",
        )
    };
    let resolved = std::fs::canonicalize(raw).map_err(|_| denied())?;
    let inside = roots
        .iter()
        .filter_map(|r| std::fs::canonicalize(r).ok())
        .any(|r| resolved.starts_with(r));
    if !inside {
        return Err(denied());
    }
    if crate::security::ingest_guard::is_secret_path(&resolved) {
        return Err(fail(
            "refusing to ingest a secret-bearing file",
            "Choose a file that is not a credential or key store",
        ));
    }
    let meta = std::fs::metadata(&resolved).map_err(|_| denied())?;
    if !meta.is_file() {
        return Err(fail("path is not a regular file", "Pass a file path"));
    }
    if meta.len() > MAX_INGEST_BYTES {
        return Err(fail(
            "file is too large to index",
            "Split the file below 64 MiB",
        ));
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pageindex_glue::settings::PageIndexSettings;

    fn state_with(roots: Vec<std::path::PathBuf>) -> Arc<PageIndexState> {
        let settings = PageIndexSettings {
            ingest_roots: roots,
            ..PageIndexSettings::default()
        };
        Arc::new(PageIndexState::with_store(
            settings,
            SqliteStore::open_in_memory().unwrap(),
        ))
    }

    fn rw() -> ToolContext {
        ToolContext {
            workspace: "ws".into(),
            can_write: true,
        }
    }

    #[test]
    fn test_tool_specs_names_and_required_params() {
        let specs = tool_specs();
        let names: Vec<_> = specs.iter().map(|s| s.name).collect();
        assert_eq!(
            names,
            [
                TOOL_BROWSE,
                TOOL_GET_DOCUMENT,
                TOOL_GET_STRUCTURE,
                TOOL_GET_PAGES,
                TOOL_SEARCH,
                TOOL_INDEX
            ]
        );
        let required = |n: &str| -> Vec<String> {
            let s = specs.iter().find(|s| s.name == n).unwrap();
            s.input_schema["required"]
                .as_array()
                .map(|a| a.iter().map(|v| v.as_str().unwrap().to_string()).collect())
                .unwrap_or_default()
        };
        assert!(required(TOOL_BROWSE).is_empty());
        assert_eq!(required(TOOL_GET_DOCUMENT), ["doc_name"]);
        assert_eq!(required(TOOL_GET_STRUCTURE), ["doc_name"]);
        assert_eq!(required(TOOL_GET_PAGES), ["doc_name", "pages"]);
        assert_eq!(required(TOOL_SEARCH), ["query"]);
        assert_eq!(required(TOOL_INDEX), ["doc_name"]);
        assert!(specs.iter().all(|s| s.description.len() <= 200));
    }

    #[tokio::test]
    async fn test_tool_errors_are_envelopes_not_panics() {
        let st = state_with(vec![]);
        for (tool, args) in [
            (TOOL_GET_DOCUMENT, json!({})),
            (TOOL_GET_DOCUMENT, json!({"doc_name": "nope"})),
            (TOOL_GET_STRUCTURE, json!({"doc_name": 5})),
            (TOOL_GET_PAGES, json!({"doc_name": "nope", "pages": "1"})),
            (TOOL_INDEX, json!({"doc_name": "a"})),
            ("pageindex_bogus", json!({})),
        ] {
            let v = call(&st, tool, &args, &rw()).await;
            assert_eq!(v["ok"], false, "{tool}: {v}");
            assert!(
                v["error"].is_string() && v["hint"].is_string(),
                "{tool}: {v}"
            );
        }
        let put = json!({"doc_name": "guide", "content": "# One\ntext\n## Two\nmore\n", "format": "markdown"});
        assert_eq!(call(&st, TOOL_INDEX, &put, &rw()).await["ok"], true);
        let v = call(
            &st,
            TOOL_GET_PAGES,
            &json!({"doc_name": "guide", "pages": "99"}),
            &rw(),
        )
        .await;
        assert_eq!(v["ok"], false, "{v}");
        let v = call(&st, TOOL_GET_DOCUMENT, &json!({"doc_name": "guid"}), &rw()).await;
        assert!(v["error"].as_str().unwrap().contains("guide"), "{v}");
    }

    #[tokio::test]
    async fn test_ingest_then_read_roundtrip() {
        let st = state_with(vec![]);
        let put = json!({"doc_name": "guide", "content": "# One\ntext\n## Two\nmore\n", "format": "markdown"});
        let v = call(&st, TOOL_INDEX, &put, &rw()).await;
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["unchanged"], false);
        assert_eq!(v["builder"], "markdown");
        let again = call(&st, TOOL_INDEX, &put, &rw()).await;
        assert_eq!(again["unchanged"], true, "{again}");
        let s = call(
            &st,
            TOOL_GET_STRUCTURE,
            &json!({"doc_name": "guide"}),
            &rw(),
        )
        .await;
        assert_eq!(s["ok"], true, "{s}");
        assert!(!s["nodes"].as_array().unwrap().is_empty());
        let p = call(
            &st,
            TOOL_GET_PAGES,
            &json!({"doc_name": "guide", "pages": "1"}),
            &rw(),
        )
        .await;
        assert!(
            p["pages"][0]["text"].as_str().unwrap().contains("text"),
            "{p}"
        );
        let b = call(&st, TOOL_BROWSE, &json!({}), &rw()).await;
        assert_eq!(b["total"], 1, "{b}");
    }

    #[tokio::test]
    async fn test_ingest_tool_denied_for_readonly_role() {
        let st = state_with(vec![]);
        let ro = ToolContext {
            workspace: "ws".into(),
            can_write: false,
        };
        let v = call(
            &st,
            TOOL_INDEX,
            &json!({"doc_name": "a", "content": "hi"}),
            &ro,
        )
        .await;
        assert_eq!(v["ok"], false);
        assert!(
            v["error"].as_str().unwrap().contains("permission denied"),
            "{v}"
        );
        let b = call(&st, TOOL_BROWSE, &json!({}), &ro).await;
        assert_eq!(b["total"], 0, "nothing was stored: {b}");
    }

    #[tokio::test]
    async fn test_ingest_path_outside_roots_rejected() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("in.md"), "# Title\nbody\n").unwrap();
        std::fs::write(outside.path().join("out.md"), "# Title\nbody\n").unwrap();
        let st = state_with(vec![root.path().to_path_buf()]);

        let out = outside.path().join("out.md");
        let v = call(
            &st,
            TOOL_INDEX,
            &json!({"doc_name": "o", "path": out}),
            &rw(),
        )
        .await;
        assert_eq!(v["ok"], false, "{v}");
        assert!(v["error"].as_str().unwrap().contains("outside"), "{v}");

        let sneaky = root
            .path()
            .join("..")
            .join(outside.path().file_name().unwrap())
            .join("out.md");
        let v = call(
            &st,
            TOOL_INDEX,
            &json!({"doc_name": "o", "path": sneaky}),
            &rw(),
        )
        .await;
        assert_eq!(v["ok"], false, "{v}");

        let inside = root.path().join("in.md");
        let v = call(
            &st,
            TOOL_INDEX,
            &json!({"doc_name": "i", "path": inside}),
            &rw(),
        )
        .await;
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["builder"], "markdown");

        let closed = state_with(vec![]);
        let v = call(
            &closed,
            TOOL_INDEX,
            &json!({"doc_name": "i", "path": root.path().join("in.md")}),
            &rw(),
        )
        .await;
        assert_eq!(v["ok"], false);
        assert!(v["error"].as_str().unwrap().contains("disabled"), "{v}");
    }

    #[tokio::test]
    async fn test_path_ingest_rejects_secret_filenames() {
        let root = tempfile::tempdir().unwrap();
        for name in [
            ".env",
            "credentials.json",
            "id_rsa",
            "server.pem",
            "record.key",
        ] {
            std::fs::write(
                root.path().join(name),
                "API_KEY=FAKEkey0123456789ABCDEFGH\n",
            )
            .unwrap();
            let st = state_with(vec![root.path().to_path_buf()]);
            let v = call(
                &st,
                TOOL_INDEX,
                &json!({"doc_name": name, "path": root.path().join(name)}),
                &rw(),
            )
            .await;
            assert_eq!(v["ok"], false, "{name}: {v}");
            assert!(
                v["error"].as_str().unwrap().contains("secret"),
                "{name}: {v}"
            );
        }
    }

    #[tokio::test]
    async fn test_path_ingest_redacts_allowed_file_content() {
        let root = tempfile::tempdir().unwrap();
        let secret = "FAKEkey0123456789ABCDEFGH";
        std::fs::write(
            root.path().join("notes.md"),
            format!("# Note\nAPI_KEY={secret}\n"),
        )
        .unwrap();
        let st = state_with(vec![root.path().to_path_buf()]);
        let v = call(
            &st,
            TOOL_INDEX,
            &json!({"doc_name": "n", "path": root.path().join("notes.md")}),
            &rw(),
        )
        .await;
        assert_eq!(v["ok"], true, "{v}");
        let pages = call(
            &st,
            TOOL_GET_PAGES,
            &json!({"doc_name": "n", "pages": "1"}),
            &rw(),
        )
        .await;
        assert_eq!(pages["ok"], true, "{pages}");
        let text = pages["pages"][0]["text"].as_str().unwrap_or_default();
        assert!(!text.contains(secret), "{pages}");
    }

    #[tokio::test]
    async fn test_pdf_ingest_requires_path_not_inline_content() {
        let st = state_with(vec![]);
        let v = call(
            &st,
            TOOL_INDEX,
            &json!({"doc_name": "p", "content": "%PDF-1.4", "format": "pdf"}),
            &rw(),
        )
        .await;
        assert_eq!(v["ok"], false, "{v}");
        assert!(v["error"].as_str().unwrap().contains("'path'"), "{v}");
    }

    #[tokio::test]
    async fn test_pdf_path_ingest_envelope_matches_build_features() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.pdf"), b"%PDF-1.4 not really").unwrap();
        let st = state_with(vec![root.path().to_path_buf()]);
        let v = call(
            &st,
            TOOL_INDEX,
            &json!({"doc_name": "a", "path": root.path().join("a.pdf")}),
            &rw(),
        )
        .await;
        assert_eq!(v["ok"], false, "{v}");
        let err = v["error"].as_str().unwrap();
        if cfg!(feature = "pageindex-pdf") {
            // Extension picks format=pdf; the broken file is a build error.
            assert!(err.starts_with("build error"), "{v}");
        } else {
            assert!(err.contains("feature disabled: pdf"), "{v}");
        }
        assert!(v["hint"].is_string());
    }

    #[tokio::test]
    async fn search_without_doc_name_ranks_across_documents() {
        let st = state_with(vec![]);
        let doc1 = json!({
            "doc_name": "doc_a",
            "content": "# Guide A\nThis is a general guide.\n## Details\nSome generic text here.\n",
            "format": "markdown"
        });
        let doc2 = json!({
            "doc_name": "doc_b",
            "content": "# Guide B\nRust compiler internals.\n## Section\nRust compiler optimization passes.\n",
            "format": "markdown"
        });
        assert_eq!(call(&st, TOOL_INDEX, &doc1, &rw()).await["ok"], true);
        assert_eq!(call(&st, TOOL_INDEX, &doc2, &rw()).await["ok"], true);

        let res = call(
            &st,
            TOOL_SEARCH,
            &json!({"query": "rust compiler", "limit": 5}),
            &rw(),
        )
        .await;

        assert_eq!(res["ok"], true, "{res}");
        assert_eq!(res["query"], "rust compiler");
        let hits = res["hits"].as_array().expect("hits array");
        assert!(!hits.is_empty(), "expected hits across documents");
        for hit in hits {
            assert!(
                hit.get("doc_name").and_then(Value::as_str).is_some(),
                "each hit must include doc_name: {hit:?}"
            );
            let score = hit["score"].as_f64().expect("score as float");
            assert!(
                (0.0..=1.0).contains(&score),
                "score must be normalized: {score}"
            );
        }
        assert_eq!(hits[0]["doc_name"], "doc_b");
    }

    #[tokio::test]
    async fn browse_rejects_out_of_range_limit() {
        let st = state_with(vec![]);
        let v0 = call(&st, TOOL_BROWSE, &json!({"limit": 0}), &rw()).await;
        assert_eq!(v0["ok"], false, "{v0}");
        let err0 = v0["error"].as_str().unwrap();
        assert!(err0.contains("limit"), "{v0}");
        assert!(err0.contains("1..100"), "{v0}");

        let v101 = call(&st, TOOL_BROWSE, &json!({"limit": 101}), &rw()).await;
        assert_eq!(v101["ok"], false, "{v101}");
        let err101 = v101["error"].as_str().unwrap();
        assert!(err101.contains("limit"), "{v101}");
        assert!(err101.contains("1..100"), "{v101}");

        let va = call(&st, TOOL_BROWSE, &json!({"limit": "a"}), &rw()).await;
        assert_eq!(va["ok"], false, "{va}");
        let erra = va["error"].as_str().unwrap();
        assert!(erra.contains("limit"), "{va}");
        assert!(erra.contains("1..100"), "{va}");
    }

    #[tokio::test]
    async fn index_rejects_traversal_doc_name() {
        let st = state_with(vec![]);
        for bad_name in [
            "../secret.md",
            "dir/../secret.md",
            "/etc/passwd",
            "bad\x00name",
            &"a".repeat(257),
        ] {
            let res = call(
                &st,
                TOOL_INDEX,
                &json!({"doc_name": bad_name, "content": "text", "format": "text"}),
                &rw(),
            )
            .await;
            assert_eq!(
                res["ok"], false,
                "name {bad_name:?} should be rejected: {res}"
            );
            let err = res["error"].as_str().unwrap();
            assert!(
                err.contains("doc_name"),
                "error must mention doc_name: {res}"
            );
        }
    }

    #[tokio::test]
    async fn structure_node_not_found_hint() {
        let st = state_with(vec![]);
        let put = json!({
            "doc_name": "guide",
            "content": "# Section One\nsome content\n",
            "format": "markdown"
        });
        assert_eq!(call(&st, TOOL_INDEX, &put, &rw()).await["ok"], true);

        let res = call(
            &st,
            TOOL_GET_STRUCTURE,
            &json!({"doc_name": "guide", "node_id": "nonexistent_node"}),
            &rw(),
        )
        .await;

        assert_eq!(res["ok"], false, "{res}");
        let hint = res["hint"].as_str().expect("hint string");
        assert_ne!(
            hint, "Check the name with pageindex_browse_documents",
            "node-not-found must not use generic browse documents hint: {res}"
        );
        assert!(
            hint.to_lowercase()
                .contains("copy node_id from get_document_structure"),
            "hint must advise copying node_id from get_document_structure: {res}"
        );
    }
}
