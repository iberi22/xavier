//! Domain schema and preset definitions for multi-modal RAG data type profiles.
//!
//! Provides `DataTypeKind`, `ModalityTuning`, and `DataProfile` for domain-level
//! configuration of chunking, vector table mapping, retention, and hybrid search weights.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Supported data type kinds for multi-modal RAG profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataTypeKind {
    LegalText,
    CodeAst,
    VideoMultimodal,
    ImageVisual,
    AudioVoice,
    GeneralKnowledge,
}

impl DataTypeKind {
    /// Returns the string slice representation of the enum variant.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LegalText => "legal_text",
            Self::CodeAst => "code_ast",
            Self::VideoMultimodal => "video_multimodal",
            Self::ImageVisual => "image_visual",
            Self::AudioVoice => "audio_voice",
            Self::GeneralKnowledge => "general_knowledge",
        }
    }
}

impl fmt::Display for DataTypeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Tuning parameters for a specific modality's chunking and vector storage strategy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModalityTuning {
    /// Maximum tokens or units per chunk.
    pub max_chunk_tokens: usize,
    /// Overlap tokens between consecutive chunks.
    pub chunk_overlap: usize,
    /// Weight for dense vs. sparse search (0.0 = purely sparse, 1.0 = purely dense).
    pub hybrid_alpha: f32,
    /// Table name for storing vectors of this modality dimension.
    pub vector_table_name: String,
}

/// Domain profile describing RAG parameters, extensions, retention, and tuning for a data kind.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DataProfile {
    /// Unique identifier for the profile instance or preset.
    pub id: String,
    /// High-level modality data type kind.
    pub kind: DataTypeKind,
    /// Human-readable display name.
    pub display_name: String,
    /// Associated file extensions (e.g. `["pdf", "docx"]`).
    pub file_extensions: Vec<String>,
    /// Identifier for the chunking algorithm/strategy used.
    pub chunking_strategy: String,
    /// Vector embedding dimension size (e.g. 1536, 512).
    pub embedding_dim: usize,
    /// Default retention period in days, if applicable.
    pub default_retention_days: Option<u32>,
    /// Specific tuning configuration for chunking and search.
    pub modality_tuning: ModalityTuning,
}

impl DataProfile {
    /// Returns a pre-tuned domain `DataProfile` default for the given `DataTypeKind`.
    pub fn preset(kind: DataTypeKind) -> Self {
        match kind {
            DataTypeKind::LegalText => Self {
                id: "preset_legal_text".to_string(),
                kind: DataTypeKind::LegalText,
                display_name: "Legal Text & Contracts".to_string(),
                file_extensions: vec![
                    "pdf".to_string(),
                    "docx".to_string(),
                    "txt".to_string(),
                    "md".to_string(),
                ],
                chunking_strategy: "semantic_paragraph".to_string(),
                embedding_dim: 1536,
                default_retention_days: Some(3650),
                modality_tuning: ModalityTuning {
                    max_chunk_tokens: 1024,
                    chunk_overlap: 128,
                    hybrid_alpha: 0.7,
                    vector_table_name: "vec_legal_1536".to_string(),
                },
            },
            DataTypeKind::CodeAst => Self {
                id: "preset_code_ast".to_string(),
                kind: DataTypeKind::CodeAst,
                display_name: "Source Code & AST Symbols".to_string(),
                file_extensions: vec![
                    "rs".to_string(),
                    "ts".to_string(),
                    "js".to_string(),
                    "py".to_string(),
                    "go".to_string(),
                    "java".to_string(),
                    "c".to_string(),
                    "cpp".to_string(),
                    "h".to_string(),
                    "hpp".to_string(),
                ],
                chunking_strategy: "ast_symbol_tree".to_string(),
                embedding_dim: 1536,
                default_retention_days: None,
                modality_tuning: ModalityTuning {
                    max_chunk_tokens: 512,
                    chunk_overlap: 64,
                    hybrid_alpha: 0.3,
                    vector_table_name: "vec_code_1536".to_string(),
                },
            },
            DataTypeKind::VideoMultimodal => Self {
                id: "preset_video_multimodal".to_string(),
                kind: DataTypeKind::VideoMultimodal,
                display_name: "Video Keyframes & Transcripts".to_string(),
                file_extensions: vec![
                    "mp4".to_string(),
                    "mkv".to_string(),
                    "webm".to_string(),
                    "mov".to_string(),
                ],
                chunking_strategy: "scene_frame_interval".to_string(),
                embedding_dim: 512,
                default_retention_days: Some(365),
                modality_tuning: ModalityTuning {
                    max_chunk_tokens: 256,
                    chunk_overlap: 32,
                    hybrid_alpha: 0.8,
                    vector_table_name: "vec_video_512".to_string(),
                },
            },
            DataTypeKind::ImageVisual => Self {
                id: "preset_image_visual".to_string(),
                kind: DataTypeKind::ImageVisual,
                display_name: "Visual Images & OCR".to_string(),
                file_extensions: vec![
                    "png".to_string(),
                    "jpg".to_string(),
                    "jpeg".to_string(),
                    "webp".to_string(),
                    "svg".to_string(),
                ],
                chunking_strategy: "visual_patch_grid".to_string(),
                embedding_dim: 512,
                default_retention_days: Some(365),
                modality_tuning: ModalityTuning {
                    max_chunk_tokens: 128,
                    chunk_overlap: 0,
                    hybrid_alpha: 0.9,
                    vector_table_name: "vec_image_512".to_string(),
                },
            },
            DataTypeKind::AudioVoice => Self {
                id: "preset_audio_voice".to_string(),
                kind: DataTypeKind::AudioVoice,
                display_name: "Audio & Voice Recording Transcripts".to_string(),
                file_extensions: vec![
                    "mp3".to_string(),
                    "wav".to_string(),
                    "flac".to_string(),
                    "m4a".to_string(),
                    "ogg".to_string(),
                ],
                chunking_strategy: "time_window_speech".to_string(),
                embedding_dim: 768,
                default_retention_days: Some(180),
                modality_tuning: ModalityTuning {
                    max_chunk_tokens: 384,
                    chunk_overlap: 48,
                    hybrid_alpha: 0.6,
                    vector_table_name: "vec_audio_768".to_string(),
                },
            },
            DataTypeKind::GeneralKnowledge => Self {
                id: "preset_general_knowledge".to_string(),
                kind: DataTypeKind::GeneralKnowledge,
                display_name: "General Knowledge & Notes".to_string(),
                file_extensions: vec![
                    "md".to_string(),
                    "txt".to_string(),
                    "json".to_string(),
                    "yaml".to_string(),
                    "html".to_string(),
                ],
                chunking_strategy: "sliding_window".to_string(),
                embedding_dim: 1536,
                default_retention_days: None,
                modality_tuning: ModalityTuning {
                    max_chunk_tokens: 512,
                    chunk_overlap: 64,
                    hybrid_alpha: 0.5,
                    vector_table_name: "vec_general_1536".to_string(),
                },
            },
        }
    }

    /// Checks if a file extension (with or without leading dot) matches this profile.
    pub fn matches_extension(&self, ext: &str) -> bool {
        let clean_ext = ext.trim().trim_start_matches('.').to_lowercase();
        self.file_extensions
            .iter()
            .any(|e| e.to_lowercase() == clean_ext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_type_kind_as_str_and_display() {
        assert_eq!(DataTypeKind::LegalText.as_str(), "legal_text");
        assert_eq!(DataTypeKind::CodeAst.as_str(), "code_ast");
        assert_eq!(DataTypeKind::VideoMultimodal.as_str(), "video_multimodal");
        assert_eq!(DataTypeKind::ImageVisual.as_str(), "image_visual");
        assert_eq!(DataTypeKind::AudioVoice.as_str(), "audio_voice");
        assert_eq!(DataTypeKind::GeneralKnowledge.as_str(), "general_knowledge");

        assert_eq!(format!("{}", DataTypeKind::LegalText), "legal_text");
        assert_eq!(format!("{}", DataTypeKind::CodeAst), "code_ast");
    }

    #[test]
    fn test_data_type_kind_serde_roundtrip() {
        let kinds = vec![
            DataTypeKind::LegalText,
            DataTypeKind::CodeAst,
            DataTypeKind::VideoMultimodal,
            DataTypeKind::ImageVisual,
            DataTypeKind::AudioVoice,
            DataTypeKind::GeneralKnowledge,
        ];

        for kind in kinds {
            let json = serde_json::to_string(&kind).expect("serialization should succeed");
            let deserialized: DataTypeKind =
                serde_json::from_str(&json).expect("deserialization should succeed");
            assert_eq!(kind, deserialized);
        }

        // Test snake_case JSON string exact matching
        assert_eq!(
            serde_json::to_string(&DataTypeKind::LegalText).unwrap(),
            "\"legal_text\""
        );
        assert_eq!(
            serde_json::to_string(&DataTypeKind::CodeAst).unwrap(),
            "\"code_ast\""
        );
        assert_eq!(
            serde_json::to_string(&DataTypeKind::VideoMultimodal).unwrap(),
            "\"video_multimodal\""
        );
    }

    #[test]
    fn test_data_profile_presets() {
        let kinds = vec![
            DataTypeKind::LegalText,
            DataTypeKind::CodeAst,
            DataTypeKind::VideoMultimodal,
            DataTypeKind::ImageVisual,
            DataTypeKind::AudioVoice,
            DataTypeKind::GeneralKnowledge,
        ];

        for kind in kinds {
            let profile = DataProfile::preset(kind);
            assert_eq!(profile.kind, kind);
            assert!(!profile.id.is_empty());
            assert!(!profile.display_name.is_empty());
            assert!(!profile.file_extensions.is_empty());
            assert!(!profile.chunking_strategy.is_empty());
            assert!(profile.embedding_dim > 0);
            assert!(!profile.modality_tuning.vector_table_name.is_empty());
            assert!(profile.modality_tuning.hybrid_alpha >= 0.0);
            assert!(profile.modality_tuning.hybrid_alpha <= 1.0);
        }
    }

    #[test]
    fn test_data_profile_extension_matching() {
        let code_profile = DataProfile::preset(DataTypeKind::CodeAst);
        assert!(code_profile.matches_extension("rs"));
        assert!(code_profile.matches_extension(".RS"));
        assert!(code_profile.matches_extension("py"));
        assert!(!code_profile.matches_extension("pdf"));

        let legal_profile = DataProfile::preset(DataTypeKind::LegalText);
        assert!(legal_profile.matches_extension("pdf"));
        assert!(legal_profile.matches_extension(".PDF"));
        assert!(!legal_profile.matches_extension("rs"));
    }

    #[test]
    fn test_data_profile_serde_roundtrip() {
        let profile = DataProfile::preset(DataTypeKind::LegalText);
        let json = serde_json::to_string_pretty(&profile).expect("serialization failed");
        let deserialized: DataProfile =
            serde_json::from_str(&json).expect("deserialization failed");
        assert_eq!(profile, deserialized);
    }
}
