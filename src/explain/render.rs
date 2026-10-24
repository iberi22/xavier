//! Prompt building and answer rendering for code explanations.
//!
//! The LLM is reached through [`ExplanationModel`]; the HTTP route supplies the
//! implementation. The answer is rendered as OpenUI markup by the UI agent.

use std::collections::HashSet;

use anyhow::Result;
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};

use super::context::ExplainContext;
use crate::agents::ui_render::UiRenderAgent;

/// Completion port for the explanation LLM.
pub trait ExplanationModel: Send + Sync {
    fn complete<'a>(&'a self, system: &'a str, user: &'a str) -> BoxFuture<'a, Result<String>>;
}

/// Who the explanation is written for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Audience {
    #[default]
    Newcomer,
    Maintainer,
}

/// A rendered explanation with its citation check.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Explanation {
    pub plain_text: String,
    pub openui_lang: String,
    /// Cited refs that match evidence, in order of first appearance.
    pub cited_refs: Vec<String>,
    /// Cited refs that match no evidence. Non-empty suggests hallucination.
    pub unknown_refs: Vec<String>,
    /// `(source name, reason)` for each source that failed while assembling.
    pub skipped_sources: Vec<(String, String)>,
}

const SYSTEM_RULES: &str = "You explain a code fragment from the Xavier repository. \
Answer ONLY from the evidence provided. Cite evidence inline by putting its reference \
in square brackets exactly as given, for example [src/lib.rs:2-3]. If the evidence does \
not cover something, say so explicitly instead of guessing. Use these markdown sections: \
What it does; Why it is this way (decisions and commits); What calls it and what it \
calls; Risks.";

fn audience_guidance(audience: Audience) -> &'static str {
    match audience {
        Audience::Newcomer => {
            "The reader is new to this codebase: define terms and explain the role of the code in plain language."
        }
        Audience::Maintainer => {
            "The reader maintains this codebase: be precise about invariants, callers and change risks."
        }
    }
}

/// Builds the `(system, user)` prompts for `ctx`.
///
/// The user prompt names the target and lists the evidence in the order it was
/// assembled, each under its `[source_ref]`.
pub fn build_prompt(ctx: &ExplainContext, audience: Audience) -> (String, String) {
    let system = format!("{SYSTEM_RULES}\n\n{}", audience_guidance(audience));

    let target = &ctx.target;
    let mut user = format!(
        "Explain the code at {}:{}-{}",
        target.file.display(),
        target.start_line,
        target.end_line
    );
    if let Some(symbol) = &target.symbol {
        user.push_str(&format!(" (symbol `{symbol}`)"));
    }
    user.push_str(".\n\nEvidence, in order:\n");
    for evidence in &ctx.evidence {
        user.push_str(&format!(
            "\n[{}] ({:?})\n{}\n",
            evidence.source_ref, evidence.kind, evidence.text
        ));
    }
    if !ctx.skipped_sources.is_empty() {
        user.push_str("\nSources that failed, so their evidence is missing:\n");
        for (name, reason) in &ctx.skipped_sources {
            user.push_str(&format!("- {name}: {reason}\n"));
        }
    }

    (system, user)
}

/// Asks `model` to explain `ctx` and renders the answer.
///
/// Errors come only from the model. Citations are checked against the evidence
/// refs: matching ones go to `cited_refs`, the rest to `unknown_refs`.
pub async fn explain(
    ctx: &ExplainContext,
    model: &dyn ExplanationModel,
    audience: Audience,
) -> Result<Explanation> {
    let (system, user) = build_prompt(ctx, audience);
    let answer = model.complete(&system, &user).await?;

    let known: HashSet<&str> = ctx.evidence.iter().map(|e| e.source_ref.as_str()).collect();
    let (cited_refs, unknown_refs) = split_citations(&answer, &known);
    let rendered = UiRenderAgent::new().render_answer(&answer);

    Ok(Explanation {
        plain_text: rendered.plain_text,
        openui_lang: rendered.openui_lang,
        cited_refs,
        unknown_refs,
        skipped_sources: ctx.skipped_sources.clone(),
    })
}

/// Splits the `[ref]` citations in `answer` into known and unknown refs.
/// Each ref appears once, in order of first appearance.
fn split_citations(answer: &str, known: &HashSet<&str>) -> (Vec<String>, Vec<String>) {
    let mut cited = Vec::new();
    let mut unknown = Vec::new();
    for reference in bracketed(answer) {
        let bucket = if known.contains(reference) {
            &mut cited
        } else {
            &mut unknown
        };
        if !bucket.iter().any(|r| r == reference) {
            bucket.push(reference.to_string());
        }
    }
    (cited, unknown)
}

/// Yields the trimmed text of each `[...]` pair that is not empty.
fn bracketed(text: &str) -> impl Iterator<Item = &str> {
    text.split('[')
        .skip(1)
        .filter_map(|chunk| chunk.split_once(']').map(|(inner, _)| inner.trim()))
        .filter(|inner| !inner.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::explain::context::{Evidence, EvidenceKind, ExplainTarget};
    use std::path::PathBuf;
    use std::sync::Mutex;

    /// Returns a fixed answer, or fails when `fail` is set.
    #[derive(Default)]
    struct FakeModel {
        answer: String,
        fail: bool,
        seen: Mutex<Vec<(String, String)>>,
    }

    impl ExplanationModel for FakeModel {
        fn complete<'a>(&'a self, system: &'a str, user: &'a str) -> BoxFuture<'a, Result<String>> {
            Box::pin(async move {
                self.seen
                    .lock()
                    .unwrap()
                    .push((system.to_string(), user.to_string()));
                if self.fail {
                    anyhow::bail!("model unavailable");
                }
                Ok(self.answer.clone())
            })
        }
    }

    fn context() -> ExplainContext {
        ExplainContext {
            target: ExplainTarget {
                repo_root: PathBuf::new(),
                file: PathBuf::from("src/lib.rs"),
                start_line: 2,
                end_line: 3,
                symbol: Some("demo".to_string()),
            },
            evidence: vec![
                Evidence {
                    kind: EvidenceKind::Code,
                    source_ref: "src/lib.rs:2-3".to_string(),
                    text: "fn demo() {}".to_string(),
                    score: f32::MAX,
                },
                Evidence {
                    kind: EvidenceKind::Commit,
                    source_ref: "abc123".to_string(),
                    text: "2026-10-01 add demo".to_string(),
                    score: 0.5,
                },
                Evidence {
                    kind: EvidenceKind::Decision,
                    source_ref: "dec-7".to_string(),
                    text: "keep demo tiny".to_string(),
                    score: 0.9,
                },
            ],
            skipped_sources: vec![("git-history".to_string(), "git log failed".to_string())],
        }
    }

    #[test]
    fn system_prompt_restricts_answers_to_evidence() {
        let (system, _) = build_prompt(&context(), Audience::Newcomer);

        assert!(system.contains("ONLY from the evidence"));
        assert!(system.contains("square brackets"));
    }

    #[test]
    fn user_prompt_lists_each_evidence_ref_in_order() {
        let (_, user) = build_prompt(&context(), Audience::Maintainer);

        let positions: Vec<usize> = ["src/lib.rs:2-3", "abc123", "dec-7"]
            .iter()
            .map(|r| user.find(&format!("[{r}]")).expect("ref present in prompt"))
            .collect();
        assert!(positions.windows(2).all(|w| w[0] < w[1]));
        assert!(user.contains("git-history: git log failed"));
    }

    #[tokio::test]
    async fn hallucinated_citations_are_reported_as_unknown() {
        let model = FakeModel {
            answer: "Runs each tick [src/lib.rs:2-3] and keeps state [dec-7]. \
                     It also cites [dec-99] and [ghost:1-2]. Again [dec-7]."
                .to_string(),
            ..Default::default()
        };

        let out = explain(&context(), &model, Audience::Newcomer)
            .await
            .unwrap();

        assert_eq!(out.cited_refs, vec!["src/lib.rs:2-3", "dec-7"]);
        assert_eq!(out.unknown_refs, vec!["dec-99", "ghost:1-2"]);
    }

    #[tokio::test]
    async fn openui_lang_is_rendered_for_multi_section_answer() {
        let answer =
            "## What it does\nDemo returns early [src/lib.rs:2-3].\n\n## Risks\nNone known.";
        let model = FakeModel {
            answer: answer.to_string(),
            ..Default::default()
        };

        let out = explain(&context(), &model, Audience::Maintainer)
            .await
            .unwrap();

        assert!(!out.openui_lang.is_empty());
        assert!(out.openui_lang.contains("SectionBlock"));
        assert_eq!(out.plain_text, answer);
    }

    #[tokio::test]
    async fn skipped_sources_are_carried_through() {
        let model = FakeModel {
            answer: "Nothing cited.".to_string(),
            ..Default::default()
        };

        let out = explain(&context(), &model, Audience::Newcomer)
            .await
            .unwrap();

        assert_eq!(out.skipped_sources, context().skipped_sources);
        assert!(out.cited_refs.is_empty());
        assert!(out.unknown_refs.is_empty());
    }

    #[tokio::test]
    async fn model_failure_is_returned_as_error() {
        let model = FakeModel {
            fail: true,
            ..Default::default()
        };

        let err = explain(&context(), &model, Audience::Newcomer)
            .await
            .unwrap_err();

        assert!(format!("{err:#}").contains("model unavailable"));
        assert_eq!(model.seen.lock().unwrap().len(), 1);
    }
}
