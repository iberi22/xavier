//! Unit tests for video temporal scene segmenter and transcript chunking pipeline.

use super::video_segmenter::*;

#[test]
fn test_timecode_formatting_and_math() {
    // 0 ms -> 00:00:00.000 / 00:00:00
    assert_eq!(format_timecode(0), "00:00:00.000");
    assert_eq!(format_timecode_short(0), "00:00:00");

    // 75200 ms -> 1m 15s 200ms -> 00:01:15.200
    assert_eq!(format_timecode(75200), "00:01:15.200");
    assert_eq!(format_timecode_short(75200), "00:01:15");

    // 2732500 ms -> 45m 32s 500ms -> 00:45:32.500
    assert_eq!(format_timecode(2732500), "00:45:32.500");
    assert_eq!(format_timecode_short(2732500), "00:45:32");

    // 3723000 ms -> 1h 2m 3s 000ms -> 01:02:03.000
    assert_eq!(format_timecode(3723000), "01:02:03.000");
    assert_eq!(format_timecode_short(3723000), "01:02:03");
}

#[test]
fn test_transcript_alignment() {
    let chunker = VideoChunker::default();

    let mut scenes = vec![
        VideoScene::new(1, 0, 10_000, 1024).with_tags(&["landscape", "mountains"]),
        VideoScene::new(2, 10_000, 25_000, 4096).with_tags(&["presenter", "stage"]),
        VideoScene::new(3, 25_000, 40_000, 8192).with_tags(&["diagram", "architecture"]),
    ];

    let transcripts = vec![
        TranscriptSegment::new(2_000, 8_000, "Welcome to the system demo."),
        TranscriptSegment::new(12_000, 18_000, "Here we present the main architecture."),
        TranscriptSegment::new(20_000, 24_000, "Notice the microservices on stage."),
        TranscriptSegment::new(
            28_000,
            38_000,
            "And this block represents the storage layer.",
        ),
    ];

    chunker.align_transcript(&mut scenes, &transcripts);

    assert_eq!(
        scenes[0].transcript_snippet.as_deref(),
        Some("Welcome to the system demo.")
    );
    assert_eq!(
        scenes[1].transcript_snippet.as_deref(),
        Some("Here we present the main architecture. Notice the microservices on stage.")
    );
    assert_eq!(
        scenes[2].transcript_snippet.as_deref(),
        Some("And this block represents the storage layer.")
    );
}

#[test]
fn test_video_chunker_sliding_window() {
    let metadata = VideoMetadata::new(60_000, 1920, 1080, 29.97, "mp4");
    // Window: 20s (20_000ms), Overlap: 2s (2_000ms) -> Step: 18s (18_000ms)
    let chunker = VideoChunker::new(20_000, 2_000);

    let scenes = vec![
        VideoScene::new(1, 0, 15_000, 100)
            .with_tags(&["intro", "logo"])
            .with_transcript("Intro speech"),
        VideoScene::new(2, 15_000, 35_000, 500)
            .with_tags(&["speaker", "slide"])
            .with_transcript("Slide presentation details"),
        VideoScene::new(3, 35_000, 60_000, 1000)
            .with_tags(&["outro", "credits"])
            .with_transcript("Conclusion and Q&A"),
    ];

    let chunks = chunker.generate_chunks(&metadata, &scenes);

    assert!(!chunks.is_empty(), "Chunks should be generated");

    // First chunk: [0s - 20s] -> includes Scene 1 (0-15s) and Scene 2 (15-35s)
    assert_eq!(chunks[0].chunk_index, 0);
    assert_eq!(chunks[0].start_ms, 0);
    assert_eq!(chunks[0].end_ms, 20_000);
    assert_eq!(chunks[0].scenes.len(), 2);
    assert!(chunks[0].formatted_text.contains("[00:00:00 - 00:00:20]"));
    assert!(chunks[0]
        .formatted_text
        .contains("Scene 1: Visual: intro, logo | Transcript: \"Intro speech\""));

    // Second chunk: [18s - 38s] -> includes Scene 2 (15-35s) and Scene 3 (35-60s)
    assert_eq!(chunks[1].chunk_index, 1);
    assert_eq!(chunks[1].start_ms, 18_000);
    assert_eq!(chunks[1].end_ms, 38_000);
    assert_eq!(chunks[1].scenes.len(), 2);
}

#[test]
fn test_keyframe_segmentation() {
    let metadata = VideoMetadata::new(50_000, 1280, 720, 25.0, "webm");
    let chunker = VideoChunker::default();

    let keyframes = vec![(0u64, 512u64), (12_000u64, 2048u64), (30_000u64, 8192u64)];

    let scenes = chunker.segment_by_keyframes(&metadata, &keyframes);

    assert_eq!(scenes.len(), 3);

    assert_eq!(scenes[0].scene_index, 1);
    assert_eq!(scenes[0].start_ms, 0);
    assert_eq!(scenes[0].end_ms, 12_000);
    assert_eq!(scenes[0].keyframe_offset_bytes, 512);

    assert_eq!(scenes[1].scene_index, 2);
    assert_eq!(scenes[1].start_ms, 12_000);
    assert_eq!(scenes[1].end_ms, 30_000);

    assert_eq!(scenes[2].scene_index, 3);
    assert_eq!(scenes[2].start_ms, 30_000);
    assert_eq!(scenes[2].end_ms, 50_000); // Clamped to metadata.duration_ms
}

#[test]
fn test_edge_cases() {
    let chunker = VideoChunker::default();

    // Zero-duration video
    let zero_meta = VideoMetadata::new(0, 1920, 1080, 0.0, "mp4");
    assert_eq!(zero_meta.fps, 30.0); // Sanitized default
    let scenes = vec![VideoScene::new(1, 0, 1000, 0)];
    let zero_chunks = chunker.generate_chunks(&zero_meta, &scenes);
    assert!(zero_chunks.is_empty());

    // Empty keyframes
    let kf_scenes = chunker.segment_by_keyframes(&zero_meta, &[]);
    assert!(kf_scenes.is_empty());

    // Out of bounds timestamp clipping
    let short_meta = VideoMetadata::new(10_000, 1920, 1080, 30.0, "mp4");
    let oob_scene = VideoScene::new(1, 15_000, 20_000, 100);
    let chunks = chunker.generate_chunks(&short_meta, &[oob_scene]);
    // Scene starting after duration should not be included in window [0, 10_000]
    assert_eq!(chunks[0].scenes.len(), 0);
    assert!(chunks[0].formatted_text.contains("No scenes detected"));
}
