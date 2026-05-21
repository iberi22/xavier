use std::collections::HashMap;
use crate::memory::schema::ContextZone;
use crate::agents::provider::ModelProviderClient;
use anyhow::Result;

#[derive(Clone)]
pub struct QueryProcessor {
    provider: ModelProviderClient,
}

impl std::fmt::Debug for QueryProcessor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QueryProcessor").finish()
    }
}

impl Default for QueryProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl QueryProcessor {
    pub fn new() -> Self {
        Self {
            provider: ModelProviderClient::from_env(),
        }
    }

    pub async fn expand_and_decompose(&self, prompt: &str, zones: &[ContextZone]) -> HashMap<ContextZone, Vec<String>> {
        if zones.is_empty() {
            return HashMap::new();
        }

        match self.expand_with_llm(prompt, zones).await {
            Ok(expanded) if !expanded.is_empty() => expanded,
            _ => {
                // Fallback: use the original prompt for each zone
                let mut fallback = HashMap::new();
                for &zone in zones {
                    fallback.insert(zone, vec![prompt.to_string()]);
                }
                fallback
            }
        }
    }

    async fn expand_with_llm(&self, prompt: &str, zones: &[ContextZone]) -> Result<HashMap<ContextZone, Vec<String>>> {
        let zones_str = zones.iter().map(|z| z.as_str()).collect::<Vec<_>>().join(", ");
        let system_prompt = format!(
            "You are a query expansion and decomposition assistant. \
             Active zones: [{}]. \
             Task: Decompose and expand the user prompt into specific queries for each zone. \
             - atomic: focused on specific details, code snippets, raw facts. \
             - cluster: focused on summaries, groups of related items, topic overviews. \
             - global: focused on high-level architecture, strategy, and overall project goals. \
             - relational: focused on connections, dependencies, and knowledge graph relationships. \
             \
             Respond ONLY with a valid JSON object where keys are zone names and values are arrays of expanded query strings. \
             Example: {{\"atomic\": [\"query1\", \"query2\"], \"cluster\": [\"query3\"]}}",
            zones_str
        );

        let response = self.provider.generate_text(&system_prompt, prompt).await?;

        // Extract JSON from response (handling potential markdown formatting)
        let json_str = if let Some(start) = response.find('{') {
            if let Some(end) = response.rfind('}') {
                &response[start..=end]
            } else {
                &response[start..]
            }
        } else {
            &response
        };

        let raw_map: HashMap<String, Vec<String>> = serde_json::from_str(json_str)?;
        let mut result = HashMap::new();
        for (zone_name, queries) in raw_map {
            let zone = ContextZone::parse(&zone_name);
            // Only include requested zones if they were actually requested
            if zones.contains(&zone) {
                result.insert(zone, queries);
            }
        }

        // Ensure all requested zones have at least one query (fallback to original if LLM skipped them)
        for &zone in zones {
            let queries = result.entry(zone).or_insert_with(Vec::new);
            if queries.is_empty() {
                queries.push(prompt.to_string());
            }
        }

        Ok(result)
    }
}
