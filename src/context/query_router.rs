//! Intent-based query dispatcher to specialized RAG indexes.
//!
//! Evaluates incoming search queries using rule-based heuristics and keyword matching
//! (supporting Spanish and English domain terms) to determine the target index modality
//! and extract search key-value filters.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Canonical data type modalities supported by specialized RAG indexes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataTypeKind {
    /// Legal documents, contracts, clauses, statutes, and regulatory text.
    Legal,
    /// Source code, AST symbols, stack traces, compiler errors, and API specs.
    Code,
    /// Graphical images, diagrams, screenshots, and OCR outputs.
    Image,
    /// Video or general visual media content.
    Media,
    /// Voice recordings, audio transcriptions, and speech logs.
    Audio,
    /// General unstructured text, chat notes, or markdown documentation.
    Text,
}

impl DataTypeKind {
    /// Returns all available data type modalities.
    pub fn all() -> &'static [DataTypeKind] {
        &[
            DataTypeKind::Legal,
            DataTypeKind::Code,
            DataTypeKind::Image,
            DataTypeKind::Media,
            DataTypeKind::Audio,
            DataTypeKind::Text,
        ]
    }
}

/// Decision output produced by the query router.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryRoutingDecision {
    /// The primary modality determined by the classification rules.
    pub primary_modality: DataTypeKind,
    /// Secondary modalities to query if confidence is low or query is cross-domain.
    pub secondary_modalities: Vec<DataTypeKind>,
    /// Classification confidence score bounded in [0.0, 1.0].
    pub confidence: f32,
    /// Key-value metadata filters extracted directly from the query string (e.g., ext, lang, file).
    pub extracted_filters: HashMap<String, String>,
}

/// Rule-based and heuristic fast intent classifier.
#[derive(Debug, Clone, Default)]
pub struct IntentQueryRouter;

impl IntentQueryRouter {
    /// Constructs a new `IntentQueryRouter`.
    pub fn new() -> Self {
        Self
    }

    /// Routes an incoming query string to its target RAG index decision.
    pub fn route_query(&self, query: &str) -> QueryRoutingDecision {
        let (clean_query, extracted_filters) = extract_filters(query);
        let lowered = clean_query.to_lowercase();

        let mut legal_score = calculate_keyword_score(&lowered, LEGAL_KEYWORDS, LEGAL_PATTERNS);
        let mut code_score = calculate_keyword_score(&lowered, CODE_KEYWORDS, CODE_PATTERNS);
        let mut image_score = calculate_keyword_score(&lowered, IMAGE_KEYWORDS, IMAGE_PATTERNS);
        let mut audio_score = calculate_keyword_score(&lowered, AUDIO_KEYWORDS, AUDIO_PATTERNS);
        let mut media_score = calculate_keyword_score(&lowered, MEDIA_KEYWORDS, MEDIA_PATTERNS);

        // Adjust scores based on explicit extracted filters
        if let Some(ext) = extracted_filters.get("ext") {
            let ext_lower = ext.to_lowercase();
            if matches!(
                ext_lower.as_str(),
                "rs" | "py" | "js" | "ts" | "c" | "cpp" | "go" | "java" | "sh" | "json" | "toml"
            ) {
                code_score += 0.5;
            } else if matches!(ext_lower.as_str(), "png" | "jpg" | "jpeg" | "svg" | "webp") {
                image_score += 0.5;
            } else if matches!(ext_lower.as_str(), "mp3" | "wav" | "flac" | "m4a" | "ogg") {
                audio_score += 0.5;
            } else if matches!(ext_lower.as_str(), "mp4" | "mkv" | "avi" | "webm") {
                media_score += 0.5;
            } else if matches!(ext_lower.as_str(), "pdf" | "doc" | "docx") {
                legal_score += 0.2;
            }
        }

        if let Some(lang) = extracted_filters.get("lang") {
            let lang_lower = lang.to_lowercase();
            if matches!(
                lang_lower.as_str(),
                "rust" | "python" | "typescript" | "javascript" | "golang" | "c" | "cpp" | "java"
            ) {
                code_score += 0.5;
            }
        }

        let scores = [
            (DataTypeKind::Legal, legal_score),
            (DataTypeKind::Code, code_score),
            (DataTypeKind::Image, image_score),
            (DataTypeKind::Audio, audio_score),
            (DataTypeKind::Media, media_score),
        ];

        let mut max_modality = DataTypeKind::Text;
        let mut max_score = 0.0f32;

        for (kind, score) in scores {
            if score > max_score {
                max_score = score;
                max_modality = kind;
            }
        }

        // Calculate bounded confidence score in [0.0, 1.0]
        let raw_confidence = if max_score == 0.0 {
            0.5f32
        } else {
            // Normalize score to confidence range (0.5 to 1.0)
            (0.5 + (max_score * 0.25)).min(1.0)
        };
        let confidence = (raw_confidence * 100.0).round() / 100.0;

        let secondary_modalities = if confidence < 0.60 {
            // Fallback cross-modality mode: query all modalities if confidence is under 0.60
            DataTypeKind::all()
                .iter()
                .copied()
                .filter(|&k| k != max_modality)
                .collect()
        } else {
            // Collect any secondary modality that scored close to the max score
            scores
                .iter()
                .filter(|&(kind, score)| *kind != max_modality && *score >= 0.30)
                .map(|&(kind, _)| kind)
                .collect()
        };

        QueryRoutingDecision {
            primary_modality: max_modality,
            secondary_modalities,
            confidence,
            extracted_filters,
        }
    }
}

/// Standalone convenience routing function.
pub fn route_query(query: &str) -> QueryRoutingDecision {
    IntentQueryRouter::new().route_query(query)
}

fn calculate_keyword_score(lowered: &str, keywords: &[&str], patterns: &[&str]) -> f32 {
    let mut score = 0.0f32;

    for &kw in keywords {
        if lowered.contains(kw) {
            score += 0.35;
        }
    }

    for &pat in patterns {
        if lowered.contains(pat) {
            score += 0.50;
        }
    }

    score
}

fn extract_filters(query: &str) -> (String, HashMap<String, String>) {
    let mut filters = HashMap::new();
    let mut clean_words = Vec::new();

    for token in query.split_whitespace() {
        if let Some((key, val)) = token.split_once(':') {
            let key_lower = key.to_lowercase();
            if matches!(
                key_lower.as_str(),
                "ext"
                    | "extension"
                    | "lang"
                    | "language"
                    | "file"
                    | "path"
                    | "type"
                    | "date"
                    | "author"
            ) {
                filters.insert(key_lower, val.trim_matches('"').to_string());
                continue;
            }
        }
        clean_words.push(token);
    }

    (clean_words.join(" "), filters)
}

// Legal domain terminology (Spanish & English)
const LEGAL_KEYWORDS: &[&str] = &[
    "cláusula",
    "clausula",
    "contrato",
    "arrendamiento",
    "demanda",
    "artículo",
    "articulo",
    "ley",
    "estatuto",
    "jurisprudencia",
    "juez",
    "tribunal",
    "abogado",
    "derecho",
    "penal",
    "civil",
    "mercantil",
    "contract",
    "clause",
    "statute",
    "lease",
    "liability",
    "indemnity",
    "court",
    "verdict",
    "plaintiff",
    "defendant",
    "regulation",
    "compliance",
    "nda",
    "terms of service",
];

const LEGAL_PATTERNS: &[&str] = &[
    "cláusula penal",
    "clausula penal",
    "contrato de",
    "de acuerdo con la ley",
    "bajo el artículo",
    "según el estatuto",
    "terms and conditions",
    "breach of contract",
    "governing law",
    "force majeure",
];

// Code domain terminology
const CODE_KEYWORDS: &[&str] = &[
    "function",
    "fn",
    "class",
    "struct",
    "enum",
    "impl",
    "trait",
    "method",
    "variable",
    "const",
    "import",
    "pub",
    "async",
    "await",
    "return",
    "stacktrace",
    "panic",
    "bug",
    "refactor",
    "compiler",
    "error[e",
    "syntaxerror",
    "nullpointerexception",
    "cargo",
    "npm",
    "pip",
    "git",
    "commit",
    "pull request",
];

const CODE_PATTERNS: &[&str] = &[
    "pub fn",
    "async fn",
    "impl ",
    "fn ",
    "let mut",
    "def ",
    "class ",
    "function ",
    "const ",
    "return ",
    "import ",
    "package ",
    "// ",
    "/*",
    "-> ",
    "=>",
    "i32",
    "u64",
    "f64",
    "string",
    "vec<",
];

// Image domain terminology
const IMAGE_KEYWORDS: &[&str] = &[
    "imagen",
    "captura",
    "diagrama",
    "foto",
    "screenshot",
    "picture",
    "photo",
    "image",
    "diagram",
    "ocr",
    "esquema",
    "dibujo",
    "grafico",
    "gráfico",
    "infografía",
];

const IMAGE_PATTERNS: &[&str] = &[
    "captura de pantalla",
    "screen capture",
    "ocr text",
    "reconocimiento de imagen",
    "image scan",
    "visual diagram",
];

// Audio domain terminology
const AUDIO_KEYWORDS: &[&str] = &[
    "audio",
    "grabacion",
    "grabación",
    "voz",
    "transcripcion",
    "transcripción",
    "whisper",
    "podcast",
    "voice",
    "recording",
    "transcript",
    "speech",
    "speech-to-text",
    "nota de voz",
    "llamada",
];

const AUDIO_PATTERNS: &[&str] = &[
    "transcripción de audio",
    "transcripcion de voz",
    "audio recording",
    "voice memo",
    "speech recognition",
    "llamada grabada",
];

// Media domain terminology
const MEDIA_KEYWORDS: &[&str] = &[
    "video",
    "vídeo",
    "pelicula",
    "película",
    "camara",
    "cámara",
    "footage",
    "recording",
    "frame",
    "clip",
    "multimedia",
    "streaming",
];

const MEDIA_PATTERNS: &[&str] = &[
    "video recording",
    "grabación de video",
    "grabacion de video",
    "camera clip",
    "video clip",
];
