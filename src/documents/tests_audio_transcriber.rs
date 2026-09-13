//! Unit tests for audio transcript segment chunking and combiner mechanisms.

#[cfg(test)]
mod tests {
    use crate::documents::audio_transcriber::*;

    const EPSILON: f32 = 1e-4;

    #[test]
    fn test_single_speaker_chunking_and_metrics() {
        let segments = vec![
            AudioTranscriptSegment::new(
                Some("Lawyer".to_string()),
                0.0,
                10.0,
                "Good morning ladies and gentlemen of the jury. We are here today to present key evidence.",
                0.95,
            ),
            AudioTranscriptSegment::new(
                Some("Lawyer".to_string()),
                10.5,
                20.0,
                "This deposition covers events that occurred on the evening of November 12th.",
                0.92,
            ),
        ];

        let combiner = AudioChunkCombiner::new()
            .with_target_words(100)
            .with_max_words(200)
            .with_max_gap_sec(5.0);

        let chunks = combiner.combine_segments("depo_001", &segments);

        assert_eq!(chunks.len(), 1);
        let chunk = &chunks[0];

        assert_eq!(chunk.chunk_id, "depo_001-chunk-0");
        assert_eq!(chunk.audio_id, "depo_001");
        assert_eq!(chunk.speaker, Some("Lawyer".to_string()));
        assert_eq!(chunk.segment_count, 2);

        assert!((chunk.time_range.0 - 0.0).abs() < EPSILON);
        assert!((chunk.time_range.1 - 20.0).abs() < EPSILON);
        assert!((chunk.duration_sec() - 20.0).abs() < EPSILON);

        let expected_words = chunk.formatted_text.split_whitespace().count();
        assert_eq!(chunk.word_count(), expected_words);

        let expected_density = expected_words as f32 / 20.0;
        assert!((chunk.word_density() - expected_density).abs() < EPSILON);

        let expected_avg_conf = (0.95 + 0.92) / 2.0;
        assert!((chunk.avg_confidence - expected_avg_conf).abs() < EPSILON);

        assert!(chunk.formatted_text.starts_with("[Lawyer, 00:00]"));
    }

    #[test]
    fn test_multi_speaker_conversation_headers() {
        let segments = vec![
            AudioTranscriptSegment::new(
                Some("Speaker A".to_string()),
                74.0,
                80.0,
                "State your full name for the record.",
                0.98,
            ),
            AudioTranscriptSegment::new(
                Some("Speaker B".to_string()),
                85.0,
                90.0,
                "My name is John Doe, residing at 100 Main Street.",
                0.96,
            ),
            AudioTranscriptSegment::new(
                Some("Speaker A".to_string()),
                92.0,
                98.0,
                "Thank you. Were you present on the site?",
                0.94,
            ),
        ];

        let combiner = AudioChunkCombiner::new()
            .with_target_words(300)
            .with_max_words(500)
            .with_max_gap_sec(10.0);

        let chunks = combiner.combine_segments("hearing_45", &segments);

        assert_eq!(chunks.len(), 1);
        let chunk = &chunks[0];

        // Multi-speaker chunk should have speaker: None
        assert_eq!(chunk.speaker, None);
        assert_eq!(chunk.segment_count, 3);

        // Header timestamps should be formatted in MM:SS format
        assert!(chunk.formatted_text.contains("[Speaker A, 01:14]"));
        assert!(chunk.formatted_text.contains("[Speaker B, 01:25]"));
        assert!(chunk.formatted_text.contains("[Speaker A, 01:32]"));

        assert!((chunk.time_range.0 - 74.0).abs() < EPSILON);
        assert!((chunk.time_range.1 - 98.0).abs() < EPSILON);
    }

    #[test]
    fn test_filter_empty_and_zero_duration_segments() {
        let segments = vec![
            AudioTranscriptSegment::new(
                Some("Witness".to_string()),
                0.0,
                0.0, // Zero duration
                "Silent preamble",
                0.50,
            ),
            AudioTranscriptSegment::new(
                Some("Witness".to_string()),
                1.0,
                5.0,
                "   \n\t  ", // Whitespace only
                0.80,
            ),
            AudioTranscriptSegment::new(
                Some("Witness".to_string()),
                5.0,
                12.0,
                "I clearly saw the vehicle cross the intersection.",
                0.99,
            ),
        ];

        let combiner = AudioChunkCombiner::default();
        let chunks = combiner.combine_segments("audio_test", &segments);

        assert_eq!(chunks.len(), 1);
        let chunk = &chunks[0];

        assert_eq!(chunk.segment_count, 1);
        assert_eq!(chunk.speaker, Some("Witness".to_string()));
        assert_eq!(chunk.time_range, (5.0, 12.0));
        assert!((chunk.duration_sec() - 7.0).abs() < EPSILON);
        assert_eq!(
            chunk.formatted_text,
            "[Witness, 00:05] I clearly saw the vehicle cross the intersection."
        );
    }

    #[test]
    fn test_custom_combiner_word_and_gap_splits() {
        let segments = vec![
            AudioTranscriptSegment::new(
                None,
                0.0,
                5.0,
                "First short audio segment with eight words total here.",
                0.90,
            ),
            AudioTranscriptSegment::new(
                None,
                6.0,
                11.0,
                "Second short audio segment also containing eight words total.",
                0.90,
            ),
            AudioTranscriptSegment::new(
                None,
                30.0, // Gap of 19 seconds > max_gap_sec 5.0
                35.0,
                "Third segment following a long silent pause interval.",
                0.90,
            ),
        ];

        let combiner = AudioChunkCombiner::new()
            .with_min_words(5)
            .with_target_words(10)
            .with_max_words(12) // Split forced after max_words reached
            .with_max_gap_sec(5.0);

        let chunks = combiner.combine_segments("meeting_01", &segments);

        assert!(chunks.len() >= 2);

        // Verify no zero length chunks
        for chunk in &chunks {
            assert!(chunk.duration_sec() > 0.0);
            assert!(chunk.word_count() > 0);
            assert!(chunk.word_density() > 0.0);
        }

        // Test timestamp helper
        assert_eq!(format_timestamp_mmss(0.0), "00:00");
        assert_eq!(format_timestamp_mmss(74.0), "01:14");
        assert_eq!(format_timestamp_mmss(3665.0), "01:01:05");
    }
}
