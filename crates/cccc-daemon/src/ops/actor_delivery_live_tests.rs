//! Opt-in native CLI probes. All provider/config/session state is isolated;
//! the deliberately offline model never consumes credentials or quota.
use cccc_contracts::{Actor, ActorRuntime, RunnerKind};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

#[test]
fn live_kimi_code_captures_native_delivery_and_resumes_the_same_session() {
    if std::env::var("CCCC_KIMI_CODE_LIVE").as_deref() != Ok("1") {
        return;
    }
    let executable = std::env::var("CCCC_KIMI_EXECUTABLE").unwrap_or_else(|_| "kimi".into());
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("project");
    let config = temp.path().join("kimi-code");
    std::fs::create_dir_all(&root).expect("project");
    std::fs::create_dir_all(&config).expect("config directory");
    std::fs::write(
        config.join("config.toml"),
        r#"
default_model = "offline"
telemetry = false
[providers.offline]
type = "openai"
base_url = "http://127.0.0.1:9/v1"
api_key = "cccc-local-input-test"
[models.offline]
provider = "offline"
model = "offline"
max_context_size = 128000
[loop_control]
max_attempts_per_step = 1
"#,
    )
    .expect("isolated offline config");
    let env = BTreeMap::from([
        ("HOME".into(), temp.path().to_string_lossy().into_owned()),
        (
            "USERPROFILE".into(),
            temp.path().to_string_lossy().into_owned(),
        ),
        (
            "KIMI_CODE_HOME".into(),
            config.to_string_lossy().into_owned(),
        ),
        ("TERM".into(), "xterm-256color".into()),
        ("KIMI_STARTUP_TRACE".into(), "1".into()),
        (
            "KIMI_STARTUP_TRACE_LOG".into(),
            config.join("startup.log").to_string_lossy().into_owned(),
        ),
    ]);
    let group_id = uuid::Uuid::new_v4().simple().to_string();
    let mut actor = Actor::new("kimi-live");
    actor.runtime = ActorRuntime::Kimi;
    let first = format!(
        "First native input.\n{}\nKIMI_TAIL_7e42",
        "x".repeat(16_038)
    );
    let mut session_id: Option<String> = None;
    for text in [&first, "Second input after exact session resume."] {
        // This diagnostic trace is only a live-test synchronization aid. It
        // is deliberately not a production Runtime readiness dependency.
        let _ = std::fs::remove_file(config.join("startup.log"));
        let mut command = vec![executable.clone(), "--yolo".into()];
        if let Some(id) = &session_id {
            command.extend(["--session".into(), id.clone()]);
        }
        cccc_runtime::start(cccc_runtime::LaunchSpec {
            group_id: group_id.clone(),
            actor_id: actor.id.clone(),
            runner: RunnerKind::Pty,
            command,
            cwd: root.clone(),
            env: env.clone(),
            cols: 120,
            rows: 40,
        })
        .expect("start isolated Kimi TUI");
        let result = (|| {
            if !cccc_runtime::wait_for_input_ready(
                &group_id,
                &actor.id,
                Duration::from_secs(15),
                &AtomicBool::new(false),
            )
            .unwrap_or(false)
            {
                return Err("Kimi input mode did not become available");
            }
            if session_id.is_none() {
                // The operator, not a delivered message, accepts trust in
                // this freshly-created isolated test workspace.
                std::thread::sleep(Duration::from_millis(250));
                cccc_runtime::write(&group_id, &actor.id, b"\r")
                    .map_err(|_| "could not accept test workspace")?;
            }
            let deadline = Instant::now() + Duration::from_secs(10);
            while !std::fs::read_to_string(config.join("startup.log"))
                .unwrap_or_default()
                .contains("finishStartup:end")
            {
                if Instant::now() >= deadline {
                    return Err("Kimi operator initialization did not finish");
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            if !crate::ops::actor_delivery::submit_terminal_text(
                &group_id,
                &actor,
                text,
                &AtomicBool::new(false),
            ) {
                return Err("native delivery rejected");
            }
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                if records_contain(&config.join("sessions"), text) {
                    let index = std::fs::read_to_string(config.join("session_index.jsonl"))
                        .map_err(|_| "session index missing")?;
                    let records: Vec<Value> = index
                        .lines()
                        .filter_map(|line| serde_json::from_str(line).ok())
                        .collect();
                    let id = records
                        .last()
                        .and_then(|record| record["sessionId"].as_str())
                        .ok_or("durable session ID missing")?
                        .to_owned();
                    return Ok(id);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            eprintln!(
                "Kimi terminal before timeout: {:?}",
                cccc_runtime::retained_history_tail(&group_id, &actor.id, 4000)
            );
            eprintln!(
                "Kimi startup phases: {}",
                std::fs::read_to_string(config.join("startup.log")).unwrap_or_default()
            );
            Err("native input was not recorded in the provider session")
        })();
        let stopped = cccc_runtime::stop(&group_id, &actor.id);
        stopped.expect("stop isolated Kimi terminal");
        let id = result.expect("capture complete native input");
        if let Some(previous) = &session_id {
            assert_eq!(&id, previous, "resume must keep the exact session");
        }
        session_id = Some(id);
    }
}

fn records_contain(root: &Path, text: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(root) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && records_contain(&path, text) {
            return true;
        }
        if path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(&path) {
            for line in content.lines() {
                if let Ok(record) = serde_json::from_str::<Value>(line)
                    && value_contains(&record, text)
                {
                    return true;
                }
            }
        }
    }
    false
}

fn value_contains(value: &Value, text: &str) -> bool {
    match value {
        Value::String(value) => value == text,
        Value::Array(values) => values.iter().any(|value| value_contains(value, text)),
        Value::Object(values) => values.values().any(|value| value_contains(value, text)),
        _ => false,
    }
}
