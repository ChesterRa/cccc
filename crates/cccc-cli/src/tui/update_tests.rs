use super::*;
use crate::update_checker::TriggerSlot;
use std::sync::Arc;

/// Answers every check with a newer release, at once.
struct Newer;

impl cccc_web::UpdateChecker for Newer {
    fn status(&self) -> cccc_web::UpdateStatus {
        cccc_web::UpdateStatus {
            current: "0.4.45".into(),
            installation: "standalone".into(),
            last: None,
        }
    }

    fn check(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = UpdateCheck> + Send>> {
        Box::pin(async {
            UpdateCheck {
                current: "0.4.45".into(),
                channel: "stable".into(),
                installation: "standalone".into(),
                latest: Some("0.4.46".into()),
                status: Some("newer".into()),
                failure: None,
                release_notes_url: None,
                command: Some("cccc update --channel stable".into()),
            }
        })
    }
}

#[tokio::test]
async fn a_check_is_reported_and_an_update_without_a_serving_web_does_not_start() {
    // No Web serves, so the host has no entry to start an update through.
    let entry = UpdateEntry::new(Arc::new(Newer), TriggerSlot::default(), true);
    let (commands, commands_rx) = mpsc::unbounded_channel();
    let (progress_tx, mut progress) = watch::channel(UpdateProgress::default());
    let (stop, stop_rx) = watch::channel(false);
    let worker = tokio::spawn(run(entry, commands_rx, progress_tx, stop_rx));

    commands
        .send(UpdateCommand::Check { generation: 1 })
        .expect("worker");
    let checked = progress
        .wait_for(|progress| progress.check.is_some())
        .await
        .expect("progress")
        .clone();
    assert_eq!(checked.generation, 1);
    assert!(!checked.checking);
    assert_eq!(
        checked.check.and_then(|check| check.latest),
        Some("0.4.46".into())
    );

    // A start meant for another visit is ignored.
    commands
        .send(UpdateCommand::Start {
            generation: 7,
            version: "0.4.46".into(),
        })
        .expect("worker");
    commands
        .send(UpdateCommand::Start {
            generation: 1,
            version: "0.4.46".into(),
        })
        .expect("worker");
    let rejected = progress
        .wait_for(|progress| progress.rejected.is_some())
        .await
        .expect("progress")
        .clone();
    assert_eq!(rejected.preparing, None);
    assert!(!rejected.handed_over);
    assert!(
        rejected
            .rejected
            .as_deref()
            .is_some_and(|reason| reason.contains("Web is serving"))
    );

    stop.send(true).expect("stop");
    worker.await.expect("worker ends when stopped");
}
