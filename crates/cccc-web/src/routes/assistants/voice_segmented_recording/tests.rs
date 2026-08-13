use super::*;

#[test]
fn default_segment_is_twenty_five_minutes_of_pcm16() {
    assert_eq!(DEFAULT_SEGMENT_DURATION_MS, 1_500_000);
    assert_eq!(DEFAULT_SEGMENT_BYTES, 48_000_000);
}

#[tokio::test]
async fn recording_rolls_without_splitting_or_accumulating_audio_in_memory() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let mut recording = SegmentedPcmRecording::create_with_limits(&home, 8, 24).expect("recording");

    assert!(
        recording
            .append(&[1, 2, 3, 4, 5, 6])
            .await
            .expect("first")
            .is_empty()
    );
    let boundaries = recording
        .append(&[7, 8, 9, 10, 11, 12, 13, 14])
        .await
        .expect("roll");
    assert_eq!(
        boundaries,
        vec![SegmentBoundary {
            index: 1,
            start_ms: 0,
            end_ms: 0,
            bytes: 8,
        }]
    );

    let segments = recording.finish().await.expect("finish");
    assert_eq!(segments.len(), 2);
    assert_eq!(
        std::fs::read(segments[0].file.path()).expect("first file"),
        [1, 2, 3, 4, 5, 6, 7, 8]
    );
    assert_eq!(
        std::fs::read(segments[1].file.path()).expect("second file"),
        [9, 10, 11, 12, 13, 14]
    );
    assert_eq!((segments[0].index, segments[1].index), (1, 2));
    assert_eq!((segments[0].bytes, segments[1].bytes), (8, 6));
}

#[tokio::test]
async fn recording_rejects_unaligned_or_over_limit_audio_before_writing_it() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let mut recording = SegmentedPcmRecording::create_with_limits(&home, 4, 8).expect("recording");

    assert_eq!(
        recording.append(&[1]).await.expect_err("odd audio").code,
        "invalid_audio"
    );
    recording
        .append(&[1, 2, 3, 4, 5, 6])
        .await
        .expect("within limit");
    assert_eq!(
        recording
            .append(&[7, 8, 9, 10])
            .await
            .expect_err("session limit")
            .code,
        "audio_too_large"
    );
    let segments = recording.finish().await.expect("finish");
    assert_eq!(
        segments.iter().map(|segment| segment.bytes).sum::<usize>(),
        6
    );
}
