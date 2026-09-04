use crate::test_support::test_guard;
use crate::{LaunchSpec, history, start, status, stop, submit};
use cccc_contracts::RunnerKind;
use std::collections::BTreeMap;
use std::time::Duration;

#[test]
fn npm_style_batch_actor_survives_utf8_message_delivery() {
    let _guard = test_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    let actor_dir = temp.path().join("batch actor");
    std::fs::create_dir(&actor_dir).expect("batch actor directory");
    std::fs::write(actor_dir.join("actor"), "not a Win32 executable")
        .expect("extensionless npm shim");
    std::fs::write(actor_dir.join("actor.cmd"), "@echo off\r\ncmd.exe /Q\r\n").expect("batch shim");
    let path = std::env::join_paths(std::iter::once(actor_dir).chain(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    )))
    .expect("PATH")
    .to_string_lossy()
    .into_owned();
    let group = "g_windows_delivery";
    let actor = "peer1";
    start(LaunchSpec {
        group_id: group.into(),
        actor_id: actor.into(),
        runner: RunnerKind::Pty,
        command: vec!["actor".into()],
        cwd: temp.path().into(),
        env: BTreeMap::from([("PATH".into(), path)]),
        cols: 80,
        rows: 24,
    })
    .expect("start actor through .cmd shim");

    submit(
        group,
        actor,
        "echo PONG-自检".as_bytes(),
        b"\r",
        Duration::ZERO,
    )
    .expect("submit UTF-8 message");
    for _ in 0..100 {
        let output = history(group, actor, None, 4096).expect("history").data;
        if output.contains("PONG-自检") {
            assert!(status(group, actor).expect("status").running);
            stop(group, actor).expect("cleanup");
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = history(group, actor, None, 4096).expect("history").data;
    let _ = stop(group, actor);
    panic!("actor did not echo delivered UTF-8 text: {output:?}");
}
