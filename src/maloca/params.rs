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
