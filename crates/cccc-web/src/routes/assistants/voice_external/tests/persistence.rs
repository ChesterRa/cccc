use super::*;
use cccc_core::GroupStore;
use std::sync::atomic::Ordering;
#[path = "persistence_fixture.rs"]
mod fixture;
use fixture::Fixture;

#[tokio::test]
async fn completed_last_packet_retries_failed_or_lost_checkpoint_without_duplicate_input() {
    for lose_reply in [false, true] {
        let f = Fixture::new(lose_reply).await;
        let (mut run, _provider) = f.active().await;
        run.audio(&vec![0; 6400]).await.expect("send audio");
        run.stop(json!(2)).await.expect("stop recording");
        run.receive(super::volcengine_tests::volc_reply(
            "最后一句不能丢",
            0,
            true,
            true,
        ))
        .expect("process recording checkpoint");
        assert!(run.completed);
        f.failures.store(1, Ordering::SeqCst);
        assert!(
            persistence::checkpoints(&f.state, &f.group, &mut run, false)
                .await
                .is_err()
        );
        assert!(run.persisted.is_empty());
        let final_event = persistence::recover(&f.state, &f.group, &mut run).await;
        assert_eq!(final_event["type"], "final_asr_text");
        assert_eq!(final_event["text"], "最后一句不能丢");
        assert_eq!(final_event["transcript_persisted"], true);
        let requests = f.requests.lock().expect("recorded requests lock");
        assert_eq!(
            requests[0].args["segment_id"],
            requests[1].args["segment_id"]
        );
        let store = GroupStore::new(f.state.home.clone()).expect("process recording checkpoint");
        let ledger = cccc_core::ledger::read_all(
            &store
                .ledger_path(&f.group)
                .expect("process recording checkpoint"),
        )
        .expect("process recording checkpoint");
        assert_eq!(
            ledger
                .iter()
                .filter(|e| e.kind == "assistant.voice.input")
                .count(),
            1
        );
    }
}

#[tokio::test]
async fn completed_packet_keeps_browser_retry_payload_when_daemon_remains_unavailable() {
    let f = Fixture::new(false).await;
    let (mut run, _provider) = f.active().await;
    run.audio(&vec![0; 6400]).await.expect("send audio");
    run.stop(json!(2)).await.expect("stop recording");
    run.receive(super::volcengine_tests::volc_reply(
        "待恢复终稿",
        0,
        true,
        true,
    ))
    .expect("process recording checkpoint");
    f.failures.store(10, Ordering::SeqCst);
    let final_event = persistence::recover(&f.state, &f.group, &mut run).await;
    assert_eq!(final_event["text"], "待恢复终稿");
    assert_eq!(final_event["partial"], false);
    assert_eq!(final_event["transcript_persistence"], "failed");
    assert_eq!(final_event["transcript_persisted"], false);
    assert!(run.persisted.is_empty());
}

#[tokio::test]
async fn checkpoint_setting_defers_stable_sentences_until_interval_or_stop() {
    for window in [Value::Null, json!(45)] {
        let f = Fixture::new(false).await;
        let (mut run, _provider) = f.active().await;
        let settings = json!({"config":{"auto_document_max_window_seconds":window}});
        run.checkpoint_schedule = checkpoint_schedule::CheckpointSchedule::new(&settings);
        run.receive(super::volcengine_tests::volc_reply(
            "第一句",
            0,
            true,
            false,
        ))
        .expect("process recording checkpoint");
        persistence::checkpoints(&f.state, &f.group, &mut run, false)
            .await
            .expect("process recording checkpoint");
        assert!(
            f.requests
                .lock()
                .expect("recorded requests lock")
                .is_empty()
        );
        run.checkpoint_schedule = checkpoint_schedule::CheckpointSchedule::starting_at(
            &settings,
            tokio::time::Instant::now() - Duration::from_secs(46),
        );
        persistence::checkpoints(&f.state, &f.group, &mut run, false)
            .await
            .expect("process recording checkpoint");
        assert_eq!(
            f.requests.lock().expect("recorded requests lock").len(),
            usize::from(!window.is_null())
        );
        run.receive(super::volcengine_tests::volc_reply(
            "第二句",
            600,
            true,
            false,
        ))
        .expect("process recording checkpoint");
        persistence::checkpoints(&f.state, &f.group, &mut run, false)
            .await
            .expect("process recording checkpoint");
        assert_eq!(
            f.requests.lock().expect("recorded requests lock").len(),
            usize::from(!window.is_null())
        );
        run.audio(&vec![0; 6400]).await.expect("send audio");
        run.stop(json!(2)).await.expect("stop recording");
        run.receive(super::volcengine_tests::volc_reply(
            "第三句",
            1200,
            true,
            true,
        ))
        .expect("process recording checkpoint");
        persistence::checkpoints(&f.state, &f.group, &mut run, false)
            .await
            .expect("process recording checkpoint");
        let requests = f.requests.lock().expect("recorded requests lock");
        assert_eq!(requests.len(), 3);
        assert_eq!(
            requests
                .iter()
                .map(|r| r.args["start_ms"].as_u64().expect("segment start time"))
                .collect::<Vec<_>>(),
            [0, 600, 1200]
        );
        assert_eq!(run.persisted.len(), 3);
    }
}
