//! # Data Marketplace API for Data Commons
//!
//! Grounded in requirements for secure decentralized data marketplace between Xavier nodes.
//! Allows nodes to list datasets, query them with valid payment, and revoke access.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// Unique identifier for a dataset
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DatasetId(pub String);

/// Metadata and contents describing a dataset listed on the marketplace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetMetadata {
    /// Name of the dataset
    pub name: String,
    /// Detailed description
    pub description: String,
    /// Category of the dataset (e.g. Telemetry, Logs, Benchmark)
    pub category: String,
    /// Minimum price required in $SWAL to query/access the dataset
    pub price: u64,
    /// Publisher wallet address
    pub publisher: String,
    /// Actual rows/records contained in the dataset
    pub rows: Vec<serde_json::Value>,
    /// Pricing tier of the dataset
    pub tier: crate::data_commons::pricing::PricingTier,
    /// Publisher's reputation score
    pub reputation: f64,
}

/// A single page of filtered results returned from a dataset query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataPage {
    /// ID of the source dataset
    pub dataset_id: DatasetId,
    /// The page index (0-based)
    pub page_number: usize,
    /// Total pages available based on search filter
    pub total_pages: usize,
    /// Records matched on this page
    pub records: Vec<serde_json::Value>,
}

/// Marketplace manager for dataset listings, queries, and revocations
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DataMarketplace {
    /// Active and inactive dataset listings
    datasets: HashMap<DatasetId, (DatasetMetadata, bool)>,
}

impl DataMarketplace {
    /// Creates a new instance of the Data Marketplace
    pub fn new() -> Self {
        Self {
            datasets: HashMap::new(),
        }
    }

    /// Lists a new dataset in the marketplace and returns its unique ID.
    ///
    /// # Arguments
    ///
    /// * `metadata` - The metadata and rows of the dataset to list.
    pub fn list_dataset(&mut self, mut metadata: DatasetMetadata) -> DatasetId {
        // Wire pricing: calculate the price dynamically using size, tier, and reputation
        metadata.price = crate::data_commons::pricing::calculate_price(
            metadata.rows.len() as u64,
            metadata.tier,
            metadata.reputation,
        )
        .0;

        let mut hasher = Sha256::new();
        hasher.update(metadata.name.as_bytes());
        hasher.update(metadata.publisher.as_bytes());
        hasher.update(metadata.rows.len().to_be_bytes());

        let hash = crate::crypto::hex_encode(hasher.finalize());
        let id = DatasetId(format!("ds_{}", &hash[0..16]));

        // Insert dataset as active (true)
        self.datasets.insert(id.clone(), (metadata, true));
        id
    }

    /// Queries a listed dataset if the query is valid and payment is sufficient.
    /// Returns matching records wrapped in a `DataPage`.
    ///
    /// # Arguments
    ///
    /// * `id` - The ID of the dataset to query.
    /// * `query` - A substring filter applied to the records.
    /// * `payment` - The amount of $SWAL tokens offered.
    pub fn query_dataset(
        &self,
        id: &DatasetId,
        query: &str,
        payment: u64,
    ) -> Result<DataPage, String> {
        let (metadata, active) = self
            .datasets
            .get(id)
            .ok_or_else(|| "Dataset not found".to_string())?;

        if !active {
            return Err("Dataset has been revoked".to_string());
        }

        if payment < metadata.price {
            return Err(format!(
                "Insufficient payment: required {}, provided {}",
                metadata.price, payment
            ));
        }

        // Apply simple substring filter on the serialized representation of each row
        let matched: Vec<serde_json::Value> = metadata
            .rows
            .iter()
            .filter(|row| {
                if query.is_empty() {
                    true
                } else {
                    let row_str = serde_json::to_string(row)
                        .unwrap_or_default()
                        .to_lowercase();
                    row_str.contains(&query.to_lowercase())
                }
            })
            .cloned()
            .collect();

        Ok(DataPage {
            dataset_id: id.clone(),
            page_number: 0,
            total_pages: 1,
            records: matched,
        })
    }

    /// Revokes an existing dataset, preventing any future queries.
    ///
    /// # Arguments
    ///
    /// * `id` - The ID of the dataset to revoke.
    pub fn revoke_dataset(&mut self, id: &DatasetId) -> Result<(), String> {
        let entry = self
            .datasets
            .get_mut(id)
            .ok_or_else(|| "Dataset not found".to_string())?;

        if !entry.1 {
            return Err("Dataset is already revoked".to_string());
        }

        entry.1 = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dataset_list_and_query() {
        use crate::data_commons::pricing::PricingTier;
        let mut marketplace = DataMarketplace::new();

        let mut rows = vec![
            serde_json::json!({ "node_id": "xv1-node1", "cpu_usage": 45.2, "status": "active" }),
            serde_json::json!({ "node_id": "xv1-node2", "cpu_usage": 12.8, "status": "idle" }),
            serde_json::json!({ "node_id": "xv1-node3", "cpu_usage": 98.1, "status": "overloaded" }),
        ];
        // Pad to exactly 500 rows to get exactly 50 price under Colaborador tier (500 * 0.1 = 50)
        for _ in 0..497 {
            rows.push(serde_json::json!({ "node_id": "xv1-node-dummy", "cpu_usage": 10.0, "status": "idle" }));
        }

        let metadata = DatasetMetadata {
            name: "Xavier Core Telemetry".to_string(),
            description: "Anonymized network metrics and core logs".to_string(),
            category: "Telemetry".to_string(),
            price: 0, // Calculated dynamically
            publisher: "xv1_publisher_wallet_address_xyz_1234567890abcdef".to_string(),
            rows,
            tier: PricingTier::Colaborador,
            reputation: 0.0,
        };

        let id = marketplace.list_dataset(metadata);
        assert!(id.0.starts_with("ds_"));

        // Sufficient payment query
        let query_res = marketplace.query_dataset(&id, "overloaded", 50);
        assert!(query_res.is_ok());
        let page = query_res.unwrap();
        assert_eq!(page.records.len(), 1);
        assert_eq!(page.records[0]["node_id"], "xv1-node3");

        // Query with empty query (returns all)
        let query_all = marketplace.query_dataset(&id, "", 100);
        assert!(query_all.is_ok());
        assert_eq!(query_all.unwrap().records.len(), 500);

        // Insufficient payment query
        let failed_query = marketplace.query_dataset(&id, "", 40);
        assert!(failed_query.is_err());
        assert!(failed_query.unwrap_err().contains("Insufficient payment"));
    }

    #[test]
    fn dataset_revoked_after_query() {
        use crate::data_commons::pricing::PricingTier;
        let mut marketplace = DataMarketplace::new();

        let mut rows = vec![serde_json::json!({ "rtt_ms": 12, "bandwidth_mbps": 450 })];
        // Pad to exactly 100 rows to get exactly 10 price under Colaborador tier (100 * 0.1 = 10)
        for _ in 0..99 {
            rows.push(serde_json::json!({ "rtt_ms": 10, "bandwidth_mbps": 100 }));
        }

        let metadata = DatasetMetadata {
            name: "Xavier Network Benchmarks".to_string(),
            description: "Latency and throughput stats".to_string(),
            category: "Benchmark".to_string(),
            price: 0, // Calculated dynamically
            publisher: "xv1_another_publisher_wallet_address_xyz_123456789".to_string(),
            rows,
            tier: PricingTier::Colaborador,
            reputation: 0.0,
        };

        let id = marketplace.list_dataset(metadata);

        // First query works fine
        let query_res = marketplace.query_dataset(&id, "", 10);
        assert!(query_res.is_ok());

        // Revoke the dataset
        let revoke_res = marketplace.revoke_dataset(&id);
        assert!(revoke_res.is_ok());

        // Querying after revocation must fail
        let post_revoke_query = marketplace.query_dataset(&id, "", 10);
        assert!(post_revoke_query.is_err());
        assert_eq!(post_revoke_query.unwrap_err(), "Dataset has been revoked");

        // Re-revoking must fail
        let double_revoke = marketplace.revoke_dataset(&id);
        assert!(double_revoke.is_err());
    }
}
