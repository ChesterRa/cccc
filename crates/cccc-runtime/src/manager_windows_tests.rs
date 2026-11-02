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

#[test]
fn pty_actor_starts_in_the_drive_path_of_its_working_directory() {
    // cmd.exe started in a \\?\ directory falls back to the Windows directory.
    let _guard = test_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    let group = "g_windows_cwd";
    let actor = "peer1";
    let cmd = std::env::var("ComSpec").expect("ComSpec");
    start(LaunchSpec {
        group_id: group.into(),
        actor_id: actor.into(),
        runner: RunnerKind::Pty,
        command: vec![cmd, "/d".into(), "/q".into(), "/k".into(), "cd".into()],
        cwd: temp.path().into(),
        env: BTreeMap::new(),
        cols: 200,
        rows: 24,
    })
    .expect("start cmd");
    let root = temp
        .path()
        .canonicalize()
        .expect("canonical working directory");
    let expected = crate::compatible_path(&root).to_string_lossy().into_owned();
    for _ in 0..100 {
        let output = history(group, actor, None, 8192).expect("history").data;
        // The fallback warning quotes the \\?\ path, which contains the expected text.
        if output.contains(&expected) && !output.contains(r"\\?\") {
            stop(group, actor).expect("cleanup");
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let output = history(group, actor, None, 8192).expect("history").data;
    let _ = stop(group, actor);
    panic!("cmd did not start in {expected}: {output:?}");
}

#[test]
fn cmd_actors_are_refused_in_a_reserved_name_directory() {
    let _guard = test_guard();
    let temp = tempfile::tempdir().expect("tempdir");
    // Only the \\?\ form names it, and cmd.exe treats that form like a UNC path.
    let reserved = temp
        .path()
        .canonicalize()
        .expect("canonical temp")
        .join("aux");
    std::fs::create_dir(&reserved).expect("reserved-name directory");
    assert_cmd_actors_refused_in(&reserved);
    std::fs::remove_dir(&reserved).expect("remove reserved-name directory");
}

#[test]
#[ignore = "needs a reachable UNC directory in CCCC_TEST_UNC_DIR"]
fn cmd_actors_are_refused_in_a_unc_working_directory() {
    let _guard = test_guard();
    let unc = std::env::var_os("CCCC_TEST_UNC_DIR").expect("CCCC_TEST_UNC_DIR");
    assert_cmd_actors_refused_in(std::path::Path::new(&unc));
}

fn assert_cmd_actors_refused_in(cwd: &std::path::Path) {
    let temp = tempfile::tempdir().expect("tempdir");
    let shim = temp.path().join("actor.cmd");
    std::fs::write(&shim, "@echo off\r\ncmd.exe /Q\r\n").expect("batch shim");
    let cmd = std::env::var("ComSpec").expect("ComSpec");
    for command in [
        vec![cmd, "/d".into(), "/q".into(), "/k".into(), "cd".into()],
        vec![shim.to_string_lossy().into_owned()],
    ] {
        let error = start(LaunchSpec {
            group_id: "g_windows_prefixed_cwd".into(),
            actor_id: "peer1".into(),
            runner: RunnerKind::Pty,
            command: command.clone(),
            cwd: cwd.into(),
            env: BTreeMap::new(),
            cols: 80,
            rows: 24,
        })
        .expect_err("cmd.exe must not start in this directory");
        assert!(
            matches!(error, crate::RuntimeError::CmdInPrefixedDirectory(_)),
            "{command:?}: {error}"
        );
    }
}
