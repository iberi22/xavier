//! Video temporal scene segmenter & transcript chunker module for Multimodal Video RAG.
//!
//! Provides data structures and sliding window algorithms to process video keyframes,
//! align speech-to-text transcripts with timecodes, and format searchable synthetic text chunks.

use serde::{Deserialize, Serialize};

/// Represents a single time-bounded scene within a video track.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoScene {
    /// Zero-based or 1-based sequential scene index within the video.
    pub scene_index: u32,
    /// Start timestamp of the scene in milliseconds from video start.
    pub start_ms: u64,
    /// End timestamp of the scene in milliseconds from video start.
    pub end_ms: u64,
    /// Byte offset of the keyframe frame in the underlying media container.
    pub keyframe_offset_bytes: u64,
    /// Speech-to-text transcript snippet aligned with this temporal scene interval.
    pub transcript_snippet: Option<String>,
    /// Visual classification, detected objects, or OCR tags extracted from keyframe.
    pub visual_tags: Vec<String>,
}

impl VideoScene {
    /// Creates a new `VideoScene` with initialized visual tags and empty transcript.
    pub fn new(scene_index: u32, start_ms: u64, end_ms: u64, keyframe_offset_bytes: u64) -> Self {
        Self {
            scene_index,
            start_ms,
            end_ms: end_ms.max(start_ms),
            keyframe_offset_bytes,
            transcript_snippet: None,
            visual_tags: Vec::new(),
        }
    }

    /// Appends visual tags extracted from vision models or keyframe analysis.
    pub fn with_tags(mut self, tags: &[&str]) -> Self {
        self.visual_tags.extend(tags.iter().map(|s| s.to_string()));
        self
    }

    /// Attaches transcript snippet to the scene.
    pub fn with_transcript(mut self, snippet: impl Into<String>) -> Self {
        self.transcript_snippet = Some(snippet.into());
        self
    }

    /// Returns the scene duration in milliseconds.
    pub fn duration_ms(&self) -> u64 {
        self.end_ms.saturating_sub(self.start_ms)
    }
}

/// Technical metadata describing video dimensions, framerate, total duration, and codec format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoMetadata {
    /// Total duration of the video stream in milliseconds.
    pub duration_ms: u64,
    /// Pixel width of the video track.
    pub width: u32,
    /// Pixel height of the video track.
    pub height: u32,
    /// Frames per second of the video stream.
    pub fps: f32,
    /// Container or codec format identifier (e.g. "mp4", "webm", "h264").
    pub format: String,
}

impl VideoMetadata {
    /// Constructs a new `VideoMetadata` instance with safe default sanitization for fps.
    pub fn new(
        duration_ms: u64,
        width: u32,
        height: u32,
        fps: f32,
        format: impl Into<String>,
    ) -> Self {
        Self {
            duration_ms,
            width,
            height,
            fps: if fps > 0.0 { fps } else { 30.0 },
            format: format.into(),
        }
    }
}

/// A speech-to-text transcript segment with millisecond timestamp boundaries (e.g. from Whisper).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscriptSegment {
    /// Start time in milliseconds.
    pub start_ms: u64,
    /// End time in milliseconds.
    pub end_ms: u64,
    /// Text content uttered during this interval.
    pub text: String,
}

impl TranscriptSegment {
    /// Creates a new `TranscriptSegment`.
    pub fn new(start_ms: u64, end_ms: u64, text: impl Into<String>) -> Self {
        Self {
            start_ms,
            end_ms: end_ms.max(start_ms),
            text: text.into(),
        }
    }
}

/// Synthetic RAG search chunk representing a temporal slice of video content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoChunk {
    /// Sequential chunk index within the segmented video stream.
    pub chunk_index: u32,
    /// Start timestamp of the chunk window in milliseconds.
    pub start_ms: u64,
    /// End timestamp of the chunk window in milliseconds.
    pub end_ms: u64,
    /// Scenes included within this temporal window.
    pub scenes: Vec<VideoScene>,
    /// Formatted synthetic text representation optimized for vector embedding and keyword indexing.
    pub formatted_text: String,
}

/// Video RAG temporal segmenter and transcript chunking engine.
#[derive(Debug, Clone)]
pub struct VideoChunker {
    /// Temporal window duration in milliseconds for sliding chunk generation (e.g. 10000ms - 30000ms).
    pub window_duration_ms: u64,
    /// Overlap duration in milliseconds between adjacent sliding chunks (e.g. 2000ms).
    pub overlap_ms: u64,
}

impl Default for VideoChunker {
    fn default() -> Self {
        Self {
            window_duration_ms: 30_000, // Default 30-second window
            overlap_ms: 2_000,          // Default 2-second overlap
        }
    }
}

impl VideoChunker {
    /// Constructs a new `VideoChunker` with specified window and overlap durations in milliseconds.
    pub fn new(window_duration_ms: u64, overlap_ms: u64) -> Self {
        let min_window = 1_000u64;
        let window = window_duration_ms.max(min_window);
        let safe_overlap = overlap_ms.min(window.saturating_sub(100));
        Self {
            window_duration_ms: window,
            overlap_ms: safe_overlap,
        }
    }

    /// Aligns speech-to-text transcript segments with scene boundaries.
    ///
    /// For each scene, collects transcript text from segments that overlap in time
    /// and joins them into `transcript_snippet`.
    pub fn align_transcript(&self, scenes: &mut [VideoScene], segments: &[TranscriptSegment]) {
        for scene in scenes.iter_mut() {
            let mut matching_texts = Vec::new();

            for seg in segments {
                // Check temporal overlap: max(start1, start2) < min(end1, end2)
                let overlap_start = scene.start_ms.max(seg.start_ms);
                let overlap_end = scene.end_ms.min(seg.end_ms);

                if overlap_start < overlap_end {
                    let trimmed = seg.text.trim();
                    if !trimmed.is_empty() {
                        matching_texts.push(trimmed);
                    }
                }
            }

            if !matching_texts.is_empty() {
                scene.transcript_snippet = Some(matching_texts.join(" "));
            }
        }
    }

    /// Generates temporal chunks using a sliding window strategy over total video duration.
    ///
    /// For each window `[win_start, win_end]`, identifies scenes intersecting the window
    /// and builds formatted synthetic text chunks suitable for semantic search indexing.
    pub fn generate_chunks(
        &self,
        metadata: &VideoMetadata,
        scenes: &[VideoScene],
    ) -> Vec<VideoChunk> {
        if metadata.duration_ms == 0 {
            return Vec::new();
        }

        let mut chunks = Vec::new();
        let step = self
            .window_duration_ms
            .saturating_sub(self.overlap_ms)
            .max(1);
        let mut curr_start = 0u64;
        let mut chunk_idx = 0u32;

        while curr_start < metadata.duration_ms {
            let curr_end = (curr_start + self.window_duration_ms).min(metadata.duration_ms);

            // Find all scenes overlapping [curr_start, curr_end]
            let window_scenes: Vec<VideoScene> = scenes
                .iter()
                .filter(|scene| {
                    let s_start = scene.start_ms.min(metadata.duration_ms);
                    let s_end = scene.end_ms.min(metadata.duration_ms);
                    s_start < curr_end && s_end > curr_start
                })
                .cloned()
                .collect();

            let formatted_text = self.format_chunk_text(curr_start, curr_end, &window_scenes);

            chunks.push(VideoChunk {
                chunk_index: chunk_idx,
                start_ms: curr_start,
                end_ms: curr_end,
                scenes: window_scenes,
                formatted_text,
            });

            chunk_idx += 1;
            curr_start += step;

            if curr_end >= metadata.duration_ms {
                break;
            }
        }

        chunks
    }

    /// Formats searchable synthetic text chunk in standard RAG format:
    /// `[HH:MM:SS - HH:MM:SS] Scene <id>: <visual description / transcript>`
    pub fn format_chunk_text(
        &self,
        chunk_start_ms: u64,
        chunk_end_ms: u64,
        scenes: &[VideoScene],
    ) -> String {
        let time_header = format!(
            "[{:.8} - {:.8}]",
            format_timecode_short(chunk_start_ms),
            format_timecode_short(chunk_end_ms)
        );

        if scenes.is_empty() {
            return format!("{} No scenes detected", time_header);
        }

        let mut scene_lines = Vec::new();
        for scene in scenes {
            let mut parts = Vec::new();

            if !scene.visual_tags.is_empty() {
                parts.push(format!("Visual: {}", scene.visual_tags.join(", ")));
            }

            if let Some(ref transcript) = scene.transcript_snippet {
                parts.push(format!("Transcript: \"{}\"", transcript));
            }

            let content = if parts.is_empty() {
                "Keyframe detected".to_string()
            } else {
                parts.join(" | ")
            };

            scene_lines.push(format!(
                "{} Scene {}: {}",
                time_header, scene.scene_index, content
            ));
        }

        scene_lines.join("\n")
    }

    /// Constructs scenes from raw keyframe timestamp boundaries and container byte offsets.
    pub fn segment_by_keyframes(
        &self,
        metadata: &VideoMetadata,
        keyframe_offsets: &[(u64, u64)], // (timestamp_ms, offset_bytes)
    ) -> Vec<VideoScene> {
        if keyframe_offsets.is_empty() || metadata.duration_ms == 0 {
            return Vec::new();
        }

        let mut scenes = Vec::new();
        let total_keyframes = keyframe_offsets.len();

        for (i, &(start_ms, offset_bytes)) in keyframe_offsets.iter().enumerate() {
            let clamped_start = start_ms.min(metadata.duration_ms);
            let end_ms = if i + 1 < total_keyframes {
                keyframe_offsets[i + 1].0.min(metadata.duration_ms)
            } else {
                metadata.duration_ms
            };

            scenes.push(VideoScene::new(
                (i + 1) as u32,
                clamped_start,
                end_ms,
                offset_bytes,
            ));
        }

        scenes
    }
}

/// Formats milliseconds into human-readable millisecond timecode: `HH:MM:SS.mmm`
pub fn format_timecode(ms: u64) -> String {
    let hours = ms / 3_600_000;
    let minutes = (ms % 3_600_000) / 60_000;
    let seconds = (ms % 60_000) / 1_000;
    let millis = ms % 1_000;
    format!("{:02}:{:02}:{:02}.{:03}", hours, minutes, seconds, millis)
}

/// Formats milliseconds into human-readable short timecode: `HH:MM:SS`
pub fn format_timecode_short(ms: u64) -> String {
    let hours = ms / 3_600_000;
    let minutes = (ms % 3_600_000) / 60_000;
    let seconds = (ms % 60_000) / 1_000;
    format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
}
