//! Impact radius analysis

use crate::db::CodeGraphDB;
use crate::error::Result;
use crate::types::{EdgeType, Symbol};
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

pub struct ImpactAnalyzer {
    db: Arc<CodeGraphDB>,
}

impl ImpactAnalyzer {
    pub fn new(db: Arc<CodeGraphDB>) -> Self {
        Self { db }
    }

    /// Analyze impact radius of a symbol (forward and backward)
    pub fn analyze(&self, symbol_id: &str, depth: usize) -> Result<ImpactResult> {
        let symbol = self.db.symbol_by_stable_id(symbol_id)?;
        let Some(symbol) = symbol else {
            return Ok(ImpactResult::default());
        };

        let callers = self.traverse(symbol_id, depth, true)?;
        let callees = self.traverse(symbol_id, depth, false)?;

        Ok(ImpactResult {
            symbol,
            callers,
            callees,
        })
    }

    fn traverse(&self, start_id: &str, max_depth: usize, reverse: bool) -> Result<Vec<ImpactNode>> {
        let mut queue = VecDeque::from([(start_id.to_string(), 0usize)]);
        let mut seen = HashSet::new();
        let mut results = Vec::new();

        while let Some((current_id, current_depth)) = queue.pop_front() {
            if current_depth >= max_depth {
                continue;
            }
            if !seen.insert(current_id.clone()) {
                continue;
            }

            let edges = if reverse {
                self.db
                    .find_edges_to(&current_id, Some(EdgeType::Calls), 100)?
            } else {
                self.db
                    .find_edges_from(&current_id, Some(EdgeType::Calls), 100)?
            };

            for edge in edges {
                let next_id = if reverse {
                    edge.from_symbol.clone()
                } else {
                    edge.to_symbol.clone()
                };

                if next_id.starts_with("file:") || next_id.starts_with("module:") {
                    continue;
                }

                if let Some(next_symbol) = self.db.symbol_by_stable_id(&next_id)? {
                    results.push(ImpactNode {
                        symbol: next_symbol,
                        depth: current_depth + 1,
                    });
                    queue.push_back((next_id, current_depth + 1));
                }
            }
        }

        Ok(results)
    }
}

#[derive(Default, serde::Serialize)]
pub struct ImpactResult {
    pub symbol: Symbol,
    pub callers: Vec<ImpactNode>,
    pub callees: Vec<ImpactNode>,
}

#[derive(serde::Serialize)]
pub struct ImpactNode {
    pub symbol: Symbol,
    pub depth: usize,
}
