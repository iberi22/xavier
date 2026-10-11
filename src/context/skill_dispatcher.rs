//! Skill Dispatcher — Routes tasks to skills and builds pre-digested context packs
//!
//! This is the core of Xavier's "masticación" capability. Instead of IDEs or CLI
//! agents deciding which skill to use and what context to inject, the dispatcher:
//! 1. Classifies the incoming task
//! 2. Matches it to the best skill from the registry
//! 3. Gathers relevant memories using the existing retrieval pipeline
//! 4. Compacts everything into a minimal-token ContextPack
//!
//! The result is a payload ready for direct LLM consumption with minimal waste.

use anyhow::Result;
use regex::RegexSet;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock};
use tracing::{info, warn};

use super::skill_controller::composer::{
    self, CompositionInput, CompositionScope, Evidence, SkillEntry,
};
use super::skill_controller::policy::{SecretFinding, SECRET_CLASSES};
use super::skill_registry::SkillRegistry;
use crate::memory::qmd_memory::QmdMemory;
use crate::memory::virtual_memory::{MemoryReference, VirtualMemory};
use crate::security::clearance::ClearanceLevel;

/// Request from an agent/IDE to dispatch a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDispatchRequest {
    /// The task description (e.g. "Analyze the failed traces from bot X")
    pub task: String,
    /// Optional model hint for budget estimation (e.g. "claude-opus-4")
    pub model_hint: Option<String>,
    /// Maximum tokens the agent can afford for this context injection
    pub max_tokens: Option<usize>,
    /// Optional project filter for memory retrieval
    pub project: Option<String>,
}

/// The pre-digested result ready for LLM consumption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDispatchResult {
    /// The matched skill
    pub skill_name: String,
    /// Skill description for the agent
    pub skill_description: String,
    /// Match confidence (0.0 - 1.0)
    pub confidence: f32,
    /// The pre-digested context pack
    pub context_pack: ContextPack,
    /// How many tokens were saved vs. sending everything raw
    pub estimated_savings_pct: f32,
    /// Candidates withheld because they exceeded the requester's ceiling.
    /// Only a count: hidden bodies are never returned.
    #[serde(default)]
    pub hidden_by_clearance: usize,
}

/// A minimal-token payload containing everything the LLM needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPack {
    /// The skill's instructions (potentially compacted to fit budget)
    pub system_instructions: String,
    /// Scoped, fenced, provenance-stamped composition of the task, the eligible
    /// skills and the in-scope evidence. Bounded and secret-gated by the
    /// composer, so it can be delivered as-is.
    #[serde(default)]
    pub composition: String,
    /// Relevant memory references (summaries, not full content)
    pub relevant_memories: Vec<MemoryReference>,
    /// Prior decisions from Xavier's decision memory
    pub prior_decisions: Vec<String>,
    /// Total estimated token count of this pack
    pub total_tokens: usize,
}

/// Minimum confidence score required to dispatch a skill.
///
/// Matches scoring below this threshold (0.40) fallback to a generic `_none`
/// result instead of dispatching a weak match.
///
/// This threshold is intentionally lower and distinct from Maximum-level HTTP
/// context fusion (`FUSION_SKILL_CONFIDENCE_THRESHOLD` / `SKILL_CONFIDENCE_THRESHOLD` = 0.50),
/// which requires higher confidence before injecting skills into conversation prompt budgets.
pub const MIN_DISPATCH_CONFIDENCE: f32 = 0.40;

/// Alias for [`MIN_DISPATCH_CONFIDENCE`].
pub const DISPATCH_CONFIDENCE_THRESHOLD: f32 = MIN_DISPATCH_CONFIDENCE;

const _: () = assert!(MIN_DISPATCH_CONFIDENCE > 0.0 && MIN_DISPATCH_CONFIDENCE < 1.0);

/// What one dispatch gathered from memory, plus its composition.
#[derive(Debug, Clone, Default)]
struct GatheredContext {
    memories: Vec<MemoryReference>,
    decisions: Vec<String>,
    composition: String,
}

/// Render the scoped, fenced, bounded composition section of a pack. It queries
/// nothing: the composer only renders what the dispatcher already retrieved and
/// already scoped to each record's stored workspace.
fn compose_pack(
    scope: &CompositionScope,
    task: &str,
    eligible: &[SkillEntry],
    evidence: &[Evidence],
    budget: usize,
) -> String {
    let input = CompositionInput {
        task_id: composer::task_fingerprint(scope.workspace_id(), task),
        task: task.to_string(),
        skills: eligible.to_vec(),
        evidence: evidence.to_vec(),
        memory_available: true,
        graph_revision: None,
    };
    match composer::compose(scope, &input, budget) {
        Ok(composition) => composition.markdown,
        Err(error) => {
            warn!(error = %error, "skill dispatch: composition blocked");
            String::new()
        }
    }
}

/// The workspace a stored record actually lives in, resolved the way
/// `resolution.rs` resolves a document: the `namespace.workspace_id` overlay
/// first, then a top-level `workspace_id`, never the authorized scope.
fn stored_workspace(metadata: &serde_json::Value) -> Option<String> {
    metadata
        .get("namespace")
        .and_then(|value| value.get("workspace_id"))
        .and_then(|value| value.as_str())
        .or_else(|| {
            metadata
                .get("workspace_id")
                .and_then(|value| value.as_str())
        })
        .map(str::trim)
        .filter(|workspace| !workspace.is_empty())
        .map(str::to_string)
}

/// Whole-output secret gate (D10): scan every field a blocked scan used to
/// leave behind, and drop the untrusted-derived ones on any finding. Only the
/// class and location are reported, never the matched bytes.
fn gate_output(
    relevant_memories: Vec<MemoryReference>,
    prior_decisions: Vec<String>,
    composition: String,
) -> (Vec<MemoryReference>, Vec<String>, String) {
    let mut haystack = composition.clone();
    for memory in &relevant_memories {
        haystack.push('\n');
        haystack.push_str(&memory.summary);
    }
    for decision in &prior_decisions {
        haystack.push('\n');
        haystack.push_str(decision);
    }
    match scan_output(&haystack) {
        Ok(findings) if findings.is_empty() => (relevant_memories, prior_decisions, composition),
        Ok(findings) => {
            let summary = findings
                .iter()
                .map(|finding| format!("{} at {}", finding.class, finding.location))
                .collect::<Vec<_>>()
                .join("; ");
            warn!(summary = %summary, "skill dispatch: whole output blocked by the secret gate");
            (Vec::new(), Vec::new(), String::new())
        }
        Err(detail) => {
            warn!(error = %detail, "skill dispatch: secret patterns unusable");
            (Vec::new(), Vec::new(), String::new())
        }
    }
}

/// Scan arbitrary dispatch bytes with the same classes the package scan uses.
fn scan_output(text: &str) -> Result<Vec<SecretFinding>, String> {
    static SET: OnceLock<Result<RegexSet, String>> = OnceLock::new();
    let set = SET.get_or_init(|| {
        RegexSet::new(SECRET_CLASSES.iter().map(|(_, pattern)| *pattern)).map_err(|e| e.to_string())
    });
    let set = set.as_ref().map_err(|detail| detail.clone())?;
    Ok(set
        .matches(text)
        .into_iter()
        .map(|index| SecretFinding {
            class: SECRET_CLASSES[index].0,
            location: "dispatch output".to_string(),
        })
        .collect())
}

/// Candidates fetched before the clearance ceiling is applied (over-fetch so a
/// private top match cannot crowd out a readable one).
const SKILL_DISPATCH_FETCH: usize = 10;

/// The maximum number of skill descriptions to inject into the Tier 1 prompt.
const MAX_TIER1_SKILLS: usize = 20;

/// The dispatcher that ties the registry, memory, and retrieval together.
pub struct SkillDispatcher {
    registry: SkillRegistry,
    memory: Option<Arc<QmdMemory>>,
    /// Read ceiling applied to matched skills. `None` keeps legacy callers
    /// unfiltered; MCP sets it from the authenticated caller.
    requester_clearance: Option<ClearanceLevel>,
}

impl SkillDispatcher {
    /// Create a new dispatcher with a skill registry and optional memory backend.
    pub fn new(registry: SkillRegistry, memory: Option<Arc<QmdMemory>>) -> Self {
        Self {
            registry,
            memory,
            requester_clearance: None,
        }
    }

    /// Apply the requester's clearance ceiling to matched skills.
    pub fn with_clearance(mut self, clearance: ClearanceLevel) -> Self {
        self.requester_clearance = Some(clearance);
        self
    }

    /// Dispatch a task: find the best skill, gather context, build a pack.
    pub async fn dispatch(&self, request: &SkillDispatchRequest) -> Result<SkillDispatchResult> {
        let max_tokens = request.max_tokens.unwrap_or(composer::DEFAULT_MAX_TOKENS);

        // 1. Find the best matching skill, then apply the caller's ceiling:
        //    hidden matches are counted, never returned (same rule as memories).
        let matches = self.registry.search(&request.task, SKILL_DISPATCH_FETCH);
        let (matches, hidden_by_clearance) = match self.requester_clearance {
            Some(ceiling) => crate::security::clearance::split_by_clearance(
                ceiling,
                matches,
                |(_, skill): &(f32, &super::skill_registry::IndexedSkill)| {
                    self.registry.clearance_of(&skill.name)
                },
            ),
            None => (matches, 0),
        };

        // Instead of unconditionally taking matches.first(), ensure the score meets minimum threshold.
        let (confidence, skill) = if let Some((score, skill)) = matches
            .first()
            .filter(|(s, _)| *s >= MIN_DISPATCH_CONFIDENCE)
        {
            (*score, (*skill).clone())
        } else {
            // No skill matched — return a generic "no-skill" result
            return Ok(SkillDispatchResult {
                skill_name: "_none".to_string(),
                skill_description: "No matching skill found".to_string(),
                confidence: 0.0,
                context_pack: ContextPack {
                    system_instructions: String::new(),
                    composition: String::new(),
                    relevant_memories: Vec::new(),
                    prior_decisions: Vec::new(),
                    total_tokens: 0,
                },
                estimated_savings_pct: 0.0,
                hidden_by_clearance,
            });
        };

        // 2. Budget allocation: skill gets 40%, memories get 50%, decisions get 10%
        let skill_budget = (max_tokens as f32 * 0.40) as usize;
        let memory_budget = (max_tokens as f32 * 0.50) as usize;
        let _decision_budget = max_tokens.saturating_sub(skill_budget + memory_budget);

        // 3. Build a compact description index of available skills
        let mut system_instructions = String::from("Available skills:\n");
        for (_, s) in matches.iter().take(MAX_TIER1_SKILLS) {
            let marker = if s.name == skill.name { "*" } else { "-" };
            system_instructions.push_str(&format!("{} {}: {}\n", marker, s.name, s.description));
        }

        // 4. Eligible skills for the provenance stamp: trusted registry metadata
        //    only (name + package hash), never a skill body.
        let eligible: Vec<SkillEntry> = matches
            .iter()
            .take(MAX_TIER1_SKILLS)
            .map(|(_, s)| SkillEntry {
                name: s.name.clone(),
                content_hash: s.content_hash.clone(),
            })
            .collect();

        // 5. Gather scoped evidence and compose the bounded pack section. The
        //    authorized scope is the memory backend's own workspace: the
        //    caller's project narrows the query, it never widens access.
        let (relevant_memories, prior_decisions, composition) = if let Some(memory) = &self.memory {
            let gathered = self
                .gather_context(
                    memory,
                    &request.task,
                    memory_budget,
                    max_tokens,
                    request.project.as_deref(),
                    &eligible,
                )
                .await;
            (gathered.memories, gathered.decisions, gathered.composition)
        } else {
            (Vec::new(), Vec::new(), String::new())
        };

        // 6. Whole-output secret gate: a finding drops every untrusted-derived
        //    field, not just the composition.
        let (relevant_memories, prior_decisions, composition) =
            gate_output(relevant_memories, prior_decisions, composition);

        // 7. Calculate total tokens
        let instructions_tokens = system_instructions.split_whitespace().count();
        let memory_tokens: usize = relevant_memories
            .iter()
            .map(|m| m.summary.split_whitespace().count() + m.keywords.len())
            .sum();
        let decision_tokens: usize = prior_decisions
            .iter()
            .map(|d| d.split_whitespace().count())
            .sum();
        let total_tokens = instructions_tokens + memory_tokens + decision_tokens;

        // 8. Estimate savings
        let raw_tokens = skill.token_cost
            + relevant_memories.len() * 500 // Avg full doc ~500 tokens
            + prior_decisions.len() * 100;
        let estimated_savings_pct = if raw_tokens > 0 {
            ((raw_tokens.saturating_sub(total_tokens) as f32) / raw_tokens as f32) * 100.0
        } else {
            0.0
        };

        info!(
            skill = skill.name,
            confidence,
            total_tokens,
            savings_pct = estimated_savings_pct,
            "Skill dispatched"
        );

        Ok(SkillDispatchResult {
            skill_name: skill.name,
            skill_description: skill.description,
            confidence,
            context_pack: ContextPack {
                system_instructions,
                composition,
                relevant_memories,
                prior_decisions,
                total_tokens,
            },
            estimated_savings_pct,
            hidden_by_clearance,
        })
    }

    /// Gather scoped evidence for the task.
    ///
    /// The authorized scope is the memory backend's own workspace (D11): the
    /// caller's `project` only narrows the query inside it, and **both** ordinary
    /// memories and prior decisions are filtered by that scope — the legacy
    /// decision lookup filtered by kind only. Every retrieved item is then
    /// re-checked and fenced by the composer before it reaches the pack.
    async fn gather_context(
        &self,
        memory: &Arc<QmdMemory>,
        query: &str,
        max_memory_tokens: usize,
        budget: usize,
        project: Option<&str>,
        eligible: &[SkillEntry],
    ) -> GatheredContext {
        let vm = VirtualMemory::new(Arc::clone(memory), None);
        let max_entries = (max_memory_tokens / 80).clamp(3, 15); // ~80 tokens per reference

        let scope = match CompositionScope::new(memory.workspace_id(), project) {
            Ok(scope) => scope,
            Err(error) => {
                warn!(error = %error, "skill dispatch: unusable composition scope");
                return GatheredContext::default();
            }
        };

        // Ordinary memories: workspace-scoped, kind-unfiltered. The query
        // filter is only a hint; every retrieved record is kept only when its
        // own stored workspace equals the authorized one exactly.
        let ordinary_filters = Some(scope.filters(None));
        let entries = vm
            .page_in_filtered(query, max_entries, ordinary_filters.as_ref())
            .await
            .unwrap_or_default();

        let mut scoped: Vec<(MemoryReference, String)> = entries
            .iter()
            .filter_map(|entry| {
                let workspace = stored_workspace(&entry.metadata)?;
                (workspace == scope.workspace_id()).then(|| (entry.to_reference(), workspace))
            })
            .collect();

        // Trim to token budget
        let mut total_tokens = 0;
        scoped.retain(|(reference, _)| {
            let ref_tokens =
                reference.summary.split_whitespace().count() + reference.keywords.len();
            if total_tokens + ref_tokens <= max_memory_tokens {
                total_tokens += ref_tokens;
                true
            } else {
                false
            }
        });

        let references: Vec<MemoryReference> = scoped
            .iter()
            .map(|(reference, _)| reference.clone())
            .collect();
        let mut evidence: Vec<Evidence> = scoped
            .iter()
            .map(|(reference, workspace)| {
                Evidence::new(&reference.id, workspace, "memory", &reference.summary)
            })
            .collect();

        // Prior decisions: same workspace scope, narrowed to the decision kind,
        // then re-checked against each record's stored workspace.
        let decision_filters =
            scope.filters(Some(vec![crate::memory::schema::MemoryKind::Decision]));
        let mut decisions: Vec<String> = Vec::new();
        for doc in memory
            .search_filtered(query, 5, Some(&decision_filters))
            .await
            .unwrap_or_default()
        {
            let Some(workspace) = stored_workspace(&doc.metadata) else {
                continue;
            };
            if workspace != scope.workspace_id() {
                continue;
            }
            // First line only, bounded: a compact decision reference.
            let text = doc
                .content
                .lines()
                .next()
                .unwrap_or(&doc.content)
                .chars()
                .take(150)
                .collect::<String>();
            evidence.push(Evidence::new(
                &format!("decision-{}", decisions.len()),
                &workspace,
                "decision",
                &text,
            ));
            decisions.push(text);
        }

        let composition = compose_pack(&scope, query, eligible, &evidence, budget);
        GatheredContext {
            memories: references,
            decisions,
            composition,
        }
    }

    /// Get a reference to the skill registry.
    pub fn registry(&self) -> &SkillRegistry {
        &self.registry
    }

    /// Get a mutable reference to the skill registry (for reindexing).
    pub fn registry_mut(&mut self) -> &mut SkillRegistry {
        &mut self.registry
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_pack_serializes() {
        let pack = ContextPack {
            system_instructions: "Available skills:\n* do-the-thing: Do the thing".to_string(),
            composition: String::new(),
            relevant_memories: vec![MemoryReference {
                id: "m1".to_string(),
                path: "test/path".to_string(),
                summary: "A test memory".to_string(),
                keywords: vec!["test".to_string()],
            }],
            prior_decisions: vec!["Use clustering".to_string()],
            total_tokens: 42,
        };

        let json = serde_json::to_string(&pack).unwrap();
        assert!(json.contains("do-the-thing"));
        assert!(json.contains("test memory"));
    }

    #[test]
    fn dispatch_result_serializes() {
        let result = SkillDispatchResult {
            skill_name: "test-skill".to_string(),
            skill_description: "A test".to_string(),
            confidence: 0.85,
            context_pack: ContextPack {
                system_instructions: "Available skills:\n* test-skill: A test".to_string(),
                composition: String::new(),
                relevant_memories: Vec::new(),
                prior_decisions: Vec::new(),
                total_tokens: 1,
            },
            estimated_savings_pct: 73.2,
            hidden_by_clearance: 0,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("test-skill"));
        assert!(json.contains("73.2"));
    }

    #[tokio::test]
    async fn dispatch_returns_no_match_below_confidence_threshold() {
        // Prepare a registry with a dummy skill
        // Need to add through a file because skills is private. Let's use a temp dir.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("skills");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            "---\nname: unrelated-skill\ndescription: \"This does some other stuff\"\n---\n\nContent",
        )
        .unwrap();
        let mut registry = SkillRegistry::new(vec![dir]);
        registry.reindex().await.unwrap();

        let dispatcher = SkillDispatcher::new(registry, None);
        // "very weak query" should match weakly or not at all
        let req = SkillDispatchRequest {
            task: "very weak query".to_string(),
            model_hint: None,
            max_tokens: Some(100),
            project: None,
        };

        let result = dispatcher.dispatch(&req).await.unwrap();
        // It must fallback to _none because the confidence is below 0.40
        assert_eq!(result.skill_name, "_none");
        assert_eq!(result.confidence, 0.0);
    }

    #[tokio::test]
    async fn dispatch_never_reports_full_confidence_for_weak_match() {
        // Note: For now we test dispatch behavior to not pass through 1.0 confidence for weak matches implicitly
        // by making sure we get _none for weak queries.
        // A true test for score bounding is more appropriate on the registry score function.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("skills");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            "---\nname: weak-skill\ndescription: \"weak\"\n---\n\nContent",
        )
        .unwrap();
        let mut registry = SkillRegistry::new(vec![dir]);
        registry.reindex().await.unwrap();

        let dispatcher = SkillDispatcher::new(registry, None);
        let req = SkillDispatchRequest {
            task: "weak".to_string(),
            model_hint: None,
            max_tokens: Some(100),
            project: None,
        };
        // By using a weak exact term match "weak" to "weak" we might get a match.
        // We want to verify it doesn't give 1.0 confidence for it implicitly.
        let result = dispatcher.dispatch(&req).await.unwrap();
        assert!(result.confidence < 0.999);
    }

    /// Documents stored in two workspaces: only the authorized one may reach the
    /// pack, for ordinary memories and for prior decisions alike (D11).
    #[tokio::test]
    async fn dispatch_scopes_memories_and_decisions_to_the_memory_workspace() {
        let result = dispatch_fixture(None).await;
        assert_eq!(result.skill_name, "trace-audit");

        let composition = &result.context_pack.composition;
        assert!(
            composition.contains("m-in") && composition.contains("d-in"),
            "in-scope evidence must reach the composition: {composition}"
        );
        assert!(!composition.contains("m-out"), "out-of-scope memory leaked");
        assert!(
            !composition.contains("d-out"),
            "out-of-scope decision leaked"
        );
        assert!(
            !composition.contains("ws-b"),
            "another workspace reached the pack"
        );
        assert!(
            !result
                .context_pack
                .relevant_memories
                .iter()
                .any(|m| m.id == "m-out"),
            "out-of-scope memory reached the memory list"
        );
        assert!(
            !result
                .context_pack
                .prior_decisions
                .iter()
                .any(|d| d.contains("drop the lexical ranking guard")),
            "the decision lookup still filters by kind only"
        );
    }

    /// A caller-supplied project is data, not authorization: naming another
    /// workspace widens nothing.
    #[tokio::test]
    async fn dispatch_does_not_treat_the_caller_project_as_authorization() {
        let result = dispatch_fixture(Some("ws-b")).await;
        let composition = &result.context_pack.composition;
        assert!(!composition.contains("m-out"), "project widened the scope");
        assert!(!composition.contains("d-out"), "project widened the scope");
        assert!(
            !composition.contains("ws-b"),
            "another workspace reached the pack"
        );
        assert!(
            !result
                .context_pack
                .prior_decisions
                .iter()
                .any(|d| d.contains("drop the lexical ranking guard")),
            "the decision lookup still filters by kind only"
        );
    }

    /// A stored workspace that only differs in case from the authorized one is
    /// not the authorized workspace: the retrieval filter is case-insensitive,
    /// the scope boundary is not.
    #[tokio::test]
    async fn dispatch_excludes_a_case_variant_namespace() {
        let result = dispatch_fixture(None).await;
        let composition = &result.context_pack.composition;
        assert!(
            !composition.contains("CASELEAK-MARKER"),
            "case-variant memory leaked: {composition}"
        );
        assert!(
            !composition.contains("CASELEAK-DECISION"),
            "case-variant decision leaked: {composition}"
        );
        assert!(
            !result
                .context_pack
                .relevant_memories
                .iter()
                .any(|m| m.summary.contains("CASELEAK-MARKER")),
            "case-variant memory reached the memory list"
        );
        assert!(
            !result
                .context_pack
                .prior_decisions
                .iter()
                .any(|d| d.contains("CASELEAK-DECISION")),
            "case-variant decision reached the decision list"
        );
    }

    /// A record whose only workspace is a top-level `workspace_id` is resolved
    /// like `resolution.rs` resolves it; here that is `ws-b`, not the authorized
    /// `ws-a` the pool stamped on the query.
    #[tokio::test]
    async fn dispatch_excludes_a_top_level_workspace_id_without_namespace() {
        let result = dispatch_fixture(None).await;
        let composition = &result.context_pack.composition;
        assert!(
            !composition.contains("TOPLEAK-MARKER"),
            "top-level workspace memory leaked: {composition}"
        );
        assert!(
            !composition.contains("TOPLEAK-DECISION"),
            "top-level workspace decision leaked: {composition}"
        );
        assert!(
            !result
                .context_pack
                .relevant_memories
                .iter()
                .any(|m| m.summary.contains("TOPLEAK-MARKER")),
            "top-level workspace memory reached the memory list"
        );
        assert!(
            !result
                .context_pack
                .prior_decisions
                .iter()
                .any(|d| d.contains("TOPLEAK-DECISION")),
            "top-level workspace decision reached the decision list"
        );
    }

    /// The whole-output gate covers every delivered field, not just the
    /// composition: a secret in any of them blocks all of them.
    #[tokio::test]
    async fn dispatch_blocks_a_secret_in_any_delivered_field() {
        let leaked = "sk-or-v1-".to_string() + &"A".repeat(24);
        let result = dispatch_secret_fixture(&leaked).await;
        let pack = &result.context_pack;
        assert!(
            !pack.composition.contains(&leaked),
            "the secret reached the composition"
        );
        assert!(
            !pack
                .relevant_memories
                .iter()
                .any(|m| m.summary.contains(&leaked)),
            "the secret reached the memory list"
        );
        assert!(
            !pack.prior_decisions.iter().any(|d| d.contains(&leaked)),
            "the secret reached the decision list"
        );
        assert!(pack.composition.is_empty());
        assert!(pack.relevant_memories.is_empty());
    }

    /// Seed one memory backend holding two workspaces and dispatch a task that
    /// matches the registered skill.
    async fn dispatch_fixture(project: Option<&str>) -> SkillDispatchResult {
        use crate::memory::qmd_memory::{MemoryDocument, QmdMemory};

        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("skills");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            "---\nname: trace-audit\ndescription: \"Audit the failed traces of a run\"\n---\n\nContent",
        )
        .unwrap();
        let mut registry = SkillRegistry::new(vec![dir]);
        registry.reindex().await.unwrap();

        let document = |id: &str, workspace: &str, kind: &str, body: &str| MemoryDocument {
            id: Some(id.to_string()),
            path: format!("sessions/{id}"),
            content: body.to_string(),
            metadata: serde_json::json!({
                "kind": kind,
                "namespace": { "workspace_id": workspace }
            }),
            ..Default::default()
        };
        let documents = Arc::new(tokio::sync::RwLock::new(vec![
            document(
                "m-in",
                "ws-a",
                "observation",
                "trace audit insight inside the workspace",
            ),
            document(
                "m-out",
                "ws-b",
                "observation",
                "trace audit insight outside the workspace",
            ),
            document(
                "m-case",
                "WS-A",
                "observation",
                "CASELEAK-MARKER trace audit insight in a case-variant workspace",
            ),
            document(
                "d-case",
                "WS-A",
                "decision",
                "CASELEAK-DECISION trace audit case-variant decision",
            ),
            document(
                "d-in",
                "ws-a",
                "decision",
                "we keep lexical ranking for traces",
            ),
            document(
                "d-out",
                "ws-b",
                "decision",
                "we drop the lexical ranking guard",
            ),
            MemoryDocument {
                id: Some("m-toplevel".to_string()),
                path: "sessions/m-toplevel".to_string(),
                content: "TOPLEAK-MARKER trace audit insight declared at the top level".to_string(),
                metadata: serde_json::json!({
                    "kind": "observation",
                    "workspace_id": "ws-b"
                }),
                ..Default::default()
            },
            MemoryDocument {
                id: Some("d-toplevel".to_string()),
                path: "sessions/d-toplevel".to_string(),
                content: "TOPLEAK-DECISION trace audit top-level decision".to_string(),
                metadata: serde_json::json!({
                    "kind": "decision",
                    "workspace_id": "ws-b"
                }),
                ..Default::default()
            },
        ]));
        let dispatcher = SkillDispatcher::new(
            registry,
            Some(Arc::new(QmdMemory::new_with_workspace(documents, "ws-a"))),
        );

        let req = SkillDispatchRequest {
            task: "audit the failed traces".to_string(),
            model_hint: None,
            max_tokens: Some(4000),
            project: project.map(str::to_string),
        };
        dispatcher.dispatch(&req).await.unwrap()
    }

    /// A single authorized-workspace memory whose body holds `secret`.
    async fn dispatch_secret_fixture(secret: &str) -> SkillDispatchResult {
        use crate::memory::qmd_memory::{MemoryDocument, QmdMemory};

        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("skills");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            "---\nname: trace-audit\ndescription: \"Audit the failed traces of a run\"\n---\n\nContent",
        )
        .unwrap();
        let mut registry = SkillRegistry::new(vec![dir]);
        registry.reindex().await.unwrap();

        let documents = Arc::new(tokio::sync::RwLock::new(vec![MemoryDocument {
            id: Some("m-secret".to_string()),
            path: "sessions/m-secret".to_string(),
            content: format!("trace audit {secret}"),
            metadata: serde_json::json!({
                "kind": "observation",
                "namespace": { "workspace_id": "ws-a" }
            }),
            ..Default::default()
        }]));
        let dispatcher = SkillDispatcher::new(
            registry,
            Some(Arc::new(QmdMemory::new_with_workspace(documents, "ws-a"))),
        );
        let req = SkillDispatchRequest {
            task: "audit the failed traces".to_string(),
            model_hint: None,
            max_tokens: Some(4000),
            project: None,
        };
        dispatcher.dispatch(&req).await.unwrap()
    }
}
