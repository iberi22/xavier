//! MKT-14 hybrid listing ranking.
use std::collections::{HashMap, HashSet};

pub const RRF_K: u32 = 60;

#[derive(Debug, Clone)]
pub struct Candidate {
    pub listing_id: String,
    pub title: String,
    pub vector_score: f32,
    pub karma: f32,
}

#[derive(Debug, Clone)]
pub struct Weights {
    pub text: f32,
    pub vector: f32,
    pub karma: f32,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            text: 1.0,
            vector: 1.0,
            karma: 0.5,
        }
    }
}

pub fn text_score(q: &str, t: &str) -> f32 {
    let q_trim = q.trim();
    if q_trim.is_empty() {
        return 0.0;
    }
    let q_tokens: HashSet<&str> = q_trim
        .to_ascii_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect();
    if q_tokens.is_empty() {
        return 0.0;
    }
    let t_lower = t.to_ascii_lowercase();
    let t_tokens: HashSet<&str> = t_lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect();
    let matched = q_tokens.iter().filter(|&tk| t_tokens.contains(tk)).count();
    matched as f32 / q_tokens.len() as f32
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ranked {
    pub listing_id: String,
    pub score: f32,
}

pub fn rank(q: &str, candidates: &[Candidate], w: Weights) -> Vec<Ranked> {
    let mut t_list: Vec<_> = candidates
        .iter()
        .map(|c| (c, text_score(q, &c.title)))
        .collect();
    let mut v_list: Vec<_> = candidates.iter().map(|c| (c, c.vector_score)).collect();
    let mut k_list: Vec<_> = candidates.iter().map(|c| (c, c.karma)).collect();

    let sf = |a: &(&Candidate, f32), b: &(&Candidate, f32)| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.listing_id.cmp(&b.0.listing_id))
    };

    t_list.sort_by(sf);
    v_list.sort_by(sf);
    k_list.sort_by(sf);

    let mut fused: HashMap<String, f32> = HashMap::new();
    let lists = [(t_list, w.text), (v_list, w.vector), (k_list, w.karma)];

    for (list, weight) in lists {
        for (i, (c, _)) in list.into_iter().enumerate() {
            let rank = i as u32 + 1;
            *fused.entry(c.listing_id.clone()).or_insert(0.0) += weight / (RRF_K + rank) as f32;
        }
    }

    let mut ranked: Vec<_> = fused
        .into_iter()
        .map(|(id, s)| Ranked {
            listing_id: id,
            score: s,
        })
        .collect();
    ranked.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.listing_id.cmp(&b.listing_id))
    });
    ranked
}

#[cfg(test)]
mod tests {
    use super::*;
    fn c(id: &str, t: &str, v: f32, k: f32) -> Candidate {
        Candidate {
            listing_id: id.into(),
            title: t.into(),
            vector_score: v,
            karma: k,
        }
    }

    #[test]
    fn test_text_score() {
        assert_eq!(text_score("", "h w"), 0.0);
        assert_eq!(text_score("h", "H W"), 1.0);
        assert_eq!(text_score("art", "party"), 0.0);
    }

    #[test]
    fn test_exact_match_above_partial() {
        let r = rank(
            "h w",
            &[c("1", "h w", 0.0, 0.0), c("2", "h t", 0.0, 0.0)],
            Weights::default(),
        );
        assert_eq!(r[0].listing_id, "1");
        assert_eq!(r[1].listing_id, "2");
    }

    #[test]
    fn test_vector_breaks_tie() {
        let r = rank(
            "t",
            &[c("1", "t", 0.5, 0.0), c("2", "t", 0.8, 0.0)],
            Weights::default(),
        );
        assert_eq!(r[0].listing_id, "2");
        assert_eq!(r[1].listing_id, "1");
    }

    #[test]
    fn test_karma_breaks_tie() {
        let r = rank(
            "t",
            &[c("1", "t", 0.5, 10.0), c("2", "t", 0.5, 20.0)],
            Weights::default(),
        );
        assert_eq!(r[0].listing_id, "2");
        assert_eq!(r[1].listing_id, "1");
    }

    #[test]
    fn test_determinism() {
        let r1 = rank(
            "t",
            &[c("1", "t", 0.5, 10.0), c("2", "t", 0.5, 10.0)],
            Weights::default(),
        );
        assert_eq!(r1[0].listing_id, "1");
        assert_eq!(r1[1].listing_id, "2");
    }

    #[test]
    fn test_empty_query() {
        let r = rank(
            "",
            &[c("1", "t", 0.5, 10.0), c("2", "t", 0.5, 10.0)],
            Weights::default(),
        );
        assert_eq!(r[0].listing_id, "1");
        assert_eq!(r[1].listing_id, "2");
    }
}
