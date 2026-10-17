use sha2::{Digest, Sha256};

pub const DEFAULT_CHAIN_ID: u64 = 80002;

pub fn leaf_hash(leaf_hex: &str) -> Option<[u8; 32]> {
    let leaf_bytes = hex::decode(leaf_hex).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(b"swal-merkle-leaf-v1:");
    hasher.update(&leaf_bytes);
    Some(hasher.finalize().into())
}

pub fn leaf_hash_hex(leaf_hex: &str) -> Option<String> {
    leaf_hash(leaf_hex).map(hex::encode)
}

fn node_hash(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"swal-merkle-node-v1:");
    hasher.update(left);
    hasher.update(right);
    hasher.finalize().into()
}

pub fn merkle_root(leaves: &[String]) -> Option<String> {
    if leaves.is_empty() {
        return None;
    }
    let mut current_level: Vec<[u8; 32]> =
        leaves.iter().map(|l| leaf_hash(l)).collect::<Option<_>>()?;
    while current_level.len() > 1 {
        let mut next_level = Vec::with_capacity(current_level.len().div_ceil(2));
        for chunk in current_level.chunks(2) {
            let left = &chunk[0];
            let right = if chunk.len() > 1 { &chunk[1] } else { left };
            next_level.push(node_hash(left, right));
        }
        current_level = next_level;
    }
    current_level.into_iter().next().map(hex::encode)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProofStep {
    pub sibling: String,
    pub sibling_is_left: bool,
}

pub fn proof(leaves: &[String], index: usize) -> Option<Vec<ProofStep>> {
    if index >= leaves.len() || leaves.is_empty() {
        return None;
    }
    let mut current_level: Vec<[u8; 32]> =
        leaves.iter().map(|l| leaf_hash(l)).collect::<Option<_>>()?;
    let mut steps = Vec::new();
    let mut current_index = index;
    while current_level.len() > 1 {
        let is_left = current_index.is_multiple_of(2);
        let sibling_index = if is_left {
            if current_index + 1 < current_level.len() {
                current_index + 1
            } else {
                current_index
            }
        } else {
            current_index - 1
        };
        steps.push(ProofStep {
            sibling: hex::encode(current_level[sibling_index]),
            sibling_is_left: !is_left,
        });
        let mut next_level = Vec::with_capacity(current_level.len().div_ceil(2));
        for chunk in current_level.chunks(2) {
            let left = &chunk[0];
            let right = if chunk.len() > 1 { &chunk[1] } else { left };
            next_level.push(node_hash(left, right));
        }
        current_level = next_level;
        current_index /= 2;
    }
    Some(steps)
}

fn decode_32_bytes(hex_str: &str) -> Option<[u8; 32]> {
    let bytes = hex::decode(hex_str).ok()?;
    bytes.try_into().ok()
}

pub fn verify_proof(leaf_hex: &str, steps: &[ProofStep], root_hex: &str) -> bool {
    let mut current_hash = match leaf_hash(leaf_hex) {
        Some(h) => h,
        None => return false,
    };
    let target_root = match decode_32_bytes(root_hex) {
        Some(r) => r,
        None => return false,
    };
    for step in steps {
        let sibling_bytes = match decode_32_bytes(&step.sibling) {
            Some(s) => s,
            None => return false,
        };
        if step.sibling_is_left {
            current_hash = node_hash(&sibling_bytes, &current_hash);
        } else {
            current_hash = node_hash(&current_hash, &sibling_bytes);
        }
    }
    current_hash == target_root
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnchorPlan {
    pub root: String,
    pub leaf_count: usize,
    pub chain_id: u64,
    pub dry_run: bool,
    pub tx: Option<String>,
}

pub fn dry_run_plan(leaves: &[String]) -> Option<AnchorPlan> {
    Some(AnchorPlan {
        root: merkle_root(leaves)?,
        leaf_count: leaves.len(),
        chain_id: DEFAULT_CHAIN_ID,
        dry_run: true,
        tx: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_leaf_root() {
        let leaf = "deadbeef";
        assert_eq!(
            merkle_root(&[leaf.to_string()]).unwrap(),
            leaf_hash_hex(leaf).unwrap()
        );
    }

    #[test]
    fn test_empty_list() {
        assert_eq!(merkle_root(&[]), None);
        assert!(proof(&[], 0).is_none());
        assert!(dry_run_plan(&[]).is_none());
    }

    #[test]
    fn test_invalid_hex() {
        let leaves = vec!["invalid hex".to_string()];
        assert_eq!(merkle_root(&leaves), None);
        assert!(proof(&leaves, 0).is_none());
        assert!(dry_run_plan(&leaves).is_none());
    }

    #[test]
    fn test_proof_verification_1_to_7_leaves() {
        let leaves = ["01", "02", "03", "04", "05", "06", "07"];
        for n in 1..=7 {
            let current_leaves: Vec<String> = leaves[0..n].iter().map(|s| s.to_string()).collect();
            let root = merkle_root(&current_leaves).unwrap();
            for i in 0..n {
                let p = proof(&current_leaves, i).unwrap();
                assert!(verify_proof(&current_leaves[i], &p, &root));
            }
        }
    }

    #[test]
    fn test_golden_vectors() {
        let leaves_2 = vec!["01".to_string(), "02".to_string()];
        assert_eq!(
            merkle_root(&leaves_2).unwrap(),
            "f9e2d9323dfa738af5240755ceb59985d6ff67c3ad760d4cb55da3c165d68eb2"
        );

        let leaves_3 = vec!["01".to_string(), "02".to_string(), "03".to_string()];
        assert_eq!(
            merkle_root(&leaves_3).unwrap(),
            "31b040b7dd09baa7816b70af079e7bd99ba917335c7471582cba2e0a066c0da3"
        );
    }

    #[test]
    fn test_case_insensitive_root_and_siblings() {
        let leaves = vec!["01".to_string(), "02".to_string()];
        let root = merkle_root(&leaves).unwrap();
        let mut p = proof(&leaves, 0).unwrap();

        // Verify with uppercase root
        assert!(verify_proof("01", &p, &root.to_uppercase()));

        // Verify with uppercase sibling
        p[0].sibling = p[0].sibling.to_uppercase();
        assert!(verify_proof("01", &p, &root));
        assert!(verify_proof("01", &p, &root.to_uppercase()));
    }

    #[test]
    fn test_proof_out_of_bounds() {
        let leaves = vec!["01".to_string(), "02".to_string()];
        assert!(proof(&leaves, leaves.len()).is_none());
        assert!(proof(&leaves, leaves.len() + 5).is_none());
    }

    #[test]
    fn test_negative_verification_cases() {
        let leaves = vec!["01".to_string(), "02".to_string()];
        let root = merkle_root(&leaves).unwrap();
        let p = proof(&leaves, 0).unwrap();

        // Tampered leaf
        assert!(!verify_proof("03", &p, &root));

        // Tampered sibling
        let mut tampered_p = p.clone();
        tampered_p[0].sibling = "00".repeat(32);
        assert!(!verify_proof("01", &tampered_p, &root));

        // Wrong root
        let wrong_root = "00".repeat(32);
        assert!(!verify_proof("01", &p, &wrong_root));

        // Invalid hex for sibling
        let mut invalid_sibling_p = p.clone();
        invalid_sibling_p[0].sibling = "not-valid-hex".to_string();
        assert!(!verify_proof("01", &invalid_sibling_p, &root));

        // Invalid length hex for sibling
        let mut short_sibling_p = p.clone();
        short_sibling_p[0].sibling = "010203".to_string();
        assert!(!verify_proof("01", &short_sibling_p, &root));

        // Invalid hex for root
        assert!(!verify_proof("01", &p, "not-valid-hex"));

        // Invalid length hex for root
        assert!(!verify_proof("01", &p, "010203"));
    }

    #[test]
    fn test_domain_separation_leaf_vs_node_differs() {
        let leaf = leaf_hash("01").unwrap();
        let dummy = [0u8; 32];
        let node = node_hash(&leaf, &dummy);
        assert_ne!(leaf, node);
    }

    #[test]
    fn test_dry_run_plan() {
        let leaves = vec!["01".to_string(), "02".to_string()];
        let plan = dry_run_plan(&leaves).unwrap();
        assert_eq!(plan.leaf_count, 2);
        assert_eq!(plan.chain_id, DEFAULT_CHAIN_ID);
        assert!(plan.dry_run);
        assert_eq!(plan.tx, None);
        assert_eq!(plan.root, merkle_root(&leaves).unwrap());
    }

    #[test]
    fn test_root_stable() {
        let leaves = vec!["01".to_string(), "02".to_string()];
        assert_eq!(merkle_root(&leaves).unwrap(), merkle_root(&leaves).unwrap());
    }
}
