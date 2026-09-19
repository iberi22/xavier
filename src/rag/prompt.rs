//! Prompt templates for Xavier DocBot RAG.
//!
//! Provides locale-aware prompt assembly that formats retrieved document
//! chunks into a structured context block for the LLM.

use crate::collections::CitedSource;

/// Supported prompt languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PromptLanguage {
    #[default]
    Spanish,
    English,
}

impl PromptLanguage {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "en" | "english" => Self::English,
            _ => Self::Spanish,
        }
    }
}

/// Builds prompts for RAG completions.
pub struct PromptBuilder {
    pub language: PromptLanguage,
    /// Maximum characters of context to include per chunk.
    pub max_chunk_chars: usize,
    /// Maximum total context characters.
    pub max_context_chars: usize,
}

impl Default for PromptBuilder {
    fn default() -> Self {
        Self {
            language: PromptLanguage::Spanish,
            max_chunk_chars: 600,
            max_context_chars: 4000,
        }
    }
}

impl PromptBuilder {
    pub fn new(language: PromptLanguage) -> Self {
        Self {
            language,
            ..Default::default()
        }
    }

    /// Build the full RAG prompt from query + retrieved chunks.
    pub fn build(&self, query: &str, chunks: &[RetrievedChunk]) -> String {
        let context = self.build_context(chunks);

        match self.language {
            PromptLanguage::Spanish => format!(
                "Eres un asistente experto en gestión documental. \
                Responde la pregunta del usuario ÚNICAMENTE con la información \
                proporcionada en el contexto. Si la respuesta no está en el \
                contexto, di \"No encontré información sobre eso en los documentos disponibles.\"\n\n\
                CONTEXTO:\n{context}\n\n\
                PREGUNTA: {query}\n\n\
                RESPUESTA (en español, citando el documento cuando sea relevante):"
            ),
            PromptLanguage::English => format!(
                "You are an expert document management assistant. \
                Answer the user's question ONLY using the information provided in the context. \
                If the answer is not in the context, say \"I could not find information about that in the available documents.\"\n\n\
                CONTEXT:\n{context}\n\n\
                QUESTION: {query}\n\n\
                ANSWER (cite the document when relevant):"
            ),
        }
    }

    /// Build context block from retrieved chunks (with citations).
    fn build_context(&self, chunks: &[RetrievedChunk]) -> String {
        let mut context = String::new();
        let mut total_chars = 0;

        for (i, chunk) in chunks.iter().enumerate() {
            if total_chars >= self.max_context_chars {
                break;
            }

            let content = truncate_str(&chunk.content, self.max_chunk_chars);
            let mut citation = format!("[{}] {}", i + 1, chunk.doc_title);

            if let Some(page) = chunk.page {
                citation.push_str(&format!(" (p. {})", page));
            }
            if let Some(ref crumb) = chunk.breadcrumb {
                citation.push_str(&format!(" › {}", crumb));
            }

            let entry = format!("{}\n{}\n\n", citation, content);
            total_chars += entry.len();
            context.push_str(&entry);
        }

        if context.is_empty() {
            match self.language {
                PromptLanguage::Spanish => {
                    context = "No se encontraron documentos relevantes.".to_string()
                }
                PromptLanguage::English => context = "No relevant documents found.".to_string(),
            }
        }

        context
    }
}

/// A retrieved chunk ready for prompt assembly.
#[derive(Debug, Clone)]
pub struct RetrievedChunk {
    pub chunk_id: String,
    pub doc_id: String,
    pub doc_title: String,
    pub collection_id: String,
    pub collection_name: String,
    pub content: String,
    pub page: Option<u32>,
    pub breadcrumb: Option<String>,
    pub score: f32,
}

impl RetrievedChunk {
    /// Convert to a `CitedSource` for the API response.
    pub fn to_cited_source(&self, excerpt_len: usize) -> CitedSource {
        CitedSource {
            chunk_id: self.chunk_id.clone(),
            doc_id: self.doc_id.clone(),
            doc_title: self.doc_title.clone(),
            collection_id: self.collection_id.clone(),
            collection_name: self.collection_name.clone(),
            page: self.page,
            breadcrumb: self.breadcrumb.clone(),
            excerpt: truncate_str(&self.content, excerpt_len).to_string(),
            score: self.score,
        }
    }
}

fn truncate_str(s: &str, max_chars: usize) -> &str {
    if s.chars().count() <= max_chars {
        return s;
    }
    let mut idx = 0;
    for (count, (i, _)) in s.char_indices().enumerate() {
        if count >= max_chars {
            return &s[..idx];
        }
        idx = i;
    }
    s
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_chunk(content: &str) -> RetrievedChunk {
        RetrievedChunk {
            chunk_id: "c1".to_string(),
            doc_id: "d1".to_string(),
            doc_title: "Contrato 2024".to_string(),
            collection_id: "col1".to_string(),
            collection_name: "Legal".to_string(),
            content: content.to_string(),
            page: Some(3),
            breadcrumb: Some("Artículo 5".to_string()),
            score: 0.95,
        }
    }

    #[test]
    fn test_spanish_prompt_contains_keywords() {
        let builder = PromptBuilder::new(PromptLanguage::Spanish);
        let chunk = sample_chunk("El arrendatario pagará mensualmente.");
        let prompt = builder.build("¿Quién paga?", &[chunk]);

        assert!(prompt.contains("CONTEXTO:"));
        assert!(prompt.contains("PREGUNTA: ¿Quién paga?"));
        assert!(prompt.contains("Contrato 2024"));
        assert!(prompt.contains("p. 3"));
    }

    #[test]
    fn test_english_prompt() {
        let builder = PromptBuilder::new(PromptLanguage::English);
        let chunk = sample_chunk("The tenant shall pay monthly.");
        let prompt = builder.build("Who pays?", &[chunk]);

        assert!(prompt.contains("CONTEXT:"));
        assert!(prompt.contains("QUESTION: Who pays?"));
    }

    #[test]
    fn test_empty_chunks_context() {
        let builder = PromptBuilder::new(PromptLanguage::Spanish);
        let prompt = builder.build("¿Algo?", &[]);
        assert!(prompt.contains("No se encontraron documentos relevantes."));
    }

    #[test]
    fn test_to_cited_source() {
        let chunk = sample_chunk("Contenido del artículo cinco.");
        let cited = chunk.to_cited_source(20);
        assert_eq!(cited.doc_title, "Contrato 2024");
        assert_eq!(cited.page, Some(3));
        assert!(cited.excerpt.len() <= 80); // 20 chars * ~4 bytes max
    }

    #[test]
    fn test_language_from_str() {
        assert_eq!(PromptLanguage::from_str("en"), PromptLanguage::English);
        assert_eq!(PromptLanguage::from_str("es"), PromptLanguage::Spanish);
        assert_eq!(PromptLanguage::from_str("fr"), PromptLanguage::Spanish); // fallback
    }
}
