//! # Mesh & Data Commons Integration Bridge
//!
//! Provides the bidirectional synchronization of reputation and karma,
//! dataset announcement and consumption on top of the Data Commons Marketplace,
//! and storage rent economy mechanisms for Xavier Mesh.

use crate::data_commons::marketplace::{DataMarketplace, DataPage, DatasetId, DatasetMetadata};
use crate::data_commons::pricing::PricingTier;
use crate::data_commons::reputation::{
    ContributionCalculator, ContributionHistory, EigenTrustEngine,
};
use crate::data_commons::types::{DataCategory, ReputationAttestation, WalletAddress};
use crate::mesh::node::NodeId;
use crate::mesh::tokenomics::accounting::{PeerAccount, ResourceAccounting};
use std::collections::HashMap;

pub struct MeshCommonsBridge {
    /// Maps a mesh NodeId to its associated Data Commons WalletAddress
    pub node_to_wallet: HashMap<NodeId, WalletAddress>,
}

impl Default for MeshCommonsBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl MeshCommonsBridge {
    pub fn new() -> Self {
        Self {
            node_to_wallet: HashMap::new(),
        }
    }

    /// Bind a NodeId to a WalletAddress
    pub fn bind_node(&mut self, node_id: NodeId, wallet: WalletAddress) {
        self.node_to_wallet.insert(node_id, wallet);
    }

    /// Retrieve the bound WalletAddress for a NodeId
    pub fn get_wallet(&self, node_id: &NodeId) -> Option<&WalletAddress> {
        self.node_to_wallet.get(node_id)
    }

    /// Bi-directional Reputation Sync: Mesh -> Data Commons.
    /// Maps resource contribution metrics from Xavier Mesh `ResourceAccounting`
    /// into Data Commons `ContributionHistory` and adds system endorsements (`ReputationAttestation`)
    /// in the `EigenTrustEngine` based on active participation.
    pub fn sync_mesh_to_commons(
        &self,
        node_id: &NodeId,
        accounting: &ResourceAccounting,
        engine: &mut EigenTrustEngine,
        history: &mut ContributionHistory,
        system_wallet: &WalletAddress,
    ) -> Result<(), String> {
        let wallet = self
            .get_wallet(node_id)
            .ok_or_else(|| "Node not bound to any wallet".to_string())?;

        let peer_acc = accounting
            .accounts
            .get(node_id)
            .ok_or_else(|| "Peer account not found in ResourceAccounting".to_string())?;

        // 1. Update ContributionHistory
        history.total_uptime = peer_acc
            .storage_contributed
            .max(peer_acc.bandwidth_contributed)
            / 1000;

        // Populate simulated validations
        for i in 0..peer_acc.quality_contributions {
            if history.validations.len() < i as usize + 1 {
                history
                    .validations
                    .push(crate::data_commons::reputation::ValidationRecord {
                        context_hash: format!("val_ctx_{}_{}", node_id.0, i),
                        was_correct: true,
                        timestamp: 1700000000 + (i as u64 * 3600),
                    });
            }
        }

        // 2. Add Reputation Attestations in EigenTrustEngine based on contributions.
        // If a node contributed significantly more than consumed, the system (pre-trusted) endorses its wallet.
        if peer_acc.storage_contributed > peer_acc.storage_consumed
            && peer_acc.storage_contributed > 0
        {
            engine.add_attestation(ReputationAttestation {
                from: system_wallet.clone(),
                to: wallet.clone(),
                score: 1,
                context_hash: Some("system_resource_contribution_endorsement".to_string()),
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                signature: Vec::new(),
            });
        } else if peer_acc.storage_consumed > peer_acc.storage_contributed * 2
            && peer_acc.storage_consumed > 1000
        {
            // Negative attestation (report/distrust) for freeloading behaviors
            engine.add_attestation(ReputationAttestation {
                from: system_wallet.clone(),
                to: wallet.clone(),
                score: -1,
                context_hash: Some("system_resource_freeloading_report".to_string()),
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                signature: Vec::new(),
            });
        }

        Ok(())
    }

    /// Bi-directional Reputation Sync: Data Commons -> Mesh.
    /// Propagates computed EigenTrust hybrid scores back to `ResourceAccounting` peer accounts,
    /// directly impacting their reputation_score and freeloader status.
    pub fn sync_commons_to_mesh(
        &self,
        node_id: &NodeId,
        engine: &EigenTrustEngine,
        history: &ContributionHistory,
        accounting: &mut ResourceAccounting,
    ) -> Result<(), String> {
        let wallet = self
            .get_wallet(node_id)
            .ok_or_else(|| "Node not bound to any wallet".to_string())?;

        // Calculate hybrid score
        let eigentrust_score = engine.trust_score(wallet).unwrap_or(0.1); // Fallback to neutral seed trust
        let contribution_score = ContributionCalculator::calculate(wallet, history);

        let hybrid = engine.hybrid_score(eigentrust_score, contribution_score as f64 / 1000.0);

        // Map 0.0..1.0 hybrid score to 0..1000 reputation score in the mesh
        let mesh_rep = (hybrid * 1000.0).clamp(0.0, 1000.0) as u32;

        let peer_acc = accounting.get_account_mut(node_id);
        peer_acc.reputation_score = mesh_rep;

        Ok(())
    }

    /// Announces a data package (dataset) to the Data Commons marketplace on behalf of a mesh node.
    pub fn announce_dataset_mesh(
        &self,
        marketplace: &mut DataMarketplace,
        publisher_node: &NodeId,
        name: String,
        description: String,
        category: String,
        rows: Vec<serde_json::Value>,
        tier: PricingTier,
        engine: &EigenTrustEngine,
    ) -> Result<DatasetId, String> {
        let wallet = self
            .get_wallet(publisher_node)
            .ok_or_else(|| "Publisher node not bound to any wallet".to_string())?;

        let reputation = engine.trust_score(wallet).unwrap_or(0.0);

        let metadata = DatasetMetadata {
            name,
            description,
            category,
            price: 0, // Computed dynamically in list_dataset
            publisher: wallet.0.clone(),
            rows,
            tier,
            reputation,
        };

        let dataset_id = marketplace.list_dataset(metadata);
        Ok(dataset_id)
    }

    /// Consumes a data package (dataset) from the marketplace.
    /// Enforces access control by verifying the buyer is not a freeloader (bad karma) in the mesh,
    /// executes the query with payment, and handles transaction settlements by updating resource accounting.
    pub fn consume_dataset_mesh(
        &self,
        marketplace: &DataMarketplace,
        buyer_node: &NodeId,
        seller_node: &NodeId,
        dataset_id: &DatasetId,
        query: &str,
        payment: u64,
        accounting: &mut ResourceAccounting,
    ) -> Result<DataPage, String> {
        // Enforce mesh access control based on karma/reputation
        if accounting.is_freeloader(buyer_node) {
            return Err(
                "Access Denied: Node is marked as a freeloader due to low reputation (bad karma)"
                    .to_string(),
            );
        }

        let page = marketplace.query_dataset(dataset_id, query, payment)?;

        // Transaction settlement in resource accounting:
        // Buyer consumed bandwidth and storage. Seller contributed bandwidth and storage.
        let data_size = serde_json::to_vec(&page.records).unwrap_or_default().len() as u64;

        // Record metrics: storage size of chunk/records plus estimated overhead
        accounting.record_consumption(buyer_node, data_size, data_size * 2, 0);
        accounting.record_contribution(seller_node, data_size, data_size * 2, 0);

        // Reward seller with a quality contribution bonus
        accounting.record_quality_contribution(seller_node);

        Ok(page)
    }

    /// Charge storage rent in the node network.
    /// Nodes storing files/data packages in the mesh pay rent (modeled as resource consumption).
    pub fn charge_storage_rent_consumer(
        &self,
        accounting: &mut ResourceAccounting,
        node_id: &NodeId,
        bytes_stored: u64,
    ) {
        // Consumer pays rent -> storage consumed increases, affecting reputation
        accounting.record_consumption(node_id, bytes_stored, 0, 0);
    }

    /// Reward storage rent to a provider node hosting data in the mesh.
    pub fn reward_storage_rent_provider(
        &self,
        accounting: &mut ResourceAccounting,
        node_id: &NodeId,
        bytes_hosted: u64,
    ) {
        // Provider gets rewarded -> storage contributed increases, improving reputation
        accounting.record_contribution(node_id, bytes_hosted, 0, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_commons::pricing::PricingTier;
    use crate::data_commons::reputation::ReputationConfig;

    fn make_test_nodes() -> (NodeId, NodeId) {
        (
            NodeId::parse("xv1-testnode11111111111111111111").unwrap(),
            NodeId::parse("xv1-testnode22222222222222222222").unwrap(),
        )
    }

    #[test]
    fn test_bridge_registration_and_binding() {
        let mut bridge = MeshCommonsBridge::new();
        let (node_a, _) = make_test_nodes();
        let wallet_a = WalletAddress(
            "xv1_1qyp0ephnj8fhf8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0".into(),
        );

        bridge.bind_node(node_a.clone(), wallet_a.clone());
        assert_eq!(bridge.get_wallet(&node_a).unwrap(), &wallet_a);
    }

    #[test]
    fn test_announce_and_consume_data_package() {
        let mut bridge = MeshCommonsBridge::new();
        let (node_seller, node_buyer) = make_test_nodes();
        let wallet_seller = WalletAddress(
            "xv1_1qyp0ephnj8fhf8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n1".into(),
        );
        let wallet_buyer = WalletAddress(
            "xv1_1qyp0ephnj8fhf8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n2".into(),
        );

        bridge.bind_node(node_seller.clone(), wallet_seller.clone());
        bridge.bind_node(node_buyer.clone(), wallet_buyer.clone());

        let mut marketplace = DataMarketplace::new();
        let mut accounting = ResourceAccounting::new();
        let engine = EigenTrustEngine::new(ReputationConfig::default(), vec![]);

        let mut rows = vec![
            serde_json::json!({ "metric": "cpu", "value": 15 }),
            serde_json::json!({ "metric": "ram", "value": 72 }),
        ];
        // Pad for price mapping under Colaborador tier
        for _ in 0..98 {
            rows.push(serde_json::json!({ "metric": "dummy", "value": 0 }));
        }

        // 1. Announce dataset on behalf of mesh node
        let dataset_id = bridge
            .announce_dataset_mesh(
                &mut marketplace,
                &node_seller,
                "Mesh Stats".to_string(),
                "Network telemetries".to_string(),
                "Telemetry".to_string(),
                rows,
                PricingTier::Colaborador,
                &engine,
            )
            .unwrap();

        assert!(dataset_id.0.starts_with("ds_"));

        // 2. Consume dataset on behalf of mesh node (Buyer is not a freeloader, initial reputation is 500)
        let page = bridge
            .consume_dataset_mesh(
                &marketplace,
                &node_buyer,
                &node_seller,
                &dataset_id,
                "ram",
                10, // Pricing Tier Colaborador, 100 items -> 10 tokens price
                &mut accounting,
            )
            .unwrap();

        assert_eq!(page.records.len(), 1);
        assert_eq!(page.records[0]["value"], 72);

        // Verify resource accounting updates
        let buyer_acc = accounting.accounts.get(&node_buyer).unwrap();
        let seller_acc = accounting.accounts.get(&node_seller).unwrap();

        assert!(buyer_acc.storage_consumed > 0);
        assert!(buyer_acc.bandwidth_consumed > 0);
        assert!(seller_acc.storage_contributed > 0);
        assert!(seller_acc.bandwidth_contributed > 0);
        assert_eq!(seller_acc.quality_contributions, 1);
    }

    #[test]
    fn test_freeloader_access_denied() {
        let mut bridge = MeshCommonsBridge::new();
        let (node_seller, node_buyer) = make_test_nodes();
        let wallet_seller = WalletAddress(
            "xv1_1qyp0ephnj8fhf8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n1".into(),
        );
        let wallet_buyer = WalletAddress(
            "xv1_1qyp0ephnj8fhf8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n2".into(),
        );

        bridge.bind_node(node_seller.clone(), wallet_seller.clone());
        bridge.bind_node(node_buyer.clone(), wallet_buyer.clone());

        let mut marketplace = DataMarketplace::new();
        let mut accounting = ResourceAccounting::new();
        let engine = EigenTrustEngine::new(ReputationConfig::default(), vec![]);

        // Make buyer a freeloader by registering massive consumption and no contribution
        accounting.record_consumption(&node_buyer, 1_000_000, 1_000_000, 1_000_000);
        assert!(accounting.is_freeloader(&node_buyer));

        let mut rows = vec![serde_json::json!({ "log": "test" })];
        for _ in 0..99 {
            rows.push(serde_json::json!({ "log": "dummy" }));
        }

        let dataset_id = bridge
            .announce_dataset_mesh(
                &mut marketplace,
                &node_seller,
                "Mesh Logs".to_string(),
                "Error logs".to_string(),
                "FunctionalError".to_string(),
                rows,
                PricingTier::Colaborador,
                &engine,
            )
            .unwrap();

        // Querying should return access denied because node_buyer is a freeloader (bad karma)
        let query_res = bridge.consume_dataset_mesh(
            &marketplace,
            &node_buyer,
            &node_seller,
            &dataset_id,
            "",
            10,
            &mut accounting,
        );

        assert!(query_res.is_err());
        assert!(query_res.unwrap_err().contains("Access Denied"));
    }

    #[test]
    fn test_bidirectional_reputation_sync() {
        let mut bridge = MeshCommonsBridge::new();
        let (node_a, _) = make_test_nodes();
        let wallet_a = WalletAddress(
            "xv1_1qyp0ephnj8fhf8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n3".into(),
        );
        let system_wallet = WalletAddress(
            "xv1_1qyp0ephnj8fhf8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0".into(),
        );

        bridge.bind_node(node_a.clone(), wallet_a.clone());

        let mut accounting = ResourceAccounting::new();
        let mut engine =
            EigenTrustEngine::new(ReputationConfig::default(), vec![system_wallet.clone()]);
        let mut history = ContributionHistory::default();

        // 1. Give node_a good contributions in mesh accounting
        accounting.record_contribution(&node_a, 5000, 5000, 0);
        accounting.record_quality_contribution(&node_a);

        // Sync mesh stats to Data Commons
        bridge
            .sync_mesh_to_commons(
                &node_a,
                &accounting,
                &mut engine,
                &mut history,
                &system_wallet,
            )
            .unwrap();

        // Verify history has been populated
        assert_eq!(history.total_uptime, 5); // 5000 / 1000
        assert_eq!(history.validations.len(), 1);

        // Run EigenTrust computation
        let _ = engine.compute().unwrap();

        // 2. Sync reputation from Data Commons back to mesh
        bridge
            .sync_commons_to_mesh(&node_a, &engine, &history, &mut accounting)
            .unwrap();

        // Node reputation should reflect the sync
        let peer_acc = accounting.accounts.get(&node_a).unwrap();
        // Since it has excellent contributions, hybrid reputation should be quite high (>= 300)
        assert!(peer_acc.reputation_score >= 300);
    }

    #[test]
    fn test_storage_rent_consumer_provider() {
        let bridge = MeshCommonsBridge::new();
        let (node_provider, node_consumer) = make_test_nodes();
        let mut accounting = ResourceAccounting::new();

        // 1. Consumer stores 10,000 bytes in mesh -> paying rent
        bridge.charge_storage_rent_consumer(&mut accounting, &node_consumer, 10000);

        // 2. Provider hosts 10,000 bytes -> earning rent
        bridge.reward_storage_rent_provider(&mut accounting, &node_provider, 10000);

        let consumer_acc = accounting.accounts.get(&node_consumer).unwrap();
        let provider_acc = accounting.accounts.get(&node_provider).unwrap();

        assert_eq!(consumer_acc.storage_consumed, 10000);
        assert_eq!(provider_acc.storage_contributed, 10000);

        // Consumer should have a lower reputation due to consumption/rent
        assert!(consumer_acc.reputation_score < provider_acc.reputation_score);
    }

    #[test]
    fn test_multi_node_sharing_and_economy_settlement() {
        let (node_a, node_b) = make_test_nodes();
        let wallet_a = WalletAddress(
            "xv1_1qyp0ephnj8fhf8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n4".into(),
        );
        let wallet_b = WalletAddress(
            "xv1_1qyp0ephnj8fhf8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n0h8n5".into(),
        );

        let mut bridge = MeshCommonsBridge::new();
        bridge.bind_node(node_a.clone(), wallet_a.clone());
        bridge.bind_node(node_b.clone(), wallet_b.clone());

        let mut marketplace = DataMarketplace::new();
        let mut accounting = ResourceAccounting::new();
        let engine = EigenTrustEngine::new(ReputationConfig::default(), vec![]);

        let rows = vec![serde_json::json!({ "signal": "ok" })];

        // Node A announces data
        let dataset_id = bridge
            .announce_dataset_mesh(
                &mut marketplace,
                &node_a,
                "Signal Data".to_string(),
                "Decentralized telemetry".to_string(),
                "BasicTelemetry".to_string(),
                rows,
                PricingTier::Free, // Free tier dataset
                &engine,
            )
            .unwrap();

        // Node B consumes A's data
        let page = bridge
            .consume_dataset_mesh(
                &marketplace,
                &node_b,
                &node_a,
                &dataset_id,
                "",
                0, // Free tier payment
                &mut accounting,
            )
            .unwrap();

        assert_eq!(page.records.len(), 1);

        // Verify resource metrics
        let peer_a = accounting.accounts.get(&node_a).unwrap();
        let peer_b = accounting.accounts.get(&node_b).unwrap();

        assert_eq!(peer_b.storage_consumed, 17); // "[{"signal":"ok"}]" serialized size is 17 bytes
        assert_eq!(peer_a.storage_contributed, 17);
    }
}
