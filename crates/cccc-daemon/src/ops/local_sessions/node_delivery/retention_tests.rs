//! Real dispatcher and native queue; only the provider executable is a fixture.
use super::*;
use cccc_contracts::session_node::{SessionNodeEndpoint, SessionNodeMembership};
use cccc_core::{
    fs,
    local_sessions::{NativeHost, NativeSession},
};
use serde_json::Value;
use std::{os::unix::fs::PermissionsExt, path::PathBuf, sync::mpsc, time::Duration};

#[path = "retention_gate.rs"]
mod gate;

struct Fixture {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    delivery: RemoteSessionDelivery,
    calls: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("private delivery fixture");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("Home");
        home.initialize().expect("initialize");
        let provider = temp.path().join("provider");
        std::fs::create_dir(&provider).expect("provider");
        let program = provider.join("native-queue");
        std::fs::write(
            &program,
            r#"#!/bin/sh
[ "$1" = queue ] || exit 1
printf 'submitted\n' >> "$CODEX_HOME/calls"
if [ -p "$CODEX_HOME/provider-release" ]; then
  printf entered > "$CODEX_HOME/provider-entered"
  IFS= read -r released < "$CODEX_HOME/provider-release"
fi
"#,
        )
        .expect("queue fixture");
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700))
            .expect("executable");
        let session = local_sessions::write(
            &home,
            NativeSession {
                source_id: "codex:retention-thread".into(),
                runtime: cccc_contracts::local_session::LocalSessionRuntime::Codex,
                native_session_id: "retention-thread".into(),
                host: NativeHost::capture(std::process::id()).expect("host"),
                cwd: temp.path().to_string_lossy().into_owned(),
                title: "Isolated delivery retention".into(),
                origin: None,
                revision: "retention-revision".into(),
                context_epoch: 0,
                codex_home: Some(provider.clone()),
                codex_can_receive: true,
                codex_program: Some(program),
                claude_config_dir: None,
                pi_agent_dir: None,
                opencode_config_dir: None,
                opencode_server: None,
                retired: None,
            },
        )
        .expect("native lifecycle");
        let membership = SessionNodeMembership {
            hub_id: "retention-hub".into(),
            source_id: session.source_id.clone(),
            source_revision: session.revision.clone(),
            group_id: "retention-group".into(),
            group_title: "Retention".into(),
            actor_id: "native".into(),
            actor_generation: "generation".into(),
            role: "peer".into(),
            scope_key: String::new(),
            enabled: true,
        };
        session_nodes::set_client(
            &home,
            &session_nodes::ClientDescriptor {
                endpoint: SessionNodeEndpoint {
                    endpoint_id: membership.hub_id.clone(),
                    addresses: vec!["127.0.0.1:12345".into()],
                    relay_url: None,
                },
                node_id: "retention-node".into(),
                grant_id: "retention-grant".into(),
                owner: session.host,
                address: "127.0.0.1:12346".parse().expect("loopback"),
                secret: "retention-secret".into(),
                connected: true,
                memberships: vec![membership.clone()],
            },
        )
        .expect("connector");
        Self {
            delivery: RemoteSessionDelivery {
                handoff_id: "current-handoff".into(),
                source_id: session.source_id,
                source_revision: session.revision,
                membership,
                event_ids: vec!["source-event".into()],
                text: "Isolated native delivery".into(),
            },
            calls: provider.join("calls"),
            _temp: temp,
            home,
        }
    }

    fn receive(&self, delivery: &RemoteSessionDelivery) -> cccc_contracts::DaemonResponse {
        crate::handle_request(
            &self.home,
            &DaemonRequest {
                v: 1,
                op: "session_node_incoming".into(),
                args: json!({"secret":"retention-secret","delivery":delivery})
                    .as_object()
                    .expect("args")
                    .clone(),
            },
        )
    }

    fn journal_path(&self) -> PathBuf {
        self.home.root().join("state/session_nodes/deliveries.json")
    }

    fn seed(&self, count: usize, outcome: Value) {
        let records: std::collections::BTreeMap<_, _> = (0..count)
            .map(|index| {
                (
                    format!("old-hub:old-grant:{index}"),
                    json!({"fingerprint":"a".repeat(64),"outcome":outcome}),
                )
            })
            .collect();
        fs::write_secret_json(&self.journal_path(), &records).expect("legacy journal");
    }

    fn submissions(&self) -> usize {
        std::fs::read_to_string(&self.calls)
            .unwrap_or_default()
            .lines()
            .count()
    }
}

#[test]
fn full_completed_legacy_journal_accepts_new_work_and_replays_receipts_without_resubmission() {
    let f = Fixture::new();
    f.seed(8192, json!({"status":"accepted"}));
    let response = f.receive(&f.delivery);
    assert!(response.ok, "{:?}", response.error);
    assert_eq!(response.result["outcome"]["status"], "accepted");
    assert_eq!(f.submissions(), 1);
    let replay = f.receive(&f.delivery);
    assert!(replay.ok, "{:?}", replay.error);
    assert_eq!(replay.result["outcome"]["status"], "accepted");
    assert_eq!(f.submissions(), 1, "completed receipt must not re-execute");
    let mut collision = f.delivery.clone();
    collision.text.push_str("different payload");
    assert!(!f.receive(&collision).ok);
    assert_eq!(f.submissions(), 1);
    let state: Value = fs::read_json(&f.journal_path()).expect("active journal");
    assert!(
        state["attempts"].as_object().expect("attempts").len() < 8192,
        "completed entries release active slots"
    );
}

#[test]
fn full_unknown_journal_refuses_new_work_without_forgetting_any_unknown_attempt() {
    let f = Fixture::new();
    f.seed(
        8192,
        json!({"status":"ambiguous","reason":"outcome unknown"}),
    );
    let before = std::fs::read(f.journal_path()).expect("journal");
    let response = f.receive(&f.delivery);
    assert!(!response.ok);
    assert!(
        response
            .error
            .expect("capacity error")
            .message
            .contains("full")
    );
    assert_eq!(std::fs::read(f.journal_path()).expect("retained"), before);
    assert_eq!(f.submissions(), 0);
}

#[test]
fn concurrent_duplicate_deliveries_cross_the_native_boundary_once() {
    let f = Fixture::new();
    let results = std::thread::scope(|scope| {
        // The guard must live inside the scope: unwind releases the provider
        // before scope joins any worker, so a failed assertion cannot deadlock.
        let mut gate = gate::ProviderGate::new(f.calls.parent().expect("provider directory"));
        let (first_tx, first_rx) = mpsc::channel();
        let fixture = &f;
        scope.spawn(move || {
            first_tx
                .send(fixture.receive(&fixture.delivery))
                .expect("first result")
        });
        gate.wait_entered();
        assert_eq!(
            f.submissions(),
            1,
            "first native invocation reached the gate"
        );
        let (tx, rx) = mpsc::channel();
        for _ in 0..3 {
            let tx = tx.clone();
            let fixture = &f;
            scope.spawn(move || {
                tx.send(fixture.receive(&fixture.delivery))
                    .expect("duplicate result")
            });
        }
        let mut results = Vec::new();
        for _ in 0..3 {
            let response = rx
                .recv_timeout(Duration::from_secs(3))
                .expect("duplicate must return while the first provider is blocked");
            assert!(response.ok, "{:?}", response.error);
            assert_eq!(response.result["outcome"]["status"], "ambiguous");
            results.push(response);
        }
        assert!(
            matches!(first_rx.try_recv(), Err(mpsc::TryRecvError::Empty)),
            "the first submission must still be blocked"
        );
        assert_eq!(f.submissions(), 1, "duplicates cannot reach the provider");
        gate.release().expect("release first provider");
        let first = first_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("first completion deadline");
        assert!(first.ok, "{:?}", first.error);
        assert_eq!(first.result["outcome"]["status"], "accepted");
        results.push(first);
        results
    });
    assert!(results.iter().all(|response| response.ok));
    assert!(
        results
            .iter()
            .any(|response| response.result["outcome"]["status"] == "accepted")
    );
    assert_eq!(f.submissions(), 1);
    assert_eq!(
        f.receive(&f.delivery).result["outcome"]["status"],
        "accepted"
    );
    assert_eq!(f.submissions(), 1);
}
