//! Marketplace folders — wallpapers and pack datasets via DataMarketplace (T-08)
//!
//! Reuses `crate::data_commons::marketplace::DataMarketplace` for folder packs.
//! Each file in a folder becomes a row {path, hash, preview, price}. Pack is a
//! DatasetMetadata category "wallpapers" / "pack". Payment via DC (burn SWAL).

use serde::{Deserialize, Serialize};

use crate::data_commons::marketplace::{DataMarketplace, DatasetId, DatasetMetadata};
use crate::data_commons::pricing::PricingTier;

/// Entry for a single file in a folder pack
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderEntry {
    pub path: String,
    pub hash: String,
    pub preview: Option<String>,
    pub price: u64,
}

/// Helper to build a DatasetMetadata for a folder pack
pub fn folder_dataset(
    name: String,
    description: String,
    category: String,
    entries: Vec<FolderEntry>,
    publisher: String,
    tier: PricingTier,
    reputation: f64,
) -> DatasetMetadata {
    let rows: Vec<serde_json::Value> = entries
        .into_iter()
        .map(|e| serde_json::json!({"path": e.path, "hash": e.hash, "preview": e.preview, "price": e.price}))
        .collect();
    DatasetMetadata {
        name,
        description,
        category,
        price: 0, // calculated by DataMarketplace::list_dataset
        publisher,
        rows,
        tier,
        reputation,
    }
}

/// Wrapper that lists a folder pack and returns its DatasetId
pub fn list_folder_pack(marketplace: &mut DataMarketplace, dataset: DatasetMetadata) -> DatasetId {
    marketplace.list_dataset(dataset)
}

/// Query a folder pack with payment, returns matching rows
pub fn query_folder_pack(
    marketplace: &DataMarketplace,
    id: &DatasetId,
    query: &str,
    payment: u64,
) -> Result<Vec<serde_json::Value>, String> {
    let page = marketplace.query_dataset(id, query, payment)?;
    Ok(page.records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_commons::pricing::PricingTier;

    #[test]
    fn folder_pack_roundtrip() {
        let mut mp = DataMarketplace::new();
        let entries = vec![
            FolderEntry {
                path: "wallpapers/neon/a.jpg".into(),
                hash: "abc".into(),
                preview: Some("cid1".into()),
                price: 5,
            },
            FolderEntry {
                path: "wallpapers/neon/b.jpg".into(),
                hash: "def".into(),
                preview: None,
                price: 5,
            },
        ];
        // pad to 100 rows to get price 10 for predictable test
        let mut all_entries = entries;
        for i in 0..98 {
            all_entries.push(FolderEntry {
                path: format!("wallpapers/neon/{}.jpg", i),
                hash: format!("h{}", i),
                preview: None,
                price: 5,
            });
        }
        let ds = folder_dataset(
            "Neon Pack".into(),
            "wallpapers".into(),
            "wallpapers".into(),
            all_entries,
            "xv1_publisher_test_1234567890abcdef1234567890abcd".into(),
            PricingTier::Colaborador,
            0.0,
        );
        let id = list_folder_pack(&mut mp, ds);
        // price for 100 rows Colaborador ~10, query with 10 succeeds
        let rows = query_folder_pack(&mp, &id, "neon", 10).unwrap();
        assert!(!rows.is_empty());
        // insufficient payment fails
        assert!(query_folder_pack(&mp, &id, "", 1).is_err());
    }

    #[test]
    fn revoke_blocks_query() {
        let mut mp = DataMarketplace::new();
        let entries = (0..100)
            .map(|i| FolderEntry {
                path: format!("f{}.jpg", i),
                hash: format!("h{}", i),
                preview: None,
                price: 1,
            })
            .collect();
        let ds = folder_dataset(
            "Pack".into(),
            "desc".into(),
            "wallpapers".into(),
            entries,
            "xv1_pub_1234567890abcdef1234567890abcdef1234".into(),
            PricingTier::Colaborador,
            0.0,
        );
        let id = list_folder_pack(&mut mp, ds);
        assert!(query_folder_pack(&mp, &id, "", 10).is_ok());
        mp.revoke_dataset(&id).unwrap();
        assert!(query_folder_pack(&mp, &id, "", 10).is_err());
    }
}
