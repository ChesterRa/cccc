#![cfg(unix)]
use super::super::{ActorLaunchConfig, AnalystSession, LaunchConfig, process};
use super::*;
use cccc_contracts::{ActorRuntime, RuntimeMode};
use cccc_core::HomeLayout;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

struct Fixture {
    client: PiRpcClient,
    owner: super::super::ChildOwner,
    temp: tempfile::TempDir,
}
fn environment(root: &Path) -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "HOME".into(),
            root.join("provider").to_string_lossy().into_owned(),
        ),
        (
            "PI_CODING_AGENT_DIR".into(),
            root.join("provider").to_string_lossy().into_owned(),
        ),
        (
            "CCCC_HOME".into(),
            root.join("cccc").to_string_lossy().into_owned(),
        ),
        (
            "PI_FIXTURE_WIRE".into(),
            root.join("wire.jsonl").to_string_lossy().into_owned(),
        ),
        (
            "PI_FIXTURE_DIRECTORY".into(),
            root.join("sessions").to_string_lossy().into_owned(),
        ),
        (
            "PI_FIXTURE_CLI".into(),
            root.join("cccc-cli").to_string_lossy().into_owned(),
        ),
    ])
}
fn executable(root: &Path) -> String {
    let file = root.join("pi");
    std::fs::write(&file, include_str!("fixture.py")).expect("write Pi executable fixture");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o700))
        .expect("make Pi fixture executable");
    std::fs::write(root.join("cccc-cli"), "#!/bin/sh\nexit 0\n")
        .expect("write fixture host executable");
    file.to_string_lossy().into_owned()
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("create isolated Pi fixture home");
        let command = vec![executable(temp.path())];
        let (owner, input, output) =
            process::spawn_piped_quiet(&command, temp.path(), &environment(temp.path()))
                .expect("spawn isolated Pi fixture");
        let client = PiRpcClient::new(input, output, "generation".into(), "fixture-session".into())
            .expect("open fixture RPC owner");
        Self {
            client,
            owner,
            temp,
        }
    }
    async fn stop(&self) {
        self.client.close().await;
        self.owner.stop().expect("stop owned Pi fixture process");
        assert!(!self.owner.running());
    }
}
async fn event(rx: &mut broadcast::Receiver<AnalystEvent>, method: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = rx.recv().await.expect("receive normalized fixture event");
            if event.message["method"] == method {
                return event.message;
            }
        }
    })
    .await
    .expect("event deadline")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pi_accepted_handled_queued_and_provider_failure_do_not_fake_success() {
    let f = Fixture::new();
    let mut events = f.client.subscribe();
    assert!(
        f.client
            .start_prompt("fixture-session", "handled", "handled")
            .await
            .is_err()
    );
    let failed = f
        .client
        .start_prompt("fixture-session", "failed", "failed")
        .await
        .expect("admit failing provider fixture turn");
    let final_event = event(&mut events, "turn/completed").await;
    assert_eq!(final_event["params"]["turn"]["id"], failed);
    assert_eq!(final_event["params"]["turn"]["status"], "failed");
    assert!(
        !final_event
            .to_string()
            .contains("private fixture provider error")
    );
    assert!(
        f.client
            .start_prompt("fixture-session", "queued", "queued")
            .await
            .is_err()
    );
    event(&mut events, MANAGED_AGENT_DISCONNECTED_METHOD).await;
    assert!(!f.client.running());
    assert!(events.try_recv().is_err()); // foreign settlement cannot complete queued owner
    f.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pi_pending_acceptance_and_exact_turn_cancellation_clear_queue_before_abort() {
    let f = Fixture::new();
    let pending = f
        .client
        .start_prompt("fixture-session", "pending", "accepted-only");
    tokio::pin!(pending);
    assert!(
        tokio::time::timeout(Duration::from_millis(80), &mut pending)
            .await
            .is_err()
    );
    assert!(
        f.client
            .cancel_input("fixture-session", Some("pending"))
            .await
            .expect("cancel pending fixture input")
            .is_some()
    );
    assert_eq!(
        pending
            .await
            .expect_err("pending admission must be interrupted")
            .kind(),
        io::ErrorKind::Interrupted
    );
    let mut events = f.client.subscribe();
    let id = f
        .client
        .start_prompt("fixture-session", "hold", "hold")
        .await
        .expect("admit held fixture turn");
    assert_eq!(
        f.client
            .cancel("fixture-session", "old-turn")
            .await
            .expect_err("stale turn cancellation must fail")
            .kind(),
        io::ErrorKind::NotFound
    );
    f.client
        .cancel("fixture-session", &id)
        .await
        .expect("cancel exact held fixture turn");
    assert_eq!(
        event(&mut events, "turn/completed").await["params"]["turn"]["status"],
        "interrupted"
    );
    let wire = std::fs::read_to_string(f.temp.path().join("wire.jsonl"))
        .expect("read cancellation wire fixture");
    let types: Vec<String> = wire
        .lines()
        .map(|line| {
            serde_json::from_str::<Value>(line).expect("decode fixture wire JSON")["type"]
                .as_str()
                .expect("fixture wire event has a type")
                .into()
        })
        .collect();
    assert_eq!(
        types,
        [
            "prompt",
            "clear_queue",
            "abort",
            "prompt",
            "clear_queue",
            "abort"
        ]
    );
    f.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pi_interaction_tokens_reject_cancelled_provider_id_reuse() {
    let f = Fixture::new();
    let mut events = f.client.subscribe();
    let old = f
        .client
        .start_prompt("fixture-session", "old", "dialog-old")
        .await
        .expect("admit first dialog fixture turn");
    let old_approval = event(&mut events, "cccc/approvalRequired").await;
    assert_eq!(old_approval["params"]["turnId"], old);
    let old_token = f.client.permissions()[0]["request_id"]
        .as_str()
        .expect("old permission has an opaque request token")
        .to_owned();
    assert_eq!(old_approval["params"]["request_id"], old_token);
    f.client
        .cancel("fixture-session", &old)
        .await
        .expect("cancel old dialog fixture turn");
    let new = f
        .client
        .start_prompt("fixture-session", "new", "dialog-new")
        .await
        .expect("admit replacement dialog fixture turn");
    let new_approval = event(&mut events, "cccc/approvalRequired").await;
    assert_eq!(new_approval["params"]["turnId"], new);
    let new_token = f.client.permissions()[0]["request_id"]
        .as_str()
        .expect("new permission has an opaque request token")
        .to_owned();
    assert_eq!(new_approval["params"]["request_id"], new_token);
    assert_ne!(new_token, old_token);
    assert_eq!(
        f.client
            .respond_interaction(&old_token, json!({"value":"one"}))
            .await
            .expect_err("cancelled token must be rejected")
            .kind(),
        io::ErrorKind::NotFound
    );
    assert!(
        f.client
            .respond_interaction(&new_token, json!({"value":"invalid"}))
            .await
            .is_err()
    );
    f.client
        .respond_interaction(&new_token, json!({"value":"one","id":"forged"}))
        .await
        .expect("send valid current dialog choice");
    loop {
        let done = event(&mut events, "turn/completed").await;
        if done["params"]["turn"]["id"] == new {
            assert_eq!(done["params"]["turn"]["status"], "completed");
            break;
        }
    }
    assert!(f.client.permissions().is_empty());
    f.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pi_foreign_live_user_record_invalidates_instead_of_claiming_owner() {
    let f = Fixture::new();
    let mut events = f.client.subscribe();
    assert!(
        f.client
            .start_prompt("fixture-session", "owned", "foreign")
            .await
            .is_err()
    );
    event(&mut events, MANAGED_AGENT_DISCONNECTED_METHOD).await;
    assert!(!f.client.running());
    f.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pi_actor_and_secretary_share_control_but_not_receipt_authority() {
    let temp = tempfile::tempdir().expect("create isolated Actor and Secretary home");
    let home =
        HomeLayout::from_path(temp.path().join("cccc")).expect("resolve isolated host layout");
    home.initialize().expect("initialize isolated host layout");
    let program = executable(temp.path());
    let env = environment(temp.path());
    let group = cccc_core::GroupStore::new(home.clone())
        .expect("open fixture Group store")
        .create("Pi fixture", "")
        .expect("create Actor fixture Group")
        .group_id;
    let config = || ActorLaunchConfig {
        workdir: temp.path().into(),
        group_id: group.clone(),
        actor_id: "pi-actor".into(),
        runtime: ActorRuntime::Pi,
        runtime_mode: RuntimeMode::Default,
        command: vec![program.clone()],
        environment: env.clone(),
    };
    let actor = AnalystSession::launch_actor(&home, config())
        .await
        .expect("launch controlled Pi Actor fixture");
    assert!(actor.structured_only());
    assert!(!actor.tui_ready());
    assert!(!actor.resumable());
    let old_generation = actor.generation().to_owned();
    let id = actor.thread_id().to_owned();
    let mut events = actor.subscribe();
    actor
        .start_turn(actor.generation(), "delivery", "normal")
        .await
        .expect("start Actor fixture delivery");
    event(&mut events, "turn/completed").await;
    assert!(actor.resumable());
    actor
        .stop(actor.generation())
        .await
        .expect("stop Actor fixture before restore");
    let resumed = AnalystSession::launch_actor(&home, config())
        .await
        .expect("restore controlled Pi Actor fixture");
    assert_eq!(resumed.thread_id(), id);
    assert!(resumed.thread_resumed);
    assert!(
        resumed
            .start_turn(&old_generation, "late", "normal")
            .await
            .is_err()
    );
    resumed
        .stop(resumed.generation())
        .await
        .expect("stop restored Pi Actor fixture");
    let store = cccc_core::voice_secretary::SecretaryTaskStore::new(home.clone());
    std::fs::create_dir_all(store.runtime_workspace()).expect("create Secretary fixture workspace");
    let analyst_file = home.daemon_dir().join("codex_voice_analyst.json");
    let analyst_receipt = json!({"thread_id":"unrelated","materialized":false});
    cccc_core::fs::write_json(&analyst_file, &analyst_receipt)
        .expect("write unrelated Analyst receipt fixture");
    let (_tx, cancel) = tokio::sync::watch::channel(false);
    let mut secretary_env = env;
    secretary_env.remove("CCCC_HOME");
    let secretary = AnalystSession::launch_secretary(
        &home,
        LaunchConfig {
            workdir: store.runtime_workspace(),
            runtime: ActorRuntime::Pi,
            runtime_mode: RuntimeMode::Default,
            command: vec![program],
            environment: secretary_env,
            resume_thread_id: None,
        },
        cancel,
        "synthetic-fixture-grant",
    )
    .await
    .expect("launch controlled Pi Secretary fixture");
    let mut events = secretary.subscribe();
    secretary
        .start_turn(secretary.generation(), "group-a", "normal")
        .await
        .expect("start first Group Secretary fixture turn");
    event(&mut events, "turn/completed").await;
    secretary
        .start_turn(secretary.generation(), "group-b", "normal")
        .await
        .expect("start second Group Secretary fixture turn");
    event(&mut events, "turn/completed").await;
    assert!(secretary.process_running());
    assert_eq!(
        cccc_core::fs::read_json::<Value>(&analyst_file)
            .expect("read unrelated Analyst receipt after Secretary turns"),
        analyst_receipt
    );
    let paths = secretary.cleanup_paths.clone();
    secretary
        .stop(secretary.generation())
        .await
        .expect("stop owned Secretary fixture");
    assert!(paths.iter().all(|path| !path.exists()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pi_late_preflight_dialogs_are_declined_during_queue_clear_and_abort() {
    let f = Fixture::new();
    let mut events = f.client.subscribe();
    let turn = f
        .client
        .start_prompt("fixture-session", "hold-late", "hold-late")
        .await
        .expect("admit late-dialog fixture turn");
    f.client
        .cancel("fixture-session", &turn)
        .await
        .expect("cancel late-dialog fixture turn");
    assert_eq!(
        event(&mut events, "turn/completed").await["params"]["turn"]["status"],
        "interrupted"
    );
    assert!(f.client.permissions().is_empty());
    f.client
        .request("get_state", json!({}), Duration::from_secs(3))
        .await
        .expect("confirm fixture protocol remains responsive after cancellation");
    let wire = std::fs::read_to_string(f.temp.path().join("wire.jsonl"))
        .expect("read late-dialog wire fixture");
    assert_eq!(wire.matches("extension_ui_response").count(), 2);
    assert!(
        wire.find("extension_ui_response")
            .expect("late dialog cancellation was written")
            < wire.find("abort").expect("abort command was written")
    );
    assert!(f.client.running());
    f.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pi_idle_abort_ack_does_not_release_an_unsettled_prompt_preflight() {
    let f = Fixture::new();
    let mut events = f.client.subscribe();
    let turn = f
        .client
        .start_prompt("fixture-session", "preflight", "preflight-dialog")
        .await
        .expect("admit unsettled preflight dialog");
    assert_eq!(
        f.client
            .cancel("fixture-session", &turn)
            .await
            .expect_err("idle preflight abort must invalidate the session")
            .kind(),
        io::ErrorKind::BrokenPipe
    );
    event(&mut events, MANAGED_AGENT_DISCONNECTED_METHOD).await;
    assert!(!f.client.running());
    assert!(f.client.permissions().is_empty());
    assert!(
        f.client
            .start_prompt("fixture-session", "replacement", "normal")
            .await
            .is_err()
    );
    let wire = std::fs::read_to_string(f.temp.path().join("wire.jsonl"))
        .expect("read preflight cancellation wire fixture");
    assert_eq!(wire.matches("\"prompt\"").count(), 1);
    f.stop().await;
}
