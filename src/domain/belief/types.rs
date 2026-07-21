// SPDX-License-Identifier: MIT OR LICENSE-MESH
//! Belief system type definitions
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeliefNode {
    pub id: String,
    pub belief: String,
    pub confidence: f32,
    pub source: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum BeliefRelation {
    Supports,
    Contradicts,
    Neutral,
}
