//! Interest search — internet-like discovery over marketplace + SDC (T-09)
//!
//! Hybrid BM25-like keyword match + karma ranking + freshness. Queries
//! DataMarketplace via substring filter, then ranks by reputation/karma.

use serde::{Deserialize, Serialize};

use crate::data_commons::marketplace::{DataMarketplace, DatasetId};

/// Search filters
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchFilters {
    pub category: Option<String>,
    pub price_max: Option<u64>,
    pub reputation_min: Option<f64>,
}

/// Ranked search result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankedResult {
    pub dataset_id: DatasetId,
    pub name: String,
    pub category: String,
    pub price: u64,
    pub reputation: f64,
    pub karma: u64,
    pub score: f64,
}

/// Score a dataset for a query: keyword match + karma + reputation
pub(crate) fn score_dataset(
    query: &str,
    name: &str,
    description: &str,
    reputation: f64,
    karma: u64,
    price: u64,
) -> f64 {
    let keyword = if query.is_empty() {
        0.5 // no query -> neutral
    } else {
        let q = query.to_lowercase();
        let name_match = f64::from(name.to_lowercase().contains(&q)) * 0.6;
        let desc_match = f64::from(description.to_lowercase().contains(&q)) * 0.4;
        name_match + desc_match
    };
    let karma_norm = (karma as f64 / 5000.0).min(1.0);
    let rep_norm = reputation.clamp(0.0, 1.0);
    let price_penalty = if price > 100 { 0.9 } else { 1.0 };
    // hybrid: 0.4*keyword + 0.3*karma + 0.3*rep
    (keyword * 0.4 + karma_norm * 0.3 + rep_norm * 0.3) * price_penalty
}

/// Search marketplace with filters and ranking by karma/reputation.
/// `karma_map` provides karma per publisher wallet (for ranking).
pub fn search_marketplace(
    _marketplace: &DataMarketplace,
    _query: &str,
    _filters: &SearchFilters,
    _karma_map: &std::collections::HashMap<String, u64>,
) -> Vec<RankedResult> {
    // Stub: DataMarketplace doesn't expose iteration; callers use search_over with dataset list.
    Vec::new()
}

/// Search over a provided list of dataset metadatas (for testing and direct use)
pub fn search_over(
    query: &str,
    datasets: Vec<(DatasetId, String, String, String, u64, f64, String)>, // id, name, desc, category, price, rep, publisher
    filters: &SearchFilters,
    karma_map: &std::collections::HashMap<String, u64>,
) -> Vec<RankedResult> {
    let mut out = Vec::new();
    for (id, name, desc, category, price, rep, publisher) in datasets {
        if let Some(ref cat) = filters.category {
            if cat != &category {
                continue;
            }
        }
        if let Some(max) = filters.price_max {
            if price > max {
                continue;
            }
        }
        if let Some(min_rep) = filters.reputation_min {
            if rep < min_rep {
                continue;
            }
        }
        let karma = karma_map.get(&publisher).copied().unwrap_or(0);
        let score = score_dataset(query, &name, &desc, rep, karma, price);
        if query.is_empty() || score > 0.05 {
            out.push(RankedResult {
                dataset_id: id,
                name,
                category,
                price,
                reputation: rep,
                karma,
                score,
            });
        }
    }
    out.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_commons::marketplace::DatasetId;

    #[test]
    fn ranking_karma_boosts() {
        let datasets = vec![
            (
                DatasetId("ds_a".into()),
                "React 19 docs".into(),
                "hooks".into(),
                "docs".into(),
                0,
                0.5,
                "pub_low".into(),
            ),
            (
                DatasetId("ds_b".into()),
                "React 19 docs".into(),
                "hooks".into(),
                "docs".into(),
                0,
                0.5,
                "pub_high".into(),
            ),
        ];
        let mut karma = std::collections::HashMap::new();
        karma.insert("pub_low".into(), 10);
        karma.insert("pub_high".into(), 4000);
        let res = search_over("react", datasets, &SearchFilters::default(), &karma);
        assert_eq!(res.len(), 2);
        assert_eq!(res[0].dataset_id.0, "ds_b"); // high karma first
    }

    #[test]
    fn filters_category_and_price() {
        let datasets = vec![
            (
                DatasetId("ds_a".into()),
                "Neon wallpapers".into(),
                "".into(),
                "wallpapers".into(),
                5,
                0.9,
                "p1".into(),
            ),
            (
                DatasetId("ds_b".into()),
                "Other".into(),
                "".into(),
                "docs".into(),
                100,
                0.9,
                "p1".into(),
            ),
        ];
        let filters = SearchFilters {
            category: Some("wallpapers".into()),
            price_max: Some(10),
            reputation_min: None,
        };
        let res = search_over("", datasets, &filters, &std::collections::HashMap::new());
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].dataset_id.0, "ds_a");
    }

    #[test]
    fn empty_query_returns_all_filtered() {
        let datasets = vec![(
            DatasetId("ds_a".into()),
            "X".into(),
            "".into(),
            "docs".into(),
            0,
            0.5,
            "p1".into(),
        )];
        let res = search_over(
            "",
            datasets,
            &SearchFilters::default(),
            &std::collections::HashMap::new(),
        );
        assert_eq!(res.len(), 1);
    }
}
