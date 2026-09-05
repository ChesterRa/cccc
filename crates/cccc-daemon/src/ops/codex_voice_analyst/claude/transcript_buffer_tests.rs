use super::*;

async fn follower_with(bytes: &[u8]) -> (tempfile::TempDir, TranscriptFollower) {
    let temp = tempfile::tempdir().expect("create test directory");
    let project = temp.path().join("projects/workspace");
    std::fs::create_dir_all(&project).expect("create fixture directory");
    let id = "52b41c61-e23c-4b7c-8b60-809c347451b5";
    let path = project.join(format!("{id}.jsonl"));
    std::fs::write(&path, bytes).expect("write test fixture");
    let state = temp.path().join("state.json");
    cccc_core::fs::write_json(&state, &serde_json::json!({"linkScanPath":path}))
        .expect("complete write json in fixture");
    let mut follower = TranscriptFollower::new(state, temp.path().into(), id.into(), false);
    follower
        .initialize()
        .await
        .expect("complete initialize in fixture");
    (temp, follower)
}

#[tokio::test]
async fn valid_large_record_with_neighboring_records_is_not_rejected_as_one_oversized_record() {
    let prefix = serde_json::json!({"text":"p".repeat(10_000)});
    let large = serde_json::json!({"text":"x".repeat(MAX_TRANSCRIPT_LINE_BYTES - 100)});
    let tail = serde_json::json!({"text":"t".repeat(10_000)});
    let data = format!("{prefix}\n{large}\n{tail}\n");
    let (_temp, mut follower) = follower_with(data.as_bytes()).await;
    let mut records = Vec::new();
    while follower.offset < data.len() as u64 {
        records.extend(
            follower
                .poll()
                .await
                .expect("every individual record is within the limit"),
        );
    }
    assert_eq!(records, vec![prefix, large, tail]);
    assert!(
        follower
            .poll()
            .await
            .expect("complete poll in fixture")
            .is_empty()
    );
}

#[tokio::test]
async fn oversized_single_record_still_fails_with_or_without_a_newline() {
    for terminated in [false, true] {
        let record = serde_json::json!({"text":"x".repeat(MAX_TRANSCRIPT_LINE_BYTES)});
        let mut data = record.to_string();
        if terminated {
            data.push('\n');
        }
        let (_temp, mut follower) = follower_with(data.as_bytes()).await;
        let error = loop {
            match follower.poll().await {
                Err(error) => break error,
                Ok(_) => assert!(
                    follower.offset < data.len() as u64,
                    "oversized record was accepted"
                ),
            }
        };
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("exceeded its limit"));
    }
}
