use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Roles defined for the ACL module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AclRole {
    Admin,
    Colaborador,
    Viewer,
}

impl std::fmt::Display for AclRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AclRole::Admin => write!(f, "admin"),
            AclRole::Colaborador => write!(f, "colaborador"),
            AclRole::Viewer => write!(f, "viewer"),
        }
    }
}

/// A directed acyclic graph representing the role inheritance hierarchy.
/// Admin > Colaborador > Viewer (inherit downward).
/// Inherits(child, parent) -> bool checks if child inherits permissions from parent.
#[derive(Debug, Clone)]
pub struct RoleHierarchy {
    // Map of child role to its direct inherited parent roles.
    relations: HashMap<AclRole, HashSet<AclRole>>,
}

impl Default for RoleHierarchy {
    fn default() -> Self {
        Self::new()
    }
}

impl RoleHierarchy {
    /// Creates a new RoleHierarchy with default relations: Admin > Colaborador > Viewer
    pub fn new() -> Self {
        let mut hierarchy = Self {
            relations: HashMap::new(),
        };
        // Admin inherits from Colaborador
        let _ = hierarchy.add_relation(AclRole::Admin, AclRole::Colaborador);
        // Colaborador inherits from Viewer
        let _ = hierarchy.add_relation(AclRole::Colaborador, AclRole::Viewer);
        hierarchy
    }

    /// Adds an inheritance relation where `child` inherits from `parent`.
    /// Validates that the hierarchy remains a directed acyclic graph (DAG).
    pub fn add_relation(&mut self, child: AclRole, parent: AclRole) -> Result<(), &'static str> {
        if child == parent {
            return Err("Cannot inherit from self");
        }

        // Temporary insert to check for cycle
        let mut test_relations = self.relations.clone();
        test_relations.entry(child).or_default().insert(parent);

        // Detect if cycle is introduced
        if self.has_cycle(&test_relations) {
            return Err("Cycle detected in role hierarchy");
        }

        self.relations = test_relations;
        Ok(())
    }

    /// Checks if a relation graph contains a cycle.
    fn has_cycle(&self, relations: &HashMap<AclRole, HashSet<AclRole>>) -> bool {
        let roles = [AclRole::Admin, AclRole::Colaborador, AclRole::Viewer];
        for &start in &roles {
            let mut visited = HashSet::new();
            let mut rec_stack = HashSet::new();
            if self.cycle_dfs(start, relations, &mut visited, &mut rec_stack) {
                return true;
            }
        }
        false
    }

    fn cycle_dfs(
        &self,
        node: AclRole,
        relations: &HashMap<AclRole, HashSet<AclRole>>,
        visited: &mut HashSet<AclRole>,
        rec_stack: &mut HashSet<AclRole>,
    ) -> bool {
        if rec_stack.contains(&node) {
            return true;
        }
        if visited.contains(&node) {
            return false;
        }

        visited.insert(node);
        rec_stack.insert(node);

        if let Some(parents) = relations.get(&node) {
            for &parent in parents {
                if self.cycle_dfs(parent, relations, visited, rec_stack) {
                    return true;
                }
            }
        }

        rec_stack.remove(&node);
        false
    }

    /// Returns whether `child` inherits from `parent` (reflexive and transitive).
    pub fn inherits(&self, child: AclRole, parent: AclRole) -> bool {
        if child == parent {
            return true;
        }
        // DFS search to find path from child to parent
        let mut visited = HashSet::new();
        let mut stack = vec![child];
        while let Some(current) = stack.pop() {
            if current == parent {
                return true;
            }
            if visited.insert(current) {
                if let Some(parents) = self.relations.get(&current) {
                    for &p in parents {
                        stack.push(p);
                    }
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_hierarchy_defaults() {
        let hierarchy = RoleHierarchy::new();
        // Reflexivity
        assert!(hierarchy.inherits(AclRole::Admin, AclRole::Admin));
        assert!(hierarchy.inherits(AclRole::Colaborador, AclRole::Colaborador));
        assert!(hierarchy.inherits(AclRole::Viewer, AclRole::Viewer));

        // Direct inheritance
        assert!(hierarchy.inherits(AclRole::Admin, AclRole::Colaborador));
        assert!(hierarchy.inherits(AclRole::Colaborador, AclRole::Viewer));

        // Transitive inheritance
        assert!(hierarchy.inherits(AclRole::Admin, AclRole::Viewer));

        // Negative check
        assert!(!hierarchy.inherits(AclRole::Viewer, AclRole::Colaborador));
        assert!(!hierarchy.inherits(AclRole::Colaborador, AclRole::Admin));
    }

    #[test]
    fn test_role_hierarchy_cycle_prevention() {
        let mut hierarchy = RoleHierarchy::new();
        // Adding a relation that creates a cycle: Viewer inherits from Admin
        // (Since Admin -> Colaborador -> Viewer, making Viewer -> Admin creates a cycle)
        let result = hierarchy.add_relation(AclRole::Viewer, AclRole::Admin);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Cycle detected in role hierarchy");

        // Self inheritance attempt
        let result_self = hierarchy.add_relation(AclRole::Viewer, AclRole::Viewer);
        assert!(result_self.is_err());
        assert_eq!(result_self.unwrap_err(), "Cannot inherit from self");
    }
}
