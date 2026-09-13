//! Local audio transcription ingestion & timestamped chunker
//!
//! Provides data structures and merging mechanisms for audio transcript segments,
//! preserving speaker diarization headers, timestamp ranges, and computing word density metrics.

use serde::{Deserialize, Serialize};

/// A single timestamped transcript segment from an audio source or Whisper transcription.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AudioTranscriptSegment {
    /// Optional speaker tag (e.g. "Speaker A", "Deponent", "Lawyer").
    pub speaker_tag: Option<String>,
    /// Start timestamp of the segment in seconds.
    pub start_sec: f32,
    /// End timestamp of the segment in seconds.
    pub end_sec: f32,
    /// Transcribed text content.
    pub text: String,
    /// Confidence score of transcription (0.0 to 1.0).
    pub confidence: f32,
}

impl AudioTranscriptSegment {
    /// Creates a new audio transcript segment.
    pub fn new(
        speaker_tag: Option<String>,
        start_sec: f32,
        end_sec: f32,
        text: impl Into<String>,
        confidence: f32,
    ) -> Self {
        Self {
            speaker_tag,
            start_sec,
            end_sec,
            text: text.into(),
            confidence: confidence.clamp(0.0, 1.0),
        }
    }

    /// Duration of segment in seconds.
    pub fn duration(&self) -> f32 {
        (self.end_sec - self.start_sec).max(0.0)
    }

    /// Word count of transcript text.
    pub fn word_count(&self) -> usize {
        self.text.split_whitespace().count()
    }

    /// Checks if the segment is empty or zero-duration.
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty() || self.duration() <= 0.0
    }
}

/// A merged RAG chunk suitable for vector indexing and LLM prompt context.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AudioRagChunk {
    /// Unique identifier for the chunk (e.g., "audio_123-chunk-0").
    pub chunk_id: String,
    /// Identifier of parent audio source.
    pub audio_id: String,
    /// Start and end time range in seconds (start_sec, end_sec).
    pub time_range: (f32, f32),
    /// Formatted text preserving speaker transitions and timestamp annotations.
    pub formatted_text: String,
    /// Speaker label if chunk contains a single speaker, or None if multi-speaker/unlabeled.
    pub speaker: Option<String>,
    /// Average transcription confidence across merged segments.
    pub avg_confidence: f32,
    /// Number of segments merged into this chunk.
    pub segment_count: usize,
}

impl AudioRagChunk {
    /// Total duration of the chunk in seconds.
    pub fn duration_sec(&self) -> f32 {
        (self.time_range.1 - self.time_range.0).max(0.0)
    }

    /// Total word count of formatted text in the chunk.
    pub fn word_count(&self) -> usize {
        self.formatted_text.split_whitespace().count()
    }

    /// Computes word density (words per second) of the audio chunk.
    pub fn word_density(&self) -> f32 {
        let dur = self.duration_sec();
        if dur > 1e-4 {
            self.word_count() as f32 / dur
        } else {
            0.0
        }
    }
}

/// Configuration and engine for merging short audio segments into coherent RAG chunks.
#[derive(Debug, Clone)]
pub struct AudioChunkCombiner {
    /// Minimum target words before completing a chunk (unless max_gap or end of input).
    pub min_words: usize,
    /// Ideal target word count per chunk (e.g., 150-300 words).
    pub target_words: usize,
    /// Maximum allowed words per chunk before forcing a split.
    pub max_words: usize,
    /// Maximum allowed silent gap between segments in seconds before splitting chunks.
    pub max_gap_sec: f32,
}

impl Default for AudioChunkCombiner {
    fn default() -> Self {
        Self {
            min_words: 50,
            target_words: 200,
            max_words: 350,
            max_gap_sec: 5.0,
        }
    }
}

impl AudioChunkCombiner {
    /// Create a new AudioChunkCombiner with default parameters.
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder method for min_words.
    pub fn with_min_words(mut self, min_words: usize) -> Self {
        self.min_words = min_words;
        self
    }

    /// Builder method for target_words.
    pub fn with_target_words(mut self, target_words: usize) -> Self {
        self.target_words = target_words;
        self
    }

    /// Builder method for max_words.
    pub fn with_max_words(mut self, max_words: usize) -> Self {
        self.max_words = max_words;
        self
    }

    /// Builder method for max_gap_sec.
    pub fn with_max_gap_sec(mut self, max_gap_sec: f32) -> Self {
        self.max_gap_sec = max_gap_sec;
        self
    }

    /// Combines timestamped audio transcript segments into coherent AudioRagChunks.
    ///
    /// Filters out empty/silence segments, groups contiguous speech up to target word count or max gap,
    /// and formats speaker transitions like `[Speaker A, 01:14] ... [Speaker B, 01:25] ...`.
    pub fn combine_segments(
        &self,
        audio_id: &str,
        segments: &[AudioTranscriptSegment],
    ) -> Vec<AudioRagChunk> {
        // Filter out zero-length or whitespace-only segments
        let valid_segments: Vec<&AudioTranscriptSegment> =
            segments.iter().filter(|s| !s.is_empty()).collect();

        if valid_segments.is_empty() {
            return Vec::new();
        }

        let mut chunks = Vec::new();
        let mut current_segments: Vec<&AudioTranscriptSegment> = Vec::new();
        let mut current_words = 0;

        for seg in valid_segments {
            let seg_words = seg.word_count();

            if !current_segments.is_empty() {
                let last_seg = current_segments.last().unwrap();
                let gap = seg.start_sec - last_seg.end_sec;

                let exceeds_max_words = current_words + seg_words > self.max_words;
                let reaches_target = current_words >= self.target_words;
                let gap_too_large = gap > self.max_gap_sec;

                if exceeds_max_words
                    || (current_words >= self.min_words && reaches_target)
                    || gap_too_large
                {
                    // Flush current accumulated chunk
                    if let Some(chunk) =
                        Self::build_chunk(audio_id, chunks.len(), &current_segments)
                    {
                        chunks.push(chunk);
                    }
                    current_segments.clear();
                    current_words = 0;
                }
            }

            current_segments.push(seg);
            current_words += seg_words;
        }

        if !current_segments.is_empty() {
            if let Some(chunk) = Self::build_chunk(audio_id, chunks.len(), &current_segments) {
                chunks.push(chunk);
            }
        }

        chunks
    }

    /// Builds an `AudioRagChunk` from a list of accumulated segments.
    fn build_chunk(
        audio_id: &str,
        chunk_idx: usize,
        segments: &[&AudioTranscriptSegment],
    ) -> Option<AudioRagChunk> {
        if segments.is_empty() {
            return None;
        }

        let start_sec = segments.first().map(|s| s.start_sec).unwrap_or(0.0);
        let end_sec = segments.last().map(|s| s.end_sec).unwrap_or(start_sec);

        let total_conf: f32 = segments.iter().map(|s| s.confidence).sum();
        let avg_confidence = total_conf / segments.len() as f32;

        // Determine speaker: if all segments share the exact same speaker tag, preserve it;
        // otherwise None (multi-speaker).
        let first_speaker = segments.first().and_then(|s| s.speaker_tag.as_ref());
        let same_speaker = segments
            .iter()
            .all(|s| s.speaker_tag.as_ref() == first_speaker);
        let speaker = if same_speaker {
            first_speaker.cloned()
        } else {
            None
        };

        // Format text with speaker transitions and timestamps
        let mut formatted_parts = Vec::new();
        let mut last_speaker: Option<&str> = None;

        for seg in segments {
            let current_spk = seg.speaker_tag.as_deref();
            let speaker_changed = current_spk != last_speaker;

            if speaker_changed || formatted_parts.is_empty() {
                let ts_str = format_timestamp_mmss(seg.start_sec);
                let header = match current_spk {
                    Some(spk) => format!("[{}, {}]", spk, ts_str),
                    None => format!("[{}]", ts_str),
                };
                formatted_parts.push(format!("{} {}", header, seg.text.trim()));
                last_speaker = current_spk;
            } else {
                formatted_parts.push(seg.text.trim().to_string());
            }
        }

        let formatted_text = formatted_parts.join(" ");

        Some(AudioRagChunk {
            chunk_id: format!("{}-chunk-{}", audio_id, chunk_idx),
            audio_id: audio_id.to_string(),
            time_range: (start_sec, end_sec),
            formatted_text,
            speaker,
            avg_confidence,
            segment_count: segments.len(),
        })
    }
}

/// Format seconds as MM:SS (or HH:MM:SS if >= 1 hour).
pub fn format_timestamp_mmss(seconds: f32) -> String {
    let total_secs = seconds.max(0.0) as u32;
    let hrs = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;

    if hrs > 0 {
        format!("{:02}:{:02}:{:02}", hrs, mins, secs)
    } else {
        format!("{:02}:{:02}", mins, secs)
    }
}
