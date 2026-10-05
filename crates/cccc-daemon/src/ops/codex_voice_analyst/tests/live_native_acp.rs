//! Opt-in paid probe. The caller supplies isolated native login copies and a
//! fresh two-prompt allowance per Runtime. Ordinary test runs never launch it.
use super::super::*;
use futures_util::FutureExt;
use serde_json::json;
use std::{path::Path, time::Duration};

fn spend(root: &Path, runtime: &str) {
    cccc_core::fs::with_exclusive_lock(&root.join("quota.lock"), || {
        let path = root.join("quota.json");
        let mut quota: Value = cccc_core::fs::read_json(&path)?;
        let used = quota[runtime].as_u64().expect("Runtime allowance");
        assert!(used < 2, "no remaining authorized prompts");
        quota[runtime] = json!(used + 1);
        cccc_core::fs::write_json(&path, &quota)
    })
    .expect("record authorized spend before sending");
}

struct ProbeDaemon {
    launcher: PathBuf,
    environment: BTreeMap<String, String>,
    home: PathBuf,
}
impl Drop for ProbeDaemon {
    fn drop(&mut self) {
        let _ = std::process::Command::new(&self.launcher)
            .args(["daemon", "stop"])
            .envs(&self.environment)
            .env("CCCC_HOME", &self.home)
            .output();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires explicit provider-credit authorization and isolated login copies"]
async fn live_native_acp_actor_and_voice() {
    use crate::ops::codex_voice_lifecycle::{AnalystLifecycle, AnalystLifecycleEvent};
    let name = std::env::var("CCCC_NATIVE_ACP_LIVE_RUNTIME").expect("explicit Runtime");
    let runtime = match name.as_str() {
        "copilot" => ActorRuntime::Copilot,
        "devin" => ActorRuntime::Devin,
        "cursor" => ActorRuntime::Cursor,
        _ => panic!("unsupported probe Runtime"),
    };
    let root = PathBuf::from(std::env::var("CCCC_NATIVE_ACP_LIVE_ROOT").expect("isolated root"));
    assert!(root.starts_with(std::env::temp_dir()));
    let settings: Value =
        cccc_core::fs::read_json(&root.join("config.json")).expect("probe settings");
    let settings = &settings[&name];
    let environment: BTreeMap<String, String> =
        serde_json::from_value(settings["environment"].clone()).expect("environment");
    for variable in [
        "HOME",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "XDG_STATE_HOME",
        "TMPDIR",
    ] {
        assert!(
            Path::new(&environment[variable]).starts_with(&root),
            "provider state must be isolated"
        );
    }
    let workdir = root.join(&name).join("project");
    let home = HomeLayout::from_path(root.join(&name).join("cccc")).expect("CCCC home");
    home.initialize().expect("initialize");
    let store = cccc_core::GroupStore::new(home.clone()).expect("store");
    let title = format!("NATIVE-ACP-{}", uuid::Uuid::new_v4().simple());
    let mut group = store
        .create(&title, "isolated adapter probe")
        .expect("group");
    group.state = cccc_contracts::GroupState::Paused;
    for id in ["alpha", "beta"] {
        cccc_core::actors::add(&mut group, cccc_contracts::Actor::new(id)).expect("actor");
    }
    store.save(&group).expect("save");
    let launcher = crate::ops::codex_mcp::resolve_cccc_executable().expect("CCCC launcher");
    let daemon = std::process::Command::new(&launcher)
        .args(["daemon", "start"])
        .envs(&environment)
        .env("CCCC_HOME", home.root())
        .current_dir(&workdir)
        .output()
        .expect("start isolated daemon");
    assert!(daemon.status.success(), "isolated daemon startup failed");
    let _daemon = ProbeDaemon {
        launcher,
        environment: environment.clone(),
        home: home.root().to_owned(),
    };

    let mut command = cccc_runtime::default_command(runtime);
    command[0] = settings["executable"]
        .as_str()
        .expect("native executable")
        .into();
    let launch = |id: &str| ActorLaunchConfig {
        workdir: workdir.clone(),
        group_id: group.group_id.clone(),
        actor_id: id.into(),
        runtime,
        runtime_mode: RuntimeMode::Acp,
        command: command.clone(),
        environment: environment.clone(),
    };
    let alpha = AnalystSession::launch_actor(&home, launch("alpha"))
        .await
        .expect("Actor launch");
    let beta = AnalystSession::launch_actor(&home, launch("beta"))
        .await
        .expect("independent Actor launch");
    let alpha_id = alpha.thread_id().to_owned();
    assert_ne!(alpha_id, beta.thread_id());
    beta.stop(beta.generation()).await.expect("stop empty beta");
    let beta = AnalystSession::launch_actor(&home, launch("beta"))
        .await
        .expect("empty Actor restart");
    let beta_id = beta.thread_id().to_owned();
    assert_ne!(alpha_id, beta_id);
    beta.stop(beta.generation()).await.expect("stop beta");
    let voice_only = std::env::var("CCCC_NATIVE_ACP_LIVE_PHASE").as_deref() == Ok("voice");
    let actor_verified = if voice_only {
        alpha
            .stop(alpha.generation())
            .await
            .expect("stop unused Actor");
        false
    } else {
        let actor_result = std::panic::AssertUnwindSafe(async {
        let mut events = alpha.subscribe();
        spend(&root, &name);
        let turn = tokio::time::timeout(Duration::from_secs(90), alpha.start_turn(alpha.generation(), "actor-live", &format!(
            "Isolated adapter test. Call cccc_bootstrap once for group_id {}. Use the actual tool result to report session.actor_id and session.group_title only. Do not read files, run commands, send messages, or delegate. Keep the answer short.", group.group_id
        ))).await.expect("Actor admission deadline").expect("Actor admission");
        super::live_support::wait_for_turn_text(&mut events, &turn.turn_id).await
    }).catch_unwind().await;
        alpha.stop(alpha.generation()).await.expect("stop Actor");
        let actor_answer = actor_result.expect("Actor task");
        cccc_core::fs::write_json(
            &root.join(format!("{name}-actor.json")),
            &json!({"answer":actor_answer,"session_id":alpha_id,"other_session_id":beta_id}),
        )
        .expect("evidence");
        assert!(
            actor_answer.contains("alpha") && actor_answer.contains(&title),
            "Actor MCP routing not verified"
        );
        true
    };
    if actor_verified {
        let resumed = AnalystSession::launch_actor(&home, launch("alpha"))
            .await
            .expect("resume populated Actor");
        assert_eq!(resumed.thread_id(), alpha_id);
        assert!(resumed.thread_resumed);
        resumed
            .stop(resumed.generation())
            .await
            .expect("stop resumed Actor");
    }
    let config = LaunchConfig {
        workdir,
        runtime,
        runtime_mode: RuntimeMode::Acp,
        command,
        environment,
        resume_thread_id: None,
    };
    let voice = Arc::new(
        AnalystSession::launch(&home, config.clone())
            .await
            .expect("Voice launch"),
    );
    cccc_core::fs::write_json(
        &home.daemon_dir().join("codex_voice_analyst.json"),
        &json!({"thread_id":voice.thread_id(),"materialized":false}),
    )
    .expect("Voice host receipt");
    let lifecycle = AnalystLifecycle::start(voice.clone());
    let voice_result = std::panic::AssertUnwindSafe(async {
        let mut events = lifecycle.subscribe();
        spend(&root, &name);
        lifecycle.admit_voice("voice-live", &format!(
            "Isolated adapter test. Call cccc_group once with action info and group_id {}. Report the group title from the actual result only. Do not read files, run commands, send messages, or delegate. Keep the answer short.", group.group_id
        )).await.expect("queue Voice input");
        tokio::time::timeout(Duration::from_secs(150), async {
            loop {
                match events.recv().await.expect("Voice lifecycle event") {
                    AnalystLifecycleEvent::Completed { status, result, speakable, .. } => {
                        assert_eq!(status,"completed"); assert!(speakable); return result;
                    }
                    AnalystLifecycleEvent::Disconnected => panic!("Voice disconnected"),
                    _ => {}
                }
            }
        }).await.expect("Voice completion deadline")
    }).catch_unwind().await;
    let voice_id = voice.thread_id().to_owned();
    assert!(voice.resumable());
    drop(lifecycle);
    voice.stop(voice.generation()).await.expect("stop Voice");
    let voice_answer = voice_result.expect("Voice task");
    cccc_core::fs::write_json(
        &root.join(format!("{name}-voice.json")),
        &json!({"answer":voice_answer,"session_id":voice_id}),
    )
    .expect("evidence");
    assert!(
        voice_answer.contains(&title),
        "Voice MCP routing not verified"
    );
    let mut config = config;
    config.resume_thread_id = Some(voice_id.clone());
    let resumed = AnalystSession::launch(&home, config)
        .await
        .expect("resume populated Voice");
    assert_eq!(resumed.thread_id(), voice_id);
    assert!(resumed.thread_resumed);
    resumed
        .stop(resumed.generation())
        .await
        .expect("stop resumed Voice");
    cccc_core::fs::write_json(&root.join(format!("{name}-passed.json")), &json!({"runtime":name,"actor_mcp":actor_verified,"voice_lifecycle_mcp":true,"independent_actor_sessions":true,"empty_restart":true,"actor_resume":actor_verified,"voice_resume":true,"prompts":if voice_only {1} else {2}})).expect("proof");
}
