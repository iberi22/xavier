//! Markdown builder (headings to tree).

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::model::{DocumentTree, Page, TreeNode};

/// One heading found in the source.
struct Heading {
    level: u8,
    title: String,
    line: usize,
}

/// Arena node used while nesting; converted to `TreeNode` at the end.
struct Raw {
    title: String,
    level: u8,
    /// First line (0-based) of the section.
    line: usize,
    /// Exclusive end line, including nested sections.
    end_line: usize,
    children: Vec<usize>,
}

fn level_num(l: HeadingLevel) -> u8 {
    match l {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// ATX and setext headings outside code blocks, with 0-based line numbers.
fn scan_headings(text: &str) -> Vec<Heading> {
    let mut line_starts = vec![0usize];
    line_starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
    let line_of = |off: usize| line_starts.partition_point(|&s| s <= off) - 1;

    let mut out = Vec::new();
    let mut current: Option<Heading> = None;
    for (ev, range) in Parser::new_ext(text, Options::empty()).into_offset_iter() {
        match ev {
            Event::Start(Tag::Heading { level, .. }) => {
                current = Some(Heading {
                    level: level_num(level),
                    title: String::new(),
                    line: line_of(range.start),
                });
            }
            Event::Text(t) | Event::Code(t) => {
                if let Some(h) = current.as_mut() {
                    h.title.push_str(&t);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(h) = current.as_mut() {
                    h.title.push(' ');
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(mut h) = current.take() {
                    h.title = h.title.trim().to_string();
                    if h.title.is_empty() {
                        h.title = "Untitled".to_string();
                    }
                    out.push(h);
                }
            }
            _ => {}
        }
    }
    out
}

struct Layout<'a> {
    lines: &'a [&'a str],
    per_page: usize,
}

impl Layout<'_> {
    fn page_of(&self, line: usize) -> u32 {
        (line / self.per_page) as u32 + 1
    }

    /// Node over its own line range; sections share boundary pages.
    fn to_node(&self, raw: &[Raw], idx: usize) -> TreeNode {
        let r = &raw[idx];
        let chars: usize = self.lines[r.line..r.end_line]
            .iter()
            .map(|l| l.chars().count() + 1)
            .sum();
        TreeNode {
            node_id: String::new(),
            title: r.title.clone(),
            start_page: self.page_of(r.line),
            end_page: self.page_of(r.end_line - 1),
            summary: None,
            token_estimate: chars.div_ceil(4) as u32,
            children: r.children.iter().map(|&c| self.to_node(raw, c)).collect(),
        }
    }
}

/// Build a heading tree plus virtual pages of `lines_per_page` lines each.
///
/// Headings may share a virtual page: a node spans the pages of its first to
/// last line, so consecutive siblings can share one boundary page. `doc_id` is
/// left empty for the caller.
pub fn build(text: &str, lines_per_page: usize) -> (DocumentTree, Vec<Page>) {
    let per_page = lines_per_page.max(1);
    let lines: Vec<&str> = text.lines().collect();
    let page_count = lines.len().div_ceil(per_page).max(1);
    let pages: Vec<Page> = (0..page_count)
        .map(|i| Page {
            doc_id: String::new(),
            page_no: i as u32 + 1,
            text: lines[(i * per_page).min(lines.len())..((i + 1) * per_page).min(lines.len())]
                .join("\n"),
        })
        .collect();

    let headings = scan_headings(text);
    let mut raw: Vec<Raw> = Vec::new();
    let mut roots: Vec<usize> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();

    // Text before the first heading becomes a root of its own.
    let mut has_preamble = false;
    if let Some(first) = headings.first() {
        has_preamble = lines[..first.line.min(lines.len())]
            .iter()
            .any(|l| !l.trim().is_empty());
        if has_preamble {
            raw.push(Raw {
                title: "Preamble".to_string(),
                level: 0,
                line: 0,
                end_line: first.line,
                children: Vec::new(),
            });
            roots.push(0);
        }
    }

    for h in &headings {
        while let Some(&s) = stack.last() {
            if raw[s].level < h.level {
                break;
            }
            raw[s].end_line = h.line;
            stack.pop();
        }
        let id = raw.len();
        raw.push(Raw {
            title: h.title.clone(),
            level: h.level,
            line: h.line,
            end_line: lines.len(),
            children: Vec::new(),
        });
        match stack.last() {
            Some(&p) => raw[p].children.push(id),
            None => roots.push(id),
        }
        stack.push(id);
    }

    let layout = Layout {
        lines: &lines,
        per_page,
    };
    let mut nodes: Vec<TreeNode> = roots.iter().map(|&r| layout.to_node(&raw, r)).collect();
    // Cover leading pages that hold no preamble node.
    if !has_preamble {
        if let Some(first) = nodes.first_mut() {
            first.start_page = 1;
        }
    }
    let mut tree = DocumentTree {
        doc_id: String::new(),
        roots: nodes,
    };
    tree.assign_node_ids();
    (tree, pages)
}
