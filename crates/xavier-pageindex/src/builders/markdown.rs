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
    start: u32,
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

fn to_tree_node(raw: &[Raw], idx: usize, end: u32, pages: &[Page]) -> TreeNode {
    let r = &raw[idx];
    let children = to_nodes(raw, &r.children, end, pages);
    let chars: usize = pages[(r.start - 1) as usize..end as usize]
        .iter()
        .map(|p| p.text.chars().count())
        .sum();
    TreeNode {
        node_id: String::new(),
        title: r.title.clone(),
        start_page: r.start,
        end_page: end,
        summary: None,
        token_estimate: chars.div_ceil(4) as u32,
        children,
    }
}

/// Each sibling ends where the next begins; the last one ends at `parent_end`.
fn to_nodes(raw: &[Raw], ids: &[usize], parent_end: u32, pages: &[Page]) -> Vec<TreeNode> {
    ids.iter()
        .enumerate()
        .map(|(i, &id)| {
            let end = ids
                .get(i + 1)
                .map_or(parent_end, |&next| raw[next].start - 1);
            to_tree_node(raw, id, end, pages)
        })
        .collect()
}

/// Build a heading tree plus virtual pages of `lines_per_page` lines each.
///
/// Sibling page ranges must be disjoint, so a heading that lands on a page
/// already claimed by its previous sibling's subtree is absorbed into that
/// sibling instead of becoming a node. `doc_id` is left empty for the caller.
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
    let page_of = |line: usize| (line / per_page) as u32 + 1;

    let headings = scan_headings(text);
    let mut raw: Vec<Raw> = Vec::new();
    let mut roots: Vec<usize> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut last_start = 0u32;

    // Text before the first heading becomes a root of its own, when it has
    // its own page(s) to occupy.
    if let Some(first) = headings.first() {
        let has_preamble = lines[..first.line.min(lines.len())]
            .iter()
            .any(|l| !l.trim().is_empty());
        if has_preamble && page_of(first.line) > 1 {
            raw.push(Raw {
                title: "Preamble".to_string(),
                level: 0,
                start: 1,
                children: Vec::new(),
            });
            roots.push(0);
            last_start = 1;
        }
    }

    for h in &headings {
        let page = page_of(h.line);
        while stack.last().is_some_and(|&s| raw[s].level >= h.level) {
            stack.pop();
        }
        let siblings = match stack.last() {
            Some(&p) => &raw[p].children,
            None => &roots,
        };
        if !siblings.is_empty() && page <= last_start {
            continue;
        }
        let id = raw.len();
        raw.push(Raw {
            title: h.title.clone(),
            level: h.level,
            start: page,
            children: Vec::new(),
        });
        match stack.last() {
            Some(&p) => raw[p].children.push(id),
            None => roots.push(id),
        }
        stack.push(id);
        last_start = page;
    }

    // Cover leading pages that hold no preamble node.
    if let Some(&first) = roots.first() {
        raw[first].start = 1;
    }

    let mut tree = DocumentTree {
        doc_id: String::new(),
        roots: to_nodes(&raw, &roots, page_count as u32, &pages),
    };
    tree.assign_node_ids();
    (tree, pages)
}
