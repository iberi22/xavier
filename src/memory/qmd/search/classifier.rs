//! Query Classification Module
//!
//! Categorizes search queries into Code, Conceptual, or Mixed types
//! to apply adaptive weighting across lexical (BM25), vector, and knowledge graph (KG) indices.

/// Represents the classification of a search query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryClass {
    /// Queries requesting specific code symbols, functions, files, or technical errors.
    /// Prefers high lexical (BM25) weighting.
    ExactCode,
    /// Queries using natural language to express high-level concepts, architectural design, or user intent.
    /// Prefers high vector (semantic) weighting.
    Conceptual,
    /// General or ambiguous queries that do not clearly fit code or conceptual archetypes.
    /// Uses balanced weighting.
    Mixed,
}

/// Holds the adaptive weights for lexical (BM25), vector, and knowledge graph (KG) retrieval.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QueryClassWeights {
    pub bm25: f32,
    pub vector: f32,
    pub kg: f32,
}

/// Classifies a query string into one of the three `QueryClass` categories.
/// Uses deterministic heuristics without external ML dependencies.
pub fn classify_query(query: &str) -> QueryClass {
    let query_lower = query.to_lowercase();

    // 1. Check for ExactCode symbols/patterns
    // Symbols/punctuation common in code
    let code_symbols = [
        "::", "->", "=>", "!", "@", "#", "[", "]", "{", "}", "(", ")", ";", "<", ">", "*", "&", "_",
    ];
    let has_code_symbol = code_symbols.iter().any(|&sym| query.contains(sym));

    // Technical uppercase terms or acronyms (case-sensitive check on original query)
    let technical_terms = ["FTS5", "SQLite", "ML-KEM", "BM25", "UUID", "RRF", "JSON", "Bincode", "EVM", "DAO"];
    let has_technical_term = technical_terms.iter().any(|&term| query.contains(term));

    // Common code patterns or keyword fragments
    let code_patterns = [
        "fn", "struct", "impl", "result", "match", "if let", "some", "none",
        "borrow checker", "error", "main", "vec", "pub", "mod", "use", "cargo",
        "pointer", "compile", "null", "unwrap", "panic", "null", "sqlite-vec",
    ];
    // Check if any code patterns appear as words or distinct substrings
    let has_code_pattern = code_patterns.iter().any(|&pattern| {
        if pattern.contains(' ') {
            query_lower.contains(pattern)
        } else {
            // Check word boundaries roughly using split
            query_lower.split(|c: char| !c.is_alphanumeric()).any(|word| word == pattern)
        }
    });

    if has_code_symbol || has_technical_term || has_code_pattern {
        return QueryClass::ExactCode;
    }

    // 2. Check for Conceptual patterns
    // Conceptual keywords based on domain and benchmark queries
    let conceptual_keywords = [
        "arquitectura", "memoria", "agente", "agentes", "gobernanza", "token", "votacion",
        "reputacion", "funnel", "rag", "busqueda", "indexacion", "conceptual", "retrieval",
        "lifecycle", "proposal", "consensus", "reputation", "commons", "policy", "navigation",
        "cognitive", "inference", "decay", "consolidation", "sharing", "context", "recall",
    ];

    let has_conceptual_keyword = conceptual_keywords.iter().any(|&kw| {
        query_lower.split(|c: char| !c.is_alphanumeric()).any(|word| word == kw)
    });

    // Language structure indicators (stop words / prepositions / pronouns) to detect natural language
    let natural_language_indicators = [
        // English
        "the", "a", "an", "and", "or", "but", "if", "then", "else", "how", "what", "why", "who", "where",
        "when", "which", "to", "for", "in", "on", "at", "by", "with", "about", "of", "from", "is", "are",
        "was", "were", "be", "been", "have", "has", "had", "do", "does", "did", "does",
        // Spanish
        "el", "la", "los", "las", "un", "una", "unos", "unas", "y", "o", "pero", "si", "entonces", "como",
        "que", "quien", "donde", "cuando", "cual", "a", "para", "en", "por", "con", "de", "desde", "es",
        "son", "era", "eran", "ser", "sido", "tener", "tiene", "tienen", "hacer", "hace", "hacen"
    ];

    let has_natural_language_indicator = natural_language_indicators.iter().any(|&indicator| {
        query_lower.split(|c: char| !c.is_alphanumeric()).any(|word| word == indicator)
    });

    // Word count calculation
    let word_count = query.split_whitespace().count();

    // 4+ words and looks like natural language (contains structure indicators), OR contains highly specific domain keywords
    if (word_count >= 4 && has_natural_language_indicator) || has_conceptual_keyword {
        return QueryClass::Conceptual;
    }

    // 3. Fallback to Mixed
    QueryClass::Mixed
}

/// Returns the adaptive retrieval weights for a given query.
pub fn weights_for(query: &str) -> QueryClassWeights {
    match classify_query(query) {
        QueryClass::ExactCode => QueryClassWeights {
            bm25: 0.80,
            vector: 0.15,
            kg: 0.05,
        },
        QueryClass::Conceptual => QueryClassWeights {
            bm25: 0.15,
            vector: 0.80,
            kg: 0.05,
        },
        QueryClass::Mixed => QueryClassWeights {
            bm25: 0.50,
            vector: 0.45,
            kg: 0.05,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_exact_code() {
        // Contains technical terms and/or code symbols/patterns
        assert_eq!(classify_query("FTS5 SQLite vec"), QueryClass::ExactCode);
        assert_eq!(classify_query("rust error borrow checker match"), QueryClass::ExactCode);
        assert_eq!(classify_query("fn main Result match if let Some"), QueryClass::ExactCode);
        assert_eq!(classify_query("struct QueryClassWeights"), QueryClass::ExactCode);
        assert_eq!(classify_query("let x = y;"), QueryClass::ExactCode);
        assert_eq!(classify_query("pub use self::classifier;"), QueryClass::ExactCode);
    }

    #[test]
    fn test_classify_conceptual() {
        // Long natural language query or contains conceptual keywords
        assert_eq!(classify_query("arquitectura memoria agentes"), QueryClass::Conceptual);
        assert_eq!(classify_query("gobernanza token votacion"), QueryClass::Conceptual);
        assert_eq!(classify_query("data commons funnel reputacion"), QueryClass::Conceptual);
        assert_eq!(classify_query("how does the cognitive memory consolidation decay policy work"), QueryClass::Conceptual);
    }

    #[test]
    fn test_classify_mixed() {
        // Short, non-conceptual, non-code queries
        assert_eq!(classify_query("configuracion bridge sincronizacion"), QueryClass::Mixed);
        assert_eq!(classify_query("unicode chinese japanese korean emoji"), QueryClass::Mixed);
    }

    #[test]
    fn test_weights_for() {
        let code_weights = weights_for("FTS5 SQLite vec");
        assert!((code_weights.bm25 - 0.80).abs() < f32::EPSILON);
        assert!((code_weights.vector - 0.15).abs() < f32::EPSILON);
        assert!((code_weights.kg - 0.05).abs() < f32::EPSILON);

        let conceptual_weights = weights_for("arquitectura memoria agentes");
        assert!((conceptual_weights.bm25 - 0.15).abs() < f32::EPSILON);
        assert!((conceptual_weights.vector - 0.80).abs() < f32::EPSILON);
        assert!((conceptual_weights.kg - 0.05).abs() < f32::EPSILON);

        let mixed_weights = weights_for("configuracion bridge sincronizacion");
        assert!((mixed_weights.bm25 - 0.50).abs() < f32::EPSILON);
        assert!((mixed_weights.vector - 0.45).abs() < f32::EPSILON);
        assert!((mixed_weights.kg - 0.05).abs() < f32::EPSILON);
    }
}
