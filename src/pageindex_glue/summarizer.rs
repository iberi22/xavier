//! `Summarizer` adapter over the RAG LLM adapter (Ollama / OpenAI-compatible).
//!
//! The summarizer is blocking: it must run on a blocking thread (the ingest
//! runs inside `spawn_blocking`), never directly on a Tokio worker.

use std::sync::Arc;

use tokio::runtime::Handle;
use xavier_pageindex::summarize::Summarizer;
use xavier_pageindex::PageIndexError;

use super::settings::PageIndexSettings;
use crate::rag::llm_adapter::{LlmAdapter, LlmAdapterTrait, LlmBackend, LlmConfig};

pub struct LlmSummarizer {
    llm: Arc<dyn LlmAdapterTrait>,
    handle: Handle,
    max_words: usize,
}

impl LlmSummarizer {
    pub fn new(llm: Arc<dyn LlmAdapterTrait>, handle: Handle, max_words: usize) -> Self {
        Self {
            llm,
            handle,
            max_words,
        }
    }

    /// `None` (no LLM call ever) when summaries are disabled, the backend is
    /// `RetrievalOnly`, or there is no Tokio runtime to drive the adapter.
    pub fn from_settings(settings: &PageIndexSettings, cfg: LlmConfig) -> Option<Self> {
        if !settings.summarize || cfg.backend == LlmBackend::RetrievalOnly {
            return None;
        }
        let handle = Handle::try_current().ok()?;
        Some(Self::new(
            Arc::new(LlmAdapter::from_config(cfg)),
            handle,
            settings.summary_max_words,
        ))
    }

    /// Same as [`Self::from_settings`], reading the `DOCBOT_LLM_*` env.
    pub fn from_env(settings: &PageIndexSettings) -> Option<Self> {
        Self::from_settings(settings, LlmConfig::from_env())
    }

    fn prompt(&self, title: &str, text: &str) -> String {
        format!(
            "You are summarizing one section of a document for a retrieval index.\n\
             Section title: {title}\n\n\
             Content:\n{text}\n\n\
             Write a description of what this section covers in at most {} words. \
             Use only the content above. Reply with the description only.",
            self.max_words
        )
    }
}

impl Summarizer for LlmSummarizer {
    fn model(&self) -> &str {
        self.llm.model_name()
    }

    fn summarize(&self, title: &str, text: &str) -> Result<String, PageIndexError> {
        let prompt = self.prompt(title, text);
        self.handle
            .block_on(self.llm.complete(&prompt))
            .map(|s| s.trim().to_string())
            .map_err(|e| PageIndexError::Build(format!("summary failed: {e}")))
    }

    fn complete(&self, prompt: &str) -> Result<String, PageIndexError> {
        self.handle
            .block_on(self.llm.complete(prompt))
            .map(|s| s.trim().to_string())
            .map_err(|e| PageIndexError::Build(format!("completion failed: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;

    struct FakeLlm {
        prompts: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl LlmAdapterTrait for FakeLlm {
        async fn complete(&self, prompt: &str) -> anyhow::Result<String> {
            self.prompts.lock().unwrap().push(prompt.to_string());
            Ok("  a summary \n".into())
        }
        fn model_name(&self) -> &str {
            "fake-model"
        }
    }

    fn settings(summarize: bool) -> PageIndexSettings {
        PageIndexSettings {
            summarize,
            ..Default::default()
        }
    }

    #[test]
    fn test_summarizer_adapter_uses_llm_config() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _g = rt.enter();
        let cfg = |backend| LlmConfig {
            backend,
            ..Default::default()
        };
        // No LLM call ever: disabled setting or retrieval-only backend.
        assert!(LlmSummarizer::from_settings(&settings(false), cfg(LlmBackend::Ollama)).is_none());
        assert!(
            LlmSummarizer::from_settings(&settings(true), cfg(LlmBackend::RetrievalOnly)).is_none()
        );
        let s = LlmSummarizer::from_settings(&settings(true), cfg(LlmBackend::Ollama)).unwrap();
        assert_eq!(s.model(), LlmConfig::default().model);

        // Summaries go through the adapter from a non-runtime thread.
        let fake = Arc::new(FakeLlm {
            prompts: Mutex::new(Vec::new()),
        });
        let s = LlmSummarizer::new(fake.clone(), rt.handle().clone(), 40);
        let out = std::thread::spawn(move || s.summarize("Intro", "body text"))
            .join()
            .unwrap()
            .unwrap();
        assert_eq!(out, "a summary");
        let p = fake.prompts.lock().unwrap()[0].clone();
        assert!(p.contains("Intro") && p.contains("body text") && p.contains("40 words"));
    }

    #[test]
    fn test_summarizer_complete_sends_prompt_unwrapped() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let fake = Arc::new(FakeLlm {
            prompts: Mutex::new(Vec::new()),
        });
        let s = LlmSummarizer::new(fake.clone(), rt.handle().clone(), 40);
        let out = std::thread::spawn(move || s.complete("TASK: raw prompt"))
            .join()
            .unwrap()
            .unwrap();
        assert_eq!(out, "a summary");
        assert_eq!(fake.prompts.lock().unwrap()[0], "TASK: raw prompt");
    }
}
