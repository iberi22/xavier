//! One shared, policy-checked reader for package members, and the text scans the
//! audit runs over what it read.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use regex::Regex;

use super::super::policy::{AuthorizedDir, PACKAGE_BOUNDS};

/// Agent Skills spec limits (04 §4): `description` 1..=1024 chars.
pub const DESCRIPTION_MAX: usize = 1024;

/// Read one package member as UTF-8 text under the same policy the scanner
/// applies. A symlinked member, a special file, a path that leaves the package
/// or a member over the size cap yields `None`, and none of them is ever opened.
pub fn read_member_text(dir: &AuthorizedDir, relative: &str) -> Option<String> {
    let path = member_path(dir.path(), relative)?;
    // `symlink_metadata` never follows the final component, so a FIFO, a device
    // or a directory is refused by type and a link by file type, with no open.
    let meta = fs::symlink_metadata(&path).ok()?;
    if !meta.is_file() {
        return None;
    }
    // The cap is enforced on the length the filesystem already reported, so an
    // oversized member is dropped before a single byte is buffered.
    if meta.len() > PACKAGE_BOUNDS.3 {
        return None;
    }
    let mut buffer = Vec::new();
    fs::File::open(&path)
        .ok()?
        .take(PACKAGE_BOUNDS.3 + 1)
        .read_to_end(&mut buffer)
        .ok()?;
    String::from_utf8(buffer).ok()
}

/// A member path confined to the package root: no `..`, no absolute segment and
/// no symlinked component, so neither the member nor an intermediate directory
/// can walk out of the package.
pub fn member_path(root: &Path, relative: &str) -> Option<PathBuf> {
    if relative.is_empty() || relative.contains('\0') {
        return None;
    }
    let root = fs::canonicalize(root).ok()?;
    let mut cursor = root.clone();
    let mut parts = 0;
    for part in relative.split(['/', '\\']) {
        match part {
            "" | "." => continue,
            ".." => return None,
            part => {
                cursor.push(part);
                parts += 1;
            }
        }
        if fs::symlink_metadata(&cursor).ok()?.file_type().is_symlink() {
            return None;
        }
    }
    if parts == 0 {
        return None;
    }
    let resolved = fs::canonicalize(&cursor).ok()?;
    resolved.starts_with(&root).then_some(resolved)
}

/// One top-level frontmatter field as `(1-based line, value)`, reusing the same
/// quote and comment stripping as `manifest::parse_frontmatter_name`. Indented
/// (nested) blocks are never mistaken for the field.
pub fn frontmatter_field(text: &str, key: &str) -> Option<(usize, String)> {
    let rest = text.trim_start().strip_prefix("---")?.split_once('\n')?.1;
    let prefix = format!("{key}:");
    for (index, line) in rest.lines().enumerate() {
        if line.trim() == "---" {
            return None;
        }
        if line.starts_with(char::is_whitespace) {
            continue;
        }
        if let Some(value) = line.strip_prefix(&prefix) {
            let head = value
                .split_once(" #")
                .or_else(|| value.split_once("\t#"))
                .map_or(value, |(before, _)| before);
            let clean: String = head
                .trim()
                .trim_matches(|c| c == '"' || c == '\'')
                .trim()
                .into();
            let line = index + 2;
            return Some((line, clean)).filter(|(_, value)| !value.is_empty());
        }
    }
    None
}

/// 1-based line where the frontmatter block starts, or 1 when there is none.
pub fn frontmatter_start(text: &str) -> usize {
    text.lines().take_while(|l| l.trim().is_empty()).count() + 1
}

/// Every inline markdown link target that names a package-relative member, with
/// its 1-based line. Anchors, schemes and absolute paths are skipped.
pub fn link_targets(text: &str, link: &Regex) -> Vec<(usize, String)> {
    let mut out: Vec<(usize, String)> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        for captured in link.captures_iter(line) {
            let target = captured.get(1).map_or("", |m| m.as_str());
            let clean = target.split('#').next().unwrap_or_default();
            let local =
                !clean.is_empty() && !clean.starts_with(['/', '~', '#']) && !clean.contains(':');
            if local {
                out.push((index + 1, clean.into()));
            }
        }
    }
    out
}

/// Every http literal with its 1-based line, plus whether the per-package cap
/// in [`super::AUDIT_BOUNDS`].2 stopped the scan early, so the report can say
/// the result is partial.
pub fn http_hits(text: &str, url: &Regex) -> (Vec<(usize, String)>, bool) {
    let mut out: Vec<(usize, String)> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        for hit in url.find_iter(line) {
            if out.len() >= super::AUDIT_BOUNDS.2 {
                return (out, true);
            }
            out.push((index + 1, hit.as_str().into()));
        }
    }
    (out, false)
}
