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

fn node_hash(left: &str, right: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"swal-merkle-node-v1:");
    hasher.update(left.as_bytes());
    hasher.update(right.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn merkle_root(leaves: &[String]) -> Option<String> {
    if leaves.is_empty() {
        return None;
    }
    let mut current_level: Vec<String> = leaves.iter().filter_map(|l| leaf_hash_hex(l)).collect();
    if current_level.len() != leaves.len() {
        return None;
    }
    while current_level.len() > 1 {
        let mut next_level = Vec::with_capacity(current_level.len().div_ceil(2));
        for chunk in current_level.chunks(2) {
            let left = &chunk[0];
            let right = if chunk.len() > 1 { &chunk[1] } else { left };
            next_level.push(node_hash(left, right));
        }
        current_level = next_level;
    }
    current_level.into_iter().next()
}

pub struct ProofStep {
    pub sibling: String,
    pub sibling_is_left: bool,
}

pub fn proof(leaves: &[String], index: usize) -> Option<Vec<ProofStep>> {
    if index >= leaves.len() || leaves.is_empty() {
        return None;
    }
    let mut current_level: Vec<String> = leaves.iter().filter_map(|l| leaf_hash_hex(l)).collect();
    if current_level.len() != leaves.len() {
        return None;
    }
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
            sibling: current_level[sibling_index].clone(),
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

pub fn verify_proof(leaf_hex: &str, steps: &[ProofStep], root_hex: &str) -> bool {
    let mut current_hash = match leaf_hash_hex(leaf_hex) {
        Some(h) => h,
        None => return false,
    };
    for step in steps {
        if step.sibling_is_left {
            current_hash = node_hash(&step.sibling, &current_hash);
        } else {
            current_hash = node_hash(&current_hash, &step.sibling);
        }
    }
    current_hash == root_hex
}

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
        let leaves = vec!["01", "02", "03", "04", "05", "06", "07"];
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
    fn test_tampered_leaf_fails() {
        let leaves = vec!["01".to_string(), "02".to_string()];
        let root = merkle_root(&leaves).unwrap();
        let p = proof(&leaves, 0).unwrap();
        assert!(!verify_proof("03", &p, &root));
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
    fn test_domain_separation_prefixes_differ() {
        assert_ne!(b"swal-merkle-leaf-v1:", b"swal-merkle-node-v1:");
    }

    #[test]
    fn test_root_stable() {
        let leaves = vec!["01".to_string(), "02".to_string()];
        assert_eq!(merkle_root(&leaves).unwrap(), merkle_root(&leaves).unwrap());
    }
}
