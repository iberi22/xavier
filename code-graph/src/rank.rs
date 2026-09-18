//! Structural ranking: edge weights + PageRank + teleport + margin (O6).
//!
//! Ideas rewritten from `redhat-et/ripwire` (`ARCHITECTURE.md` rank/CSR,
//! `COMMANDS.md --rank-by/--adaptive/--map-diff`): edge weight
//! `(mean conf)·√nref` capped at 8, PageRank α=0.85, change teleport β=0.7,
//! head/margin verdict with adaptive floor 5%. No upstream code.
//! See `docs/EXTRACTION-RIPWIRE-GRAPHIFY.md` (R4).
//!
//! Determinism: nodes/edges enter the graph in sorted order and the power
//! iteration walks index order, so identical input yields bit-identical
//! ranks — agents can cache them.

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::Direction;
use std::collections::{HashMap, HashSet};

/// PageRank damping factor (ripwire rank).
pub const PAGERANK_ALPHA: f64 = 0.85;
/// Power-iteration ceiling.
pub const PAGERANK_MAX_ITER: usize = 100;
/// Convergence tolerance on max rank delta.
pub const PAGERANK_TOL: f64 = 1e-6;
/// Edge-weight ceiling (ripwire `cap8`).
pub const EDGE_WEIGHT_CAP: f64 = 8.0;
/// Teleport mass on changed symbols for `--map-diff` reranking.
pub const MAP_DIFF_BETA: f64 = 0.7;
/// Adaptive floor: gaps below this are "a starting point", not an answer.
pub const MARGIN_FLOOR_PCT: f64 = 5.0;

/// Aggregate parallel caller→callee edges: `(mean confidence)·√nref`,
/// capped at [`EDGE_WEIGHT_CAP`]. Rewards corroboration with diminishing
/// returns; hot-loop spam cannot inflate past the cap.
pub fn aggregate_weight(conf_sum: f64, nref: usize) -> f64 {
    if nref == 0 {
        return 0.0;
    }
    ((conf_sum / nref as f64) * (nref as f64).sqrt()).min(EDGE_WEIGHT_CAP)
}

/// Build the call graph from `(caller, callee, confidence)` triples:
/// nodes and edges inserted in sorted order (determinism), parallel edges
/// aggregated via [`aggregate_weight`], self-loops dropped.
pub fn build_call_graph(edges: &[(String, String, f64)]) -> DiGraph<String, f64> {
    let mut nodes: Vec<&String> = Vec::new();
    for (from, to, _) in edges {
        if from == to {
            continue;
        }
        nodes.push(from);
        nodes.push(to);
    }
    nodes.sort();
    nodes.dedup();

    let mut graph = DiGraph::new();
    let mut index: HashMap<&str, NodeIndex> = HashMap::new();
    for name in nodes {
        let ix = graph.add_node(name.clone());
        index.insert(name.as_str(), ix);
    }

    // Aggregate parallel edges in sorted order.
    let mut sorted: Vec<(&String, &String, f64)> =
        edges.iter().map(|(a, b, c)| (a, b, *c)).collect();
    sorted.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    let mut i = 0;
    while i < sorted.len() {
        let (from, to, _) = sorted[i];
        if from == to {
            i += 1;
            continue;
        }
        let mut sum = 0.0;
        let mut n = 0usize;
        while i < sorted.len() && sorted[i].0 == from && sorted[i].1 == to {
            sum += sorted[i].2;
            n += 1;
            i += 1;
        }
        let weight = aggregate_weight(sum, n);
        if let (Some(&a), Some(&b)) = (index.get(from.as_str()), index.get(to.as_str())) {
            graph.add_edge(a, b, weight);
        }
    }
    graph
}

/// Weighted PageRank over `graph` with an explicit teleport distribution
/// (need not sum to 1; it is normalized). Dangling nodes redistribute
/// through teleport. Deterministic for identical input.
pub fn pagerank(
    graph: &DiGraph<String, f64>,
    teleport: &HashMap<String, f64>,
    alpha: f64,
    max_iter: usize,
    tol: f64,
) -> HashMap<String, f64> {
    let order: Vec<NodeIndex> = graph.node_indices().collect();
    let n = order.len();
    let ranks = HashMap::new();
    if n == 0 {
        return ranks;
    }
    // Normalized teleport in index order.
    let tele_sum: f64 = order
        .iter()
        .map(|ix| teleport.get(&graph[*ix]).copied().unwrap_or(0.0))
        .sum();
    let tele: Vec<f64> = order
        .iter()
        .map(|ix| {
            let w = teleport.get(&graph[*ix]).copied().unwrap_or(0.0);
            if tele_sum > 0.0 {
                w / tele_sum
            } else {
                1.0 / n as f64
            }
        })
        .collect();

    // Out-weight sums and incoming adjacency in index order.
    let mut out_sum = vec![0.0f64; n];
    let mut incoming: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
    for (pos, ix) in order.iter().enumerate() {
        let mut sum = 0.0;
        let mut outs: Vec<(usize, f64)> = Vec::new();
        let mut nbrs: Vec<NodeIndex> = graph.neighbors_directed(*ix, Direction::Outgoing).collect();
        nbrs.sort();
        for nb in nbrs {
            let pos_nb = order.iter().position(|x| x == &nb).expect("index");
            let w = graph
                .find_edge(*ix, nb)
                .and_then(|e| graph.edge_weight(e).copied())
                .unwrap_or(1.0);
            sum += w;
            outs.push((pos_nb, w));
        }
        out_sum[pos] = sum;
        for (pos_nb, w) in outs {
            incoming[pos_nb].push((pos, w));
        }
    }

    let mut rank = tele.clone();
    for _ in 0..max_iter {
        // Dangling mass redistributes through teleport (ripwire dangling-via-p).
        let dangling: f64 = rank
            .iter()
            .enumerate()
            .filter(|(i, _)| out_sum[*i] == 0.0)
            .map(|(_, r)| r)
            .sum();
        let mut next = vec![0.0f64; n];
        let mut delta: f64 = 0.0;
        for v in 0..n {
            let mut flow = dangling * tele[v];
            for (u, w) in &incoming[v] {
                if out_sum[*u] > 0.0 {
                    flow += rank[*u] * w / out_sum[*u];
                }
            }
            next[v] = (1.0 - alpha) * tele[v] + alpha * flow;
            delta = delta.max((next[v] - rank[v]).abs());
        }
        rank = next;
        if delta < tol {
            break;
        }
    }
    order
        .iter()
        .enumerate()
        .map(|(i, ix)| (graph[*ix].clone(), rank[i]))
        .collect()
}

/// Uniform teleport over graph nodes.
pub fn uniform_teleport(graph: &DiGraph<String, f64>) -> HashMap<String, f64> {
    graph
        .node_weights()
        .map(|name| (name.clone(), 1.0))
        .collect()
}

/// `--map-diff` teleport: `beta` mass split over changed∩nodes, the rest
/// uniform over all nodes. Empty intersection degrades to uniform (never
/// a zero distribution).
pub fn personalize_for_changed(
    nodes: &[String],
    changed: &HashSet<String>,
    beta: f64,
) -> HashMap<String, f64> {
    let mut sorted: Vec<&String> = nodes.iter().collect();
    sorted.sort();
    let n = sorted.len();
    let mut tele = HashMap::new();
    if n == 0 {
        return tele;
    }
    let hit: Vec<bool> = sorted.iter().map(|name| changed.contains(*name)).collect();
    let hits = hit.iter().filter(|h| **h).count();
    let beta = beta.clamp(0.0, 1.0);
    for (i, name) in sorted.iter().enumerate() {
        let w = (1.0 - beta) / n as f64
            + if hit[i] && hits > 0 {
                beta / hits as f64
            } else {
                0.0
            };
        tele.insert((*name).clone(), w);
    }
    tele
}

/// Head/margin verdict over descending scores: the head ends at the
/// biggest relative drop; `confident` needs `margin_pct` above
/// [`MARGIN_FLOOR_PCT`]. A flat ranking reads as a starting point.
#[derive(Debug, Clone, PartialEq)]
pub struct RankVerdict {
    pub head_len: usize,
    pub margin_pct: f64,
    pub confident: bool,
}

pub fn ranking_margin(scores_desc: &[f64]) -> RankVerdict {
    if scores_desc.len() < 2 {
        return RankVerdict {
            head_len: scores_desc.len(),
            margin_pct: 0.0,
            confident: false,
        };
    }
    let mut best_gap = 0.0;
    let mut head_len = 1;
    for i in 0..scores_desc.len() - 1 {
        let denom = scores_desc[i].abs();
        let gap = if denom > 0.0 {
            (scores_desc[i] - scores_desc[i + 1]) / denom * 100.0
        } else {
            0.0
        };
        if gap > best_gap {
            best_gap = gap;
            head_len = i + 1;
        }
    }
    RankVerdict {
        head_len,
        margin_pct: best_gap,
        confident: best_gap >= MARGIN_FLOOR_PCT,
    }
}

/// Node weights in index order (deterministic export for tests/UI).
pub fn ranks_in_order(
    graph: &DiGraph<String, f64>,
    ranks: &HashMap<String, f64>,
) -> Vec<(String, f64)> {
    graph
        .node_indices()
        .map(|ix| {
            let name = graph[ix].clone();
            let score = ranks.get(&name).copied().unwrap_or(0.0);
            (name, score)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn star_edges() -> Vec<(String, String, f64)> {
        // Leaves call the hub; the hub calls one leaf back.
        let mut edges = vec![
            ("hub".to_string(), "leaf_a".to_string(), 1.0),
            ("leaf_b".to_string(), "hub".to_string(), 1.0),
            ("leaf_c".to_string(), "hub".to_string(), 1.0),
            ("leaf_d".to_string(), "hub".to_string(), 1.0),
        ];
        for _ in 0..3 {
            edges.push(("leaf_b".to_string(), "hub".to_string(), 1.0));
        }
        edges
    }

    #[test]
    fn test_edge_weight_formula() {
        // (mean conf)·√nref with cap8, no self-loops.
        let w = aggregate_weight(1.9, 2);
        assert!((w - 0.95 * 2f64.sqrt()).abs() < 1e-9, "got {w}");
        assert_eq!(aggregate_weight(80.0, 100), EDGE_WEIGHT_CAP);
        let g = build_call_graph(&[
            ("a".to_string(), "b".to_string(), 1.0),
            ("a".to_string(), "b".to_string(), 1.0),
            ("a".to_string(), "b".to_string(), 1.0),
            ("a".to_string(), "a".to_string(), 1.0),
        ]);
        assert_eq!(g.edge_count(), 1, "parallel deduped, self-loop dropped");
        let w = g.edge_weights().next().expect("one aggregated edge");
        assert!((w - 3f64.sqrt()).abs() < 1e-9, "got {w}");
    }

    #[test]
    fn test_pagerank_deterministic_and_fast() {
        // 2000-node chain with hub links: bit-identical reruns, fast.
        let mut edges = Vec::new();
        for i in 0..2000 {
            edges.push((format!("n{i:04}"), format!("n{:04}", (i + 1) % 2000), 1.0));
            if i % 10 == 0 {
                edges.push((format!("n{i:04}"), "hub".to_string(), 0.9));
            }
        }
        let start = std::time::Instant::now();
        let g1 = build_call_graph(&edges);
        let t1 = uniform_teleport(&g1);
        let r1 = pagerank(&g1, &t1, PAGERANK_ALPHA, PAGERANK_MAX_ITER, PAGERANK_TOL);
        let g2 = build_call_graph(&edges);
        let t2 = uniform_teleport(&g2);
        let r2 = pagerank(&g2, &t2, PAGERANK_ALPHA, PAGERANK_MAX_ITER, PAGERANK_TOL);
        assert!(
            start.elapsed().as_secs_f64() < 2.0,
            "2k-node rank must stay interactive"
        );
        assert_eq!(r1, r2, "identical input must rank bit-identical");
        assert!(
            r1["hub"] > r1["n0007"],
            "hub must outrank an ordinary chain node"
        );
    }

    #[test]
    fn test_ranking_margin_gap_vs_flat() {
        let v = ranking_margin(&[1.0, 0.9, 0.5, 0.4]);
        assert_eq!(v.head_len, 2);
        assert!(
            (v.margin_pct - 44.4444).abs() < 0.01,
            "got {}",
            v.margin_pct
        );
        assert!(v.confident);
        let flat = ranking_margin(&[1.0, 0.99, 0.98]);
        assert!(!flat.confident, "flat ranking is a starting point");
        assert!(flat.margin_pct < MARGIN_FLOOR_PCT);
        let single = ranking_margin(&[1.0]);
        assert_eq!(single.head_len, 1);
        assert!(!single.confident);
    }

    #[test]
    fn test_map_diff_teleport_reranks() {
        // Personalizing on a leaf must lift it above its uniform rank.
        let g = build_call_graph(&star_edges());
        let uni = pagerank(
            &g,
            &uniform_teleport(&g),
            PAGERANK_ALPHA,
            PAGERANK_MAX_ITER,
            PAGERANK_TOL,
        );
        let nodes: Vec<String> = {
            let mut v: Vec<String> = uni.keys().cloned().collect();
            v.sort();
            v
        };
        let changed: HashSet<String> = ["leaf_a".to_string()].into_iter().collect();
        let tele = personalize_for_changed(&nodes, &changed, MAP_DIFF_BETA);
        let total: f64 = tele.values().sum();
        assert!((total - 1.0).abs() < 1e-9, "teleport must sum to 1");
        let per = pagerank(&g, &tele, PAGERANK_ALPHA, PAGERANK_MAX_ITER, PAGERANK_TOL);
        assert!(
            per["leaf_a"] > uni["leaf_a"],
            "changed leaf must rise under map-diff teleport"
        );
    }

    #[test]
    fn test_ranks_in_order_is_deterministic() {
        let g = build_call_graph(&star_edges());
        let tele = uniform_teleport(&g);
        let ranks = pagerank(&g, &tele, PAGERANK_ALPHA, PAGERANK_MAX_ITER, PAGERANK_TOL);
        let ordered = ranks_in_order(&g, &ranks);
        let names: Vec<_> = ordered.iter().map(|(n, _)| n.clone()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted, "export follows index (sorted insert) order");
    }
}
