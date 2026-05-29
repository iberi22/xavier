//! Auto-Embedder selection engine
//!
//! Picks the optimal embedding provider based on system capabilities
//! and available external providers.

use crate::onboarding::scanner::{ProviderStatus, SystemCapabilities};

/// Select the best available embedding provider
///
/// Priority:
/// 1. GLLM local (if compiled & system has AVX2)
/// 2. Ollama (local, if running)
/// 3. OpenAI (if API key available)
/// 4. Google Gemini (if API key available)
/// 5. Local embed server (if running)
/// 6. GLLM local (even without AVX2 — slower but works)
/// 7. No-op (last resort, degraded mode)
pub fn select_best_embedder(system: &SystemCapabilities, providers: &[ProviderStatus]) -> String {
    // Helper: find provider by name
    let find = |name: &str| providers.iter().find(|p| p.name == name);

    // Priority 1: GLLM local with AVX2 — fastest
    if let Some(gllm) = find("gllm-local") {
        if gllm.available && system.has_avx2 {
            return "gllm (local, AVX2)".into();
        }
    }

    // Priority 2: Ollama
    if let Some(ollama) = find("ollama") {
        if ollama.available {
            return "ollama".into();
        }
    }

    // Priority 3: OpenAI
    if let Some(openai) = find("openai") {
        if openai.available {
            return "openai (text-embedding-3-small)".into();
        }
    }

    // Priority 4: Google Gemini
    if let Some(google) = find("google-gemini") {
        if google.available {
            return "google-gemini (text-embedding-004)".into();
        }
    }

    // Priority 5: Local embed server
    if let Some(local) = find("local-embed-server") {
        if local.available {
            return "local-embed-server".into();
        }
    }

    // Priority 6: GLLM local even without AVX2
    if let Some(gllm) = find("gllm-local") {
        if gllm.available {
            return "gllm (local, fallback)".into();
        }
    }

    // Priority 7: No-op (everything failed)
    "noop".into()
}

/// The AutoEmbedder is a factory that wraps the embedder selection
pub struct AutoEmbedder;

impl AutoEmbedder {
    /// Select best embedder and return display name
    pub fn select(system: &SystemCapabilities, providers: &[ProviderStatus]) -> String {
        select_best_embedder(system, providers)
    }

    /// Returns a human-readable recommendation
    pub fn recommend(system: &SystemCapabilities, providers: &[ProviderStatus]) -> String {
        let embedder = Self::select(system, providers);

        match embedder.as_str() {
            s if s.starts_with("gllm") => {
                format!(
                    "✅ GLLM local — embeddings on-device ({}). No config needed.",
                    if system.has_avx2 { "AVX2 accelerated" } else { "CPU fallback" }
                )
            }
            "ollama" => {
                "✅ Ollama local detected. Configure: `embedding_provider: ollama`".into()
            }
            s if s.starts_with("openai") => {
                "🔑 OpenAI API detected. Configure: `embedding_provider: openai`".into()
            }
            s if s.starts_with("google") => {
                "🔑 Google Gemini API detected. Configure: `embedding_provider: google`".into()
            }
            "local-embed-server" => {
                "✅ Local embed server detected. Configure: `embedding_provider: custom`".into()
            }
            _ => {
                "⚠️  No embedding provider available. Xavier will run in DEGRADED mode (no embeddings). Install Ollama or set OPENAI_API_KEY.".into()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::onboarding::scanner::{ProviderStatus, SystemCapabilities};

    fn make_system(avx2: bool) -> SystemCapabilities {
        SystemCapabilities {
            cpu_cores: 8,
            ram_gb: 16.0,
            disk_free_gb: 100.0,
            has_avx2: avx2,
            has_avx512: false,
            has_cuda: false,
            has_vulkan: false,
            has_metal: false,
            has_nvidia_gpu: false,
            is_windows: true,
            is_linux: false,
            is_macos: false,
        }
    }

    fn make_providers(names: &[&str]) -> Vec<ProviderStatus> {
        names
            .iter()
            .map(|&name| ProviderStatus {
                name: name.to_string(),
                available: true,
                reason: String::new(),
            })
            .collect()
    }

    #[test]
    fn test_gllm_avx2_is_priority() {
        let system = make_system(true);
        let providers = make_providers(&["gllm-local", "ollama", "openai"]);
        let result = select_best_embedder(&system, &providers);
        assert!(result.starts_with("gllm"), "Expected gllm, got: {result}");
    }

    #[test]
    fn test_ollama_fallback() {
        let system = make_system(false);
        let providers = make_providers(&["gllm-local", "ollama"]);
        let result = select_best_embedder(&system, &providers);
        assert_eq!(result, "ollama", "Expected ollama, got: {result}");
    }

    #[test]
    fn test_noop_last_resort() {
        let system = make_system(false);
        let providers = vec![];
        let result = select_best_embedder(&system, &providers);
        assert_eq!(result, "noop", "Expected noop, got: {result}");
    }
}
