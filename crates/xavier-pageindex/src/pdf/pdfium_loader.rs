//! Runtime pdfium library resolution.
//!
//! libpdfium is bound at runtime (never linked). Lookup order: the
//! `XAVIER_PAGEINDEX_PDFIUM_LIB` env, then `PDFIUM_DYNAMIC_LIB_PATH`, then a
//! library next to the executable, then the system library. Each env value may
//! be a directory or a file. Nothing here panics; failure is `PdfiumUnavailable`.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use pdfium_render::prelude::{Pdfium, PdfiumError};

use crate::error::PageIndexError;

/// Env var naming the pdfium directory or file (highest priority).
pub const ENV_PDFIUM_LIB: &str = "XAVIER_PAGEINDEX_PDFIUM_LIB";
/// Conventional env var used by pdfium-render tooling (second priority).
pub const ENV_PDFIUM_DYNAMIC: &str = "PDFIUM_DYNAMIC_LIB_PATH";

static PDFIUM: OnceLock<Pdfium> = OnceLock::new();
static INIT: Mutex<()> = Mutex::new(());

fn lib_file_name() -> PathBuf {
    PathBuf::from(Pdfium::pdfium_platform_library_name())
}

/// Expands a dir-or-file location into the library file path.
fn expand(location: &Path) -> PathBuf {
    if location.is_dir() {
        location.join(lib_file_name())
    } else {
        location.to_path_buf()
    }
}

/// Ordered candidate library paths (system library excluded), empty values skipped.
/// Pure: takes the env values and executable directory as arguments.
pub fn candidate_paths(
    env_lib: Option<&str>,
    env_dynamic: Option<&str>,
    exe_dir: Option<&Path>,
) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for v in [env_lib, env_dynamic].into_iter().flatten() {
        if !v.trim().is_empty() {
            out.push(expand(Path::new(v.trim())));
        }
    }
    if let Some(dir) = exe_dir {
        out.push(dir.join(lib_file_name()));
    }
    out
}

fn unavailable(msg: impl Into<String>) -> PageIndexError {
    PageIndexError::PdfiumUnavailable(msg.into())
}

/// Tries each existing candidate in order, then the system library when allowed.
/// Returns the first successful binding wrapped in a `Pdfium`.
pub fn bind_first(candidates: &[PathBuf], allow_system: bool) -> Result<Pdfium, PageIndexError> {
    let mut tried: Vec<String> = Vec::new();
    for path in candidates {
        if !path.exists() {
            tried.push(format!("{} (missing)", path.display()));
            continue;
        }
        match Pdfium::bind_to_library(path) {
            Ok(b) => return Ok(Pdfium::new(b)),
            Err(PdfiumError::PdfiumLibraryBindingsAlreadyInitialized) => {
                return Ok(Pdfium::default())
            }
            Err(e) => tried.push(format!("{} ({e})", path.display())),
        }
    }
    if allow_system {
        match Pdfium::bind_to_system_library() {
            Ok(b) => return Ok(Pdfium::new(b)),
            Err(PdfiumError::PdfiumLibraryBindingsAlreadyInitialized) => {
                return Ok(Pdfium::default())
            }
            Err(e) => tried.push(format!("system library ({e})")),
        }
    }
    Err(unavailable(format!(
        "libpdfium not found; tried: {}",
        if tried.is_empty() {
            "nothing".to_string()
        } else {
            tried.join(", ")
        }
    )))
}

/// Process-wide pdfium handle, bound on first success (failures are retried).
pub fn pdfium() -> Result<&'static Pdfium, PageIndexError> {
    if let Some(p) = PDFIUM.get() {
        return Ok(p);
    }
    let _guard = INIT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(p) = PDFIUM.get() {
        return Ok(p);
    }
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    let candidates = candidate_paths(
        std::env::var(ENV_PDFIUM_LIB).ok().as_deref(),
        std::env::var(ENV_PDFIUM_DYNAMIC).ok().as_deref(),
        exe_dir.as_deref(),
    );
    let bound = bind_first(&candidates, true)?;
    Ok(PDFIUM.get_or_init(|| bound))
}
