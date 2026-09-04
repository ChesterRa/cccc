use super::windows::wrap_resolved_command;
use super::{prepare_pty_command_for, resolve_executable};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[test]
fn windows_resolution_skips_extensionless_shim_and_follows_pathext_order() {
    let temp = tempfile::tempdir().expect("tempdir");
    fs::write(temp.path().join("claude"), "#!/bin/sh").expect("shim");
    fs::write(temp.path().join("claude.cmd"), "@echo off").expect("cmd shim");
    fs::write(temp.path().join("claude.exe"), "binary").expect("exe shim");

    assert_eq!(
        resolve_executable(
            "claude",
            Some(&temp.path().display().to_string()),
            true,
            Some(".CMD;.EXE")
        ),
        Some(temp.path().join("claude.cmd"))
    );
    fs::remove_file(temp.path().join("claude.cmd")).expect("remove cmd");
    assert_eq!(
        resolve_executable(
            "claude",
            Some(&temp.path().display().to_string()),
            true,
            Some(".CMD;.EXE")
        ),
        Some(temp.path().join("claude.exe"))
    );
}

#[test]
fn windows_resolution_does_not_restore_extensions_excluded_by_pathext() {
    let temp = tempfile::tempdir().expect("tempdir");
    fs::write(temp.path().join("tool.exe"), "binary").expect("exe");

    assert_eq!(
        resolve_executable(
            "tool",
            Some(&temp.path().display().to_string()),
            true,
            Some(".CMD")
        ),
        None
    );
}

#[test]
fn windows_batch_shims_are_routed_through_command_processor() {
    assert_eq!(
        wrap_resolved_command(
            &["claude".into(), "--version".into()],
            Path::new(r"C:\Program Files\npm\claude.cmd"),
            true,
            Some(r"C:\Windows\System32\cmd.exe")
        ),
        [
            r"C:\Windows\System32\cmd.exe",
            "/D",
            "/S",
            "/C",
            "call",
            r"C:\Program Files\npm\claude.cmd",
            "--version",
        ]
    );
}

#[test]
fn windows_batch_arguments_are_escaped_after_call_prefix() {
    assert_eq!(
        wrap_resolved_command(
            &[
                "tool".into(),
                "two words".into(),
                "left&right".into(),
                "%PATH%".into(),
                String::new(),
            ],
            Path::new(r"C:\Program Files\tools\tool.bat"),
            true,
            None,
        ),
        [
            "cmd.exe",
            "/D",
            "/S",
            "/C",
            "call",
            r"C:\Program Files\tools\tool.bat",
            "two words",
            "left^&right",
            "%%PATH%%",
            "",
        ]
    );
}

#[test]
fn windows_batch_program_path_is_escaped_before_cmd_parses_it() {
    assert_eq!(
        wrap_resolved_command(
            &["tool".into(), "--version".into()],
            Path::new(r"C:\tools&more\actor.cmd"),
            true,
            None,
        ),
        [
            "cmd.exe",
            "/D",
            "/S",
            "/C",
            "call",
            r"C:\tools^&more\actor.cmd",
            "--version",
        ]
    );
}

#[test]
fn windows_actor_environment_keys_are_case_insensitive() {
    let temp = tempfile::tempdir().expect("tempdir");
    let inherited = temp.path().join("inherited");
    let custom = temp.path().join("custom");
    fs::create_dir_all(&inherited).expect("inherited directory");
    fs::create_dir_all(&custom).expect("custom directory");
    fs::write(inherited.join("actor.exe"), "wrong executable").expect("inherited actor");
    fs::write(custom.join("actor.cmd"), "@echo off").expect("custom actor");

    let command = prepare_pty_command_for(
        &["actor".into(), "--version".into()],
        &BTreeMap::from([
            ("PATH".into(), inherited.display().to_string()),
            ("Path".into(), custom.display().to_string()),
            ("PATHEXT".into(), ".EXE".into()),
            ("pathext".into(), ".CMD".into()),
            ("COMSPEC".into(), r"C:\wrong\cmd.exe".into()),
            ("ComSpec".into(), r"C:\Windows\System32\cmd.exe".into()),
        ]),
        true,
    );

    assert_eq!(
        command,
        [
            r"C:\Windows\System32\cmd.exe".to_owned(),
            "/D".into(),
            "/S".into(),
            "/C".into(),
            "call".into(),
            custom.join("actor.cmd").display().to_string(),
            "--version".into(),
        ]
    );
}
