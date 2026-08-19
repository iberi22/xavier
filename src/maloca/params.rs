//! Network parameter defaults — mirror `docs/SWAL/NETWORK_PARAMETERS.md`.

use super::types::NetworkParam;

pub fn network_parameters() -> Vec<NetworkParam> {
    vec![
        NetworkParam {
            key: "ledger_chain".into(),
            default: "polygon".into(),
            locked_until_quorum: false,
            notes: "Canónico; CDK fase posterior".into(),
        },
        NetworkParam {
            key: "genesis_node_id".into(),
            default: "lab_genesis".into(),
            locked_until_quorum: false,
            notes: "Fundador; réplicas = peers".into(),
        },
        NetworkParam {
            key: "vote_karma_min".into(),
            default: "500".into(),
            locked_until_quorum: true,
            notes: "Gate consejo".into(),
        },
        NetworkParam {
            key: "quorum_pct_eligible".into(),
            default: "30".into(),
            locked_until_quorum: true,
            notes: "% nodos elegibles".into(),
        },
        NetworkParam {
            key: "proposal_duration_days".into(),
            default: "7".into(),
            locked_until_quorum: true,
            notes: String::new(),
        },
        NetworkParam {
            key: "manager_reconsider_extends_days".into(),
            default: "3".into(),
            locked_until_quorum: true,
            notes: String::new(),
        },
        NetworkParam {
            key: "manager_adds_vote_weight".into(),
            default: "false".into(),
            locked_until_quorum: false,
            notes: "Siempre false".into(),
        },
        NetworkParam {
            key: "parent_nodes_enabled".into(),
            default: "false".into(),
            locked_until_quorum: false,
            notes: "Modelo plano".into(),
        },
        NetworkParam {
            key: "wallet_multi_node_anchor".into(),
            default: "true".into(),
            locked_until_quorum: false,
            notes: String::new(),
        },
        NetworkParam {
            key: "node_dividends_source".into(),
            default: "service_mesh_only".into(),
            locked_until_quorum: false,
            notes: String::new(),
        },
        NetworkParam {
            key: "token_public_sale_enabled".into(),
            default: "false".into(),
            locked_until_quorum: true,
            notes: "≥2 años intención".into(),
        },
        NetworkParam {
            key: "dex_public_enabled".into(),
            default: "false".into(),
            locked_until_quorum: true,
            notes: String::new(),
        },
        NetworkParam {
            key: "nav_accounting_mode".into(),
            default: "pending_council".into(),
            locked_until_quorum: true,
            notes: String::new(),
        },
        NetworkParam {
            key: "anti_monopoly_caps".into(),
            default: "pending_council".into(),
            locked_until_quorum: true,
            notes: String::new(),
        },
        NetworkParam {
            key: "synapse_unfrozen".into(),
            default: "false".into(),
            locked_until_quorum: true,
            notes: String::new(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_parameters_returns_expected_params() {
        let params = network_parameters();
        assert!(!params.is_empty());

        let keys: Vec<&str> = params.iter().map(|p| p.key.as_str()).collect();

        // Check for specific essential parameters
        assert!(keys.contains(&"vote_karma_min"));
        assert!(keys.contains(&"genesis_node_id"));
        assert!(keys.contains(&"manager_adds_vote_weight"));
        assert!(keys.contains(&"ledger_chain"));

        // Ensure no duplicate keys exist
        let mut unique_keys = keys.clone();
        unique_keys.sort();
        unique_keys.dedup();
        assert_eq!(keys.len(), unique_keys.len());
    }

    #[test]
    fn network_parameters_specific_values() {
        let params = network_parameters();

        let karma = params
            .iter()
            .find(|p| p.key == "vote_karma_min")
            .expect("vote_karma_min present");
        assert_eq!(karma.default, "500");
        assert!(karma.locked_until_quorum);

        let manager_weight = params
            .iter()
            .find(|p| p.key == "manager_adds_vote_weight")
            .expect("manager_adds_vote_weight present");
        assert_eq!(manager_weight.default, "false");
        assert!(!manager_weight.locked_until_quorum);
    }
}
