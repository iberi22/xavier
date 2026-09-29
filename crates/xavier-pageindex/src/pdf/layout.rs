//! Layout-based heading detection (font size / weight statistics).
//!
//! Two layers: `extract_lines` (pdfium, char-level) produces `LineRecord`s, and
//! `classify_headings` (pure) maps line statistics to heading levels.
//!
//! Ported idea from PageIndex "flash" (MIT): body size = mode of char sizes,
//! larger size clusters become levels, bold short lines at body size are the
//! lowest level, repeated margin lines are headers/footers. Intentionally left
//! out: multi-column splitting, numbering/keyword tables, TOC-page cliques,
//! script/glyph normalisation and the char-level content-stream parser.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::error::PageIndexError;
use crate::pdf::pdfium_loader;

/// One visual text line with its dominant style.
#[derive(Debug, Clone, PartialEq)]
pub struct LineRecord {
    /// 1-based page number.
    pub page: u32,
    /// Distance of the line baseline from the page top, in points.
    pub top: f32,
    /// Page height in points.
    pub page_height: f32,
    pub text: String,
    /// Dominant font size in points.
    pub font_size: f32,
    pub bold: bool,
    /// Non-space chars in the line (weight for body statistics).
    pub chars: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HeadingCandidate {
    pub title: String,
    /// 1-based page number.
    pub page: u32,
    /// 1 = top level.
    pub level: u32,
}

/// Tunables of the pure classifier.
#[derive(Debug, Clone)]
pub struct LayoutParams {
    /// Size ratio over body text for a line to be a size-based heading.
    pub min_size_ratio: f32,
    /// Sizes closer than this (points) fall in the same cluster.
    pub cluster_tolerance: f32,
    pub max_levels: u32,
    pub max_title_chars: usize,
    pub max_bold_title_chars: usize,
    /// Fraction of page height treated as header/footer band.
    pub margin_band: f32,
}

impl Default for LayoutParams {
    fn default() -> Self {
        Self {
            min_size_ratio: 1.15,
            cluster_tolerance: 0.75,
            max_levels: 6,
            max_title_chars: 120,
            max_bold_title_chars: 80,
            margin_band: 0.08,
        }
    }
}

fn norm_key(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_ascii_digit() { '#' } else { c })
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn in_margin(l: &LineRecord, band: f32) -> bool {
    if l.page_height <= 0.0 {
        return false;
    }
    let frac = l.top / l.page_height;
    frac <= band || frac >= 1.0 - band
}

/// Indexes of margin lines repeated across pages (running headers/footers, page numbers).
fn repeated_margin_lines(lines: &[LineRecord], band: f32) -> HashSet<usize> {
    let pages: HashSet<u32> = lines.iter().map(|l| l.page).collect();
    let mut per_key: HashMap<String, HashSet<u32>> = HashMap::new();
    for l in lines.iter().filter(|l| in_margin(l, band)) {
        per_key.entry(norm_key(&l.text)).or_default().insert(l.page);
    }
    lines
        .iter()
        .enumerate()
        .filter(|(_, l)| in_margin(l, band))
        .filter(|(_, l)| {
            let n = per_key.get(&norm_key(&l.text)).map_or(0, HashSet::len);
            n >= 2 && n * 2 >= pages.len()
        })
        .map(|(i, _)| i)
        .collect()
}

/// Mode of char-weighted sizes, rounded to 0.5pt; also whether body text is mostly bold.
fn body_stats(lines: &[&LineRecord]) -> Option<(f32, bool)> {
    let mut by_size: BTreeMap<i32, (u64, u64)> = BTreeMap::new();
    for l in lines {
        let e = by_size
            .entry((l.font_size * 2.0).round() as i32)
            .or_default();
        e.0 += u64::from(l.chars);
        if l.bold {
            e.1 += u64::from(l.chars);
        }
    }
    by_size
        .into_iter()
        .max_by_key(|(_, (n, _))| *n)
        .map(|(k, (n, b))| (k as f32 / 2.0, n > 0 && b * 2 >= n))
}

fn plausible_title(t: &str, max_chars: usize) -> bool {
    let n = t.chars().count();
    n >= 2 && n <= max_chars && t.chars().any(char::is_alphanumeric) && !t.ends_with([',', ';'])
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Size,
    Bold,
}

struct Cand {
    idx: usize,
    kind: Kind,
}

/// Pure classifier: line style statistics -> heading candidates in reading order.
pub fn classify_headings(lines: &[LineRecord], params: &LayoutParams) -> Vec<HeadingCandidate> {
    let drop = repeated_margin_lines(lines, params.margin_band);
    let kept: Vec<usize> = (0..lines.len())
        .filter(|i| !drop.contains(i) && !lines[*i].text.trim().is_empty())
        .collect();
    let refs: Vec<&LineRecord> = kept.iter().map(|&i| &lines[i]).collect();
    let Some((body, body_bold)) = body_stats(&refs) else {
        return Vec::new();
    };

    // Per-line candidacy, in kept order (None = body text).
    let mut cands: Vec<Option<Cand>> = Vec::with_capacity(kept.len());
    for &i in &kept {
        let l = &lines[i];
        let t = l.text.trim();
        let kind = if l.font_size >= body * params.min_size_ratio
            && plausible_title(t, params.max_title_chars)
        {
            Some(Kind::Size)
        } else if l.bold
            && !body_bold
            && l.font_size >= body * 0.95
            && t.split_whitespace().count() <= 12
            && plausible_title(t, params.max_bold_title_chars)
        {
            Some(Kind::Bold)
        } else {
            None
        };
        cands.push(kind.map(|kind| Cand { idx: i, kind }));
    }

    // Size clusters (descending) -> levels.
    let mut sizes: Vec<f32> = cands
        .iter()
        .flatten()
        .filter(|c| c.kind == Kind::Size)
        .map(|c| lines[c.idx].font_size)
        .collect();
    sizes.sort_by(|a, b| b.total_cmp(a));
    let mut centers: Vec<f32> = Vec::new();
    for s in sizes {
        if centers
            .last()
            .is_none_or(|c| (c - s).abs() > params.cluster_tolerance)
        {
            centers.push(s);
        }
    }
    let level_of = |size: f32| -> u32 {
        let pos = centers
            .iter()
            .position(|c| (c - size).abs() <= params.cluster_tolerance)
            .unwrap_or(centers.len().saturating_sub(1));
        (pos as u32 + 1).min(params.max_levels)
    };
    let bold_level = (centers.len() as u32 + 1).min(params.max_levels);

    // Emit, joining wrapped multi-line titles (same page/style, tight vertical gap).
    let mut out: Vec<HeadingCandidate> = Vec::new();
    let mut prev: Option<(usize, Kind)> = None;
    for c in cands.iter() {
        let Some(c) = c else {
            prev = None;
            continue;
        };
        let l = &lines[c.idx];
        let text = l.text.trim();
        let joins = prev.is_some_and(|(pi, pk)| {
            let p = &lines[pi];
            pk == c.kind
                && p.page == l.page
                && (p.font_size - l.font_size).abs() <= 0.25
                && (l.top - p.top).abs() <= l.font_size * 1.6
        });
        if joins && !out.is_empty() {
            let last = out.last_mut().expect("checked non-empty");
            last.title.push(' ');
            last.title.push_str(text);
        } else {
            let level = match c.kind {
                Kind::Size => level_of(l.font_size),
                Kind::Bold => bold_level,
            };
            out.push(HeadingCandidate {
                title: text.to_string(),
                page: l.page,
                level,
            });
        }
        prev = Some((c.idx, c.kind));
    }
    out
}

fn is_bold_name(name: &str) -> bool {
    let n = name.to_lowercase();
    ["bold", "black", "heavy", "semibold", "demi"]
        .iter()
        .any(|k| n.contains(k))
}

/// Extracts styled lines from every page via pdfium (char level).
pub fn extract_lines(bytes: &[u8]) -> Result<Vec<LineRecord>, PageIndexError> {
    use pdfium_render::prelude::PdfiumError;

    let pdfium = pdfium_loader::pdfium()?;
    let doc = pdfium
        .load_pdf_from_byte_slice(bytes, None)
        .map_err(|e| match e {
            PdfiumError::PdfiumLibraryInternalError(
                pdfium_render::prelude::PdfiumInternalError::PasswordError,
            ) => PageIndexError::Encrypted,
            other => PageIndexError::Build(format!("pdfium could not open document: {other}")),
        })?;

    struct Acc {
        text: String,
        top: f32,
        sizes: BTreeMap<i32, u32>,
        bold: u32,
        chars: u32,
    }
    let mut lines = Vec::new();
    for (pi, page) in doc.pages().iter().enumerate() {
        let height = page.height().value;
        let Ok(text) = page.text() else { continue };
        let mut cur: Option<Acc> = None;
        let flush = |cur: &mut Option<Acc>, lines: &mut Vec<LineRecord>| {
            if let Some(a) = cur.take() {
                if a.chars > 0 {
                    let size_key = a
                        .sizes
                        .iter()
                        .max_by_key(|(_, n)| **n)
                        .map_or(0, |(k, _)| *k);
                    lines.push(LineRecord {
                        page: pi as u32 + 1,
                        top: a.top,
                        page_height: height,
                        text: a.text.split_whitespace().collect::<Vec<_>>().join(" "),
                        font_size: size_key as f32 / 10.0,
                        bold: a.bold * 2 >= a.chars,
                        chars: a.chars,
                    });
                }
            }
        };
        for ch in text.chars().iter() {
            let Some(c) = ch.unicode_char() else { continue };
            if c == '\r' || c == '\n' {
                flush(&mut cur, &mut lines);
                continue;
            }
            if c.is_control() {
                continue;
            }
            let y = ch.origin_y().map(|p| p.value).unwrap_or(0.0);
            let size = ch.scaled_font_size().value;
            let top = height - y;
            // A jump in baseline larger than half the font size starts a new line.
            if cur
                .as_ref()
                .is_some_and(|a| (a.top - top).abs() > size.max(1.0) * 0.5)
            {
                flush(&mut cur, &mut lines);
            }
            let a = cur.get_or_insert_with(|| Acc {
                text: String::new(),
                top,
                sizes: BTreeMap::new(),
                bold: 0,
                chars: 0,
            });
            a.text.push(c);
            if !c.is_whitespace() {
                *a.sizes.entry((size * 10.0).round() as i32).or_default() += 1;
                let bold = ch.font_weight().is_some_and(|w| {
                    matches!(
                        w,
                        pdfium_render::prelude::PdfFontWeight::Weight600
                            | pdfium_render::prelude::PdfFontWeight::Weight700Bold
                            | pdfium_render::prelude::PdfFontWeight::Weight800
                            | pdfium_render::prelude::PdfFontWeight::Weight900
                    ) || matches!(w, pdfium_render::prelude::PdfFontWeight::Custom(n) if n >= 600)
                }) || ch.font_is_bold_reenforced()
                    || is_bold_name(&ch.font_name());
                a.bold += u32::from(bold);
                a.chars += 1;
            }
        }
        flush(&mut cur, &mut lines);
    }
    Ok(lines)
}

/// Typed variant: pdfium errors are surfaced (`PdfiumUnavailable`, `Encrypted`, ...).
pub fn try_detect_headings(bytes: &[u8]) -> Result<Vec<HeadingCandidate>, PageIndexError> {
    let lines = extract_lines(bytes)?;
    Ok(classify_headings(&lines, &LayoutParams::default()))
}

/// Heading candidates, or `None` when pdfium is unavailable, the document cannot be
/// read, or no headings were found (the cascade then tries the next strategy).
pub fn detect_headings(bytes: &[u8]) -> Option<Vec<HeadingCandidate>> {
    try_detect_headings(bytes).ok().filter(|h| !h.is_empty())
}
