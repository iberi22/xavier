//! Deterministic tree optimizer: merge tiny leaves, split huge ones on page
//! boundaries. Needs no LLM and keeps `DocumentTree::validate` satisfied.

use std::collections::HashMap;

use crate::model::{DocumentTree, Page, TreeNode};

/// Thresholds in the same token unit as `TreeNode::token_estimate`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OptimizeOpts {
    pub min_node_tokens: u32,
    pub max_node_tokens: u32,
}

impl Default for OptimizeOpts {
    fn default() -> Self {
        Self {
            min_node_tokens: 200,
            max_node_tokens: 6000,
        }
    }
}

/// Split oversized leaves, then merge tiny ones, then renumber the ids.
/// Idempotent: a second run leaves the tree unchanged.
pub fn optimize(tree: &mut DocumentTree, pages: &[Page], opts: OptimizeOpts) {
    split_huge(tree, pages, opts.max_node_tokens);
    merge_tiny(tree, opts.min_node_tokens, opts.max_node_tokens);
    tree.assign_node_ids();
}

/// Fold each tiny leaf into its previous sibling when that sibling is also a
/// leaf (a parent's range must keep matching its children), unless the result would
/// exceed `max_tokens`. A leading tiny leaf stays (nothing precedes it).
pub fn merge_tiny(tree: &mut DocumentTree, min_tokens: u32, max_tokens: u32) {
    fn walk(nodes: &mut Vec<TreeNode>, min: u32, max: u32) {
        for n in nodes.iter_mut() {
            walk(&mut n.children, min, max);
        }
        let mut out: Vec<TreeNode> = Vec::with_capacity(nodes.len());
        for n in std::mem::take(nodes) {
            let tiny = n.children.is_empty() && n.token_estimate < min;
            match out.last_mut() {
                Some(prev)
                    if tiny
                        && prev.children.is_empty()
                        && prev.token_estimate.saturating_add(n.token_estimate) <= max =>
                {
                    prev.end_page = prev.end_page.max(n.end_page);
                    prev.token_estimate += n.token_estimate;
                }
                _ => out.push(n),
            }
        }
        *nodes = out;
    }
    walk(&mut tree.roots, min_tokens, max_tokens);
    tree.assign_node_ids();
}

/// Give every childless node above `max_tokens` (and spanning several pages)
/// children that cut its range on page boundaries. Chunks never share pages.
pub fn split_huge(tree: &mut DocumentTree, pages: &[Page], max_tokens: u32) {
    let weight: HashMap<u32, usize> = pages
        .iter()
        .map(|p| (p.page_no, p.text.chars().count().max(1)))
        .collect();
    fn walk(nodes: &mut [TreeNode], weight: &HashMap<u32, usize>, max: u32) {
        for n in nodes {
            if !n.children.is_empty() {
                walk(&mut n.children, weight, max);
            } else if n.token_estimate > max && n.end_page > n.start_page {
                n.children = chunks(n, weight, max);
            }
        }
    }
    walk(&mut tree.roots, &weight, max_tokens);
    tree.assign_node_ids();
}

fn chunks(n: &TreeNode, weight: &HashMap<u32, usize>, max: u32) -> Vec<TreeNode> {
    let w = |p: u32| weight.get(&p).copied().unwrap_or(1);
    let total: usize = (n.start_page..=n.end_page).map(w).sum();
    let tokens_of =
        |chars: usize| ((u64::from(n.token_estimate) * chars as u64).div_ceil(total as u64)) as u32;
    let mut ranges: Vec<(u32, u32, usize)> = Vec::new();
    let (mut start, mut acc) = (n.start_page, 0usize);
    for p in n.start_page..=n.end_page {
        if acc > 0 && tokens_of(acc + w(p)) > max {
            ranges.push((start, p - 1, acc));
            start = p;
            acc = 0;
        }
        acc += w(p);
    }
    ranges.push((start, n.end_page, acc));
    if ranges.len() < 2 {
        return Vec::new();
    }
    ranges
        .into_iter()
        .map(|(s, e, chars)| TreeNode {
            node_id: String::new(),
            title: if s == e {
                format!("{} (page {s})", n.title)
            } else {
                format!("{} (pages {s}-{e})", n.title)
            },
            start_page: s,
            end_page: e,
            summary: None,
            token_estimate: tokens_of(chars),
            children: Vec::new(),
        })
        .collect()
}
