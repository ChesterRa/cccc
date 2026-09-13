//! Offline PTY fixtures for ready input and the early paste-mode startup race.
//! They do not establish provider receipt or completion of login/trust dialogs.
use super::*;
use cccc_contracts::{Event, RunnerKind};
use cccc_core::{GroupStore, HomeLayout};
use std::collections::BTreeMap;

#[test]
fn antigravity_delivers_automatically_without_terminal_labels_after_restart() {
    exercise_startup("\x1b[2J\x1b[HAn entirely different UI\r\n>", false);
}

#[test]
fn antigravity_first_payload_survives_early_input_mode_during_initialization() {
    exercise_startup("Initializing a native terminal", true);
}

#[test]
fn antigravity_captured_narrow_prompt_delivers_automatically() {
    exercise_captured_startup("narrow");
}

#[test]
fn antigravity_captured_wide_prompt_delivers_automatically() {
    exercise_captured_startup("wide");
}

fn exercise_captured_startup(layout: &str) {
    // Sanitized native 1.2.2 output from the reported blocked sessions. The
    // right-aligned model status must not affect whether input is submitted.
    let frames: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/antigravity_ready_1_2_2.json"))
            .expect("captured frames");
    exercise_startup(frames[layout].as_str().expect("captured layout"), false);
}

fn exercise_startup(prompt: &str, slow_start: bool) {
    let temp = tempfile::tempdir().expect("fixture");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("startup", "").expect("group");
    let mut actor = Actor::new("agy");
    actor.runtime = ActorRuntime::Antigravity;
    group.actors.push(actor.clone());
    store.save(&group).expect("save");
    let script = temp.path().join("terminal.py");
    std::fs::write(temp.path().join("prompt.ansi"), prompt).expect("prompt frame");
    std::fs::write(
        &script,
        r#"
import os,sys,tty,pathlib,json,time,select
root=pathlib.Path(sys.argv[1]);tty.setraw(0)
os.write(1,b'\x1b[?2004h'+(root/'prompt.ansi').read_bytes())
(root/'ready').touch()
# Some native TUIs enable paste mode before mounting the conversation input.
# Deliberately discard startup input, like the reported native initialization.
if sys.argv[2] == 'slow':
 deadline=time.monotonic()+0.8
 while time.monotonic()<deadline:
  if select.select([0],[],[],0.01)[0]:os.read(0,65536)
buf=b''
while True:
 data=os.read(0,65536)
 if not data:break
 buf+=data
 while b'\r' in buf:
  text,buf=buf.split(b'\r',1)
  with (root/'received').open('a') as f:f.write(json.dumps(text.decode())+'\n')
"#,
    )
    .expect("write fixture");
    let mut preamble = String::new();
    let cancelled = AtomicBool::new(false);
    for cycle in 0..2 {
        if cycle > 0 {
            std::fs::remove_file(temp.path().join("ready")).expect("clear fixture signal");
            std::fs::remove_file(temp.path().join("received")).expect("clear fixture input");
        }
        cccc_runtime::start(cccc_runtime::LaunchSpec {
            group_id: group.group_id.clone(),
            actor_id: actor.id.clone(),
            runner: RunnerKind::Pty,
            command: vec![
                "python3".into(),
                script.to_string_lossy().into_owned(),
                temp.path().to_string_lossy().into_owned(),
                if slow_start { "slow" } else { "ready" }.into(),
            ],
            cwd: temp.path().to_path_buf(),
            env: BTreeMap::new(),
            cols: 120,
            rows: 40,
        })
        .expect("start fixture");
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            wait_for(|| temp.path().join("ready").exists());
            assert!(
                !temp.path().join("received").exists(),
                "idle startup submits no prompt"
            );
            let mut event = Event::new("chat.message", &group.group_id);
            event.by = "user".into();
            event.data =
                serde_json::json!({"to":["agy"],"text":"FIRST_TASK","message_mode":"send"})
                    .as_object()
                    .expect("event data")
                    .clone();
            let job = DeliveryJob {
                home: home.clone(),
                group: group.clone(),
                actor: actor.clone(),
                event,
            };
            assert!(
                process_batch(std::slice::from_ref(&job), &mut preamble, &cancelled),
                "first task must be delivered without a Web attachment or confirmation"
            );
            let mut second = job;
            second.event.id = uuid::Uuid::new_v4().simple().to_string();
            second
                .event
                .data
                .insert("text".into(), "SECOND_TASK".into());
            assert!(process_batch(&[second], &mut preamble, &cancelled));
            wait_for(|| {
                std::fs::read_to_string(temp.path().join("received"))
                    .is_ok_and(|text| text.lines().count() >= 2)
            });
            let lines = std::fs::read_to_string(temp.path().join("received")).expect("read input");
            let messages: Vec<String> = lines
                .lines()
                .map(|line| serde_json::from_str(line).expect("input record"))
                .collect();
            assert_eq!(
                messages.len(),
                2,
                "one submission per task after start or restart"
            );
            assert!(!messages[0].contains("MCP setup request"));
            assert_eq!(messages[0].matches("[CCCC] You are agy").count(), 1);
            assert!(messages[0].contains("cccc_bootstrap"));
            assert!(messages[0].contains("FIRST_TASK"));
            assert!(messages[0].find("cccc_bootstrap") < messages[0].find("FIRST_TASK"));
            assert!(!messages[1].contains("MCP setup request"));
            assert!(!messages[1].contains("[CCCC] You are agy"));
            assert!(messages[1].contains("Otherwise continue without repeating bootstrap"));
            assert!(messages[1].contains("SECOND_TASK"));
        }));
        cccc_runtime::stop(&group.group_id, &actor.id).expect("stop fixture");
        if let Err(error) = result {
            std::panic::resume_unwind(error);
        }
    }
}

fn wait_for(mut ready: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while !ready() {
        assert!(std::time::Instant::now() < deadline, "fixture timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
}
