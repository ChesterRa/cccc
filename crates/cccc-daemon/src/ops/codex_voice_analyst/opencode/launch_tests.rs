use super::*;
use cccc_contracts::ActorRuntime;
use std::fs;
use std::path::PathBuf;

fn npm_install(root: &Path, local: bool) -> (PathBuf, PathBuf) {
    let modules = root.join("node_modules");
    let bin = if local {
        modules.join(".bin")
    } else {
        root.to_owned()
    };
    let script = modules.join("@kilocode/cli/bin/kilo");
    fs::create_dir_all(script.parent().expect("script directory")).expect("package directory");
    fs::create_dir_all(&bin).expect("shim directory");
    fs::write(&script, "#!/usr/bin/env node\n").expect("npm entrypoint");
    let relative = if local {
        r"..\@kilocode\cli\bin\kilo"
    } else {
        r"node_modules\@kilocode\cli\bin\kilo"
    };
    let shim = bin.join("kilo.cmd");
    fs::write(
        &shim,
        format!("@ECHO off\r\n\"%~dp0\\node.exe\" \"%~dp0\\{relative}\" %*\r\n"),
    )
    .expect("npm shim");
    fs::write(bin.join("node.exe"), "fixture").expect("bundled node fixture");
    (shim, script)
}

#[test]
fn windows_npm_kilo_global_and_local_installations_prepare() {
    for local in [false, true] {
        let temp = tempfile::tempdir().expect("tempdir");
        let (shim, script) = npm_install(&temp.path().join("npm & tools 空间"), local);
        let prepared = prepare(
            &[
                shim.to_string_lossy().into_owned(),
                "--model=provider/model".into(),
                "--pure".into(),
            ],
            &BTreeMap::new(),
            ActorRuntime::Kilo,
        )
        .unwrap_or_else(|error| {
            panic!("official npm install (local={local}) must launch: {error}")
        });
        assert_eq!(prepared.model.as_deref(), Some("provider/model"));
        assert_eq!(prepared.acp_arguments, ["--pure"]);
        assert_eq!(prepared.tui_arguments, ["--pure"]);
        assert_eq!(
            prepared.launch_prefix,
            [
                shim.parent()
                    .expect("shim directory")
                    .join("node.exe")
                    .to_string_lossy()
                    .into_owned(),
                script.to_string_lossy().into_owned(),
            ]
        );
    }
}

#[test]
fn npm_launch_uses_node_from_the_effective_path_when_not_bundled() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (shim, script) = npm_install(temp.path(), false);
    fs::remove_file(temp.path().join("node.exe")).expect("remove bundled node fixture");
    let node_dir = temp.path().join("Node & tools");
    fs::create_dir(&node_dir).expect("PATH node directory");
    let node = node_dir.join("node.exe");
    fs::write(&node, "fixture").expect("PATH node fixture");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&node, fs::Permissions::from_mode(0o700)).expect("executable fixture");
    }
    let env = BTreeMap::from([("PATH".into(), node_dir.to_string_lossy().into_owned())]);
    let prepared = prepare(
        &[shim.to_string_lossy().into_owned()],
        &env,
        ActorRuntime::Kilo,
    )
    .expect("prepare with PATH node");
    assert_eq!(
        prepared.launch_prefix,
        [node.to_string_lossy(), script.to_string_lossy()]
    );
}

#[test]
fn npm_launch_reports_missing_package_or_node_without_executing_the_shim() {
    let temp = tempfile::tempdir().expect("tempdir");
    let (shim, script) = npm_install(temp.path(), false);
    let configured = [shim.to_string_lossy().into_owned()];
    fs::remove_file(temp.path().join("node.exe")).expect("remove bundled node fixture");
    let env = BTreeMap::from([("PATH".into(), temp.path().to_string_lossy().into_owned())]);
    let error = prepare(&configured, &env, ActorRuntime::Kilo)
        .err()
        .expect("missing node must fail");
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert!(error.to_string().contains("node.exe"));
    fs::remove_file(script).expect("remove npm entrypoint fixture");
    let error = prepare(&configured, &env, ActorRuntime::Kilo)
        .err()
        .expect("missing package must fail");
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert!(error.to_string().contains("Kilo npm entrypoint is missing"));
}

#[test]
fn direct_executables_are_unchanged_and_custom_wrappers_stay_unsupported() {
    let temp = tempfile::tempdir().expect("tempdir");
    for (runtime, name) in [
        (ActorRuntime::Kilo, "kilo"),
        (ActorRuntime::Opencode, "opencode"),
    ] {
        for filename in [name.to_owned(), format!("{name}.exe")] {
            let executable = temp.path().join(filename).to_string_lossy().into_owned();
            fs::write(&executable, "fixture").expect("direct executable fixture");
            let prepared = prepare(&[executable.clone()], &BTreeMap::new(), runtime)
                .expect("direct executable");
            assert_eq!(prepared.launch_prefix, [executable]);
        }
        let wrapper = temp
            .path()
            .join("custom.cmd")
            .to_string_lossy()
            .into_owned();
        fs::write(&wrapper, "fixture").expect("custom wrapper fixture");
        let error = prepare(&[wrapper], &BTreeMap::new(), runtime)
            .err()
            .expect("unsupported custom wrapper");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }
}

/// Exercise the actual Windows process and ConPTY paths, not only argv assembly.
/// This npm-layout fixture needs Node but no Kilo download, credentials or model.
#[cfg(windows)]
#[test]
fn windows_npm_kilo_entrypoint_runs_through_owned_stdio_and_native_terminal() {
    use std::io::{BufRead, BufReader, Write};
    let node = cccc_runtime::resolve_executable_in_path("node.exe", None)
        .expect("install Node for Windows smoke tests");
    let temp = tempfile::tempdir().expect("tempdir");
    let (shim, script) = npm_install(&temp.path().join("npm & tools 空间"), false);
    fs::remove_file(shim.parent().expect("shim directory").join("node.exe"))
        .expect("use installed Node");
    // Model the official launcher's inherited stdio/environment and child tree.
    fs::write(&script, r#"
const worker = `
const fs = require('fs');
fs.writeFileSync(process.env.CCCC_KILO_FIXTURE_OUTPUT, JSON.stringify({args:process.argv.slice(1), value:process.env.KILO_FIXTURE_VALUE}));
process.stdin.on('data', bytes => process.stdout.write(bytes));
process.stdin.resume();`;
require('child_process').spawn(process.execPath, ['-e', worker, ...process.argv.slice(2)], {stdio:'inherit'});
"#).expect("npm launcher fixture");
    let output = temp.path().join("observed.json");
    let path = std::env::join_paths([
        shim.parent().expect("shim directory"),
        node.parent().expect("Node directory"),
    ])
    .expect("fixture PATH");
    let mut env = BTreeMap::from([
        ("PATH".into(), path.to_string_lossy().into_owned()),
        (
            "CCCC_KILO_FIXTURE_OUTPUT".into(),
            output.to_string_lossy().into_owned(),
        ),
        ("KILO_FIXTURE_VALUE".into(), "literal %PATH% & 私有".into()),
    ]);
    let prepared = prepare(&["kilo".into()], &env, ActorRuntime::Kilo)
        .expect("resolve official npm shim from PATH");
    let arguments = ["acp", "--cwd", "C:\\space & 路径", "%PATH%", "quote\"value"];
    let mut command = prepared.launch_prefix.clone();
    command.extend(arguments.map(str::to_owned));
    let (owner, mut stdin, stdout) =
        process::spawn_piped(&command, temp.path(), &env, "kilo-fixture")
            .expect("owned stdio launch");
    let (sender, receiver) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut line = String::new();
        let result = BufReader::new(stdout).read_line(&mut line).map(|_| line);
        let _ = sender.send(result);
    });
    writeln!(stdin, "ACP-回传").expect("stdio input");
    let observed = receiver.recv_timeout(Duration::from_secs(10));
    owner.stop().expect("stop owned npm process tree");
    reader.join().expect("join stdio reader");
    assert_eq!(
        observed
            .expect("bounded output")
            .expect("read output")
            .trim(),
        "ACP-回传"
    );
    let record: Value =
        serde_json::from_slice(&fs::read(&output).expect("worker output")).expect("worker JSON");
    assert_eq!(record["args"], json!(arguments));
    assert_eq!(record["value"], env["KILO_FIXTURE_VALUE"]);
    assert!(!owner.running());

    let output = temp.path().join("terminal.json");
    env.insert(
        "CCCC_KILO_FIXTURE_OUTPUT".into(),
        output.to_string_lossy().into_owned(),
    );
    let mut command = prepared.launch_prefix;
    command.extend([
        "attach".into(),
        "http://127.0.0.1:1234".into(),
        "--session".into(),
        "exact-session".into(),
    ]);
    let group = uuid::Uuid::new_v4().simple().to_string();
    cccc_runtime::start(cccc_runtime::LaunchSpec {
        group_id: group.clone(),
        actor_id: "kilo-fixture".into(),
        runner: cccc_contracts::RunnerKind::Pty,
        command,
        cwd: temp.path().into(),
        env,
        cols: 120,
        rows: 30,
    })
    .expect("native terminal launch");
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let record = loop {
        if let Ok(bytes) = fs::read(&output)
            && let Ok(value) = serde_json::from_slice::<Value>(&bytes)
        {
            break Some(value);
        }
        if std::time::Instant::now() >= deadline {
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    cccc_runtime::stop(&group, "kilo-fixture").expect("stop native terminal tree");
    assert_eq!(
        record.expect("native terminal launched")["args"],
        json!([
            "attach",
            "http://127.0.0.1:1234",
            "--session",
            "exact-session"
        ])
    );
}
