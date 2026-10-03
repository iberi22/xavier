//! HumanChallenge source: curated votes the user explicitly marked `training_eligible`.
//!
//! Read-only via `HumanChallengeStore`'s public API. Pairing:
//! * instruction = the challenge event `description` (the question put to the human).
//! * response = the vote's `curated_content` (human-refined) else the event `response`.
//! * Only accepted/refined votes are returned by the store (`get_training_eligible_votes`).
//! * Events flagged `privacy_p4_local_only` are included only when the export itself is P4;
//!   otherwise they are counted under `challenge:local_only` and left out.

use super::{metadata_object, SourceBatch, SourceContext, TrainingSource};
use crate::data_commons::privacy::PrivacyLevel;
use crate::humanchallenge::store::HumanChallengeStore;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

pub struct ChallengeSource {
    store: Arc<HumanChallengeStore>,
    domain: Option<String>,
    limit: u32,
}

impl ChallengeSource {
    pub fn new(store: Arc<HumanChallengeStore>) -> Self {
        Self {
            store,
            domain: None,
            limit: 10_000,
        }
    }

    /// Keep only votes whose `domain_tags` contain `domain`.
    pub fn with_domain(mut self, domain: Option<String>) -> Self {
        self.domain = domain.filter(|d| !d.is_empty());
        self
    }

    pub fn with_limit(mut self, limit: u32) -> Self {
        self.limit = limit;
        self
    }
}

#[async_trait]
impl TrainingSource for ChallengeSource {
    fn name(&self) -> &'static str {
        "challenge"
    }

    async fn collect(&self, ctx: &SourceContext) -> Result<SourceBatch, String> {
        let votes = self
            .store
            .get_training_eligible_votes(self.limit)
            .map_err(|e| e.to_string())?;
        let mut batch = SourceBatch {
            total_found: votes.len(),
            ..SourceBatch::default()
        };
        for vote in votes {
            if !vote.training_eligible {
                batch.excluded_no_consent += 1;
                continue;
            }
            if let Some(domain) = &self.domain {
                if !vote.domain_tags.iter().any(|t| t == domain) {
                    continue;
                }
            }
            let event = match self.store.get_event_by_id(&vote.challenge_id) {
                Ok(Some(event)) => event,
                _ => {
                    batch.exclude("challenge:event_missing");
                    continue;
                }
            };
            if event.privacy_p4_local_only && ctx.privacy_level != PrivacyLevel::P4 {
                batch.exclude("challenge:local_only");
                continue;
            }
            let response = vote
                .curated_content
                .clone()
                .filter(|c| !c.trim().is_empty())
                .or_else(|| event.response.clone().filter(|r| !r.trim().is_empty()));
            let Some(response) = response else {
                batch.exclude("challenge:no_response");
                continue;
            };
            if event.description.trim().is_empty() {
                batch.exclude("challenge:no_question");
                continue;
            }
            let domain = vote.domain_tags.first().map(String::as_str);
            let mut metadata = metadata_object(&[
                ("source", Some("challenge")),
                ("kind", Some(event.challenge_type.as_str())),
                ("domain", domain),
            ]);
            metadata["challenge_id"] = Value::String(vote.challenge_id.clone());
            batch.records.push(serde_json::json!({
                "instruction": event.description,
                "response": response,
                "metadata": metadata,
            }));
        }
        Ok(batch)
    }
}
