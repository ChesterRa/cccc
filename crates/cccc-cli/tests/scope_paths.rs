use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::Command;

#[test]
fn scope_commands_resolve_paths_in_the_invoking_terminal() {
    let temp = tempfile::tempdir().expect("temp");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let project = temp.path().join("project with spaces");
    let nested = project.join("nested");
    std::fs::create_dir_all(&nested).expect("project");
    let absolute = nested.to_string_lossy().into_owned();
    let cases = [
        (vec!["attach"], "attach", &project, Value::Null),
        (vec!["attach", "."], "attach", &project, Value::Null),
        (
            vec!["attach", "nested", "--group", "g_existing"],
            "attach",
            &nested,
            json!("g_existing"),
        ),
        (vec!["attach", &absolute], "attach", &nested, Value::Null),
        (
            vec!["group", "use", "g_existing"],
            "group_use",
            &project,
            json!("g_existing"),
        ),
        (
            vec!["group", "use", "g_existing", "nested"],
            "group_use",
            &nested,
            json!("g_existing"),
        ),
        (
            vec!["group", "use", "g_existing", &absolute],
            "group_use",
            &nested,
            json!("g_existing"),
        ),
    ];
    let server = echo_daemon(&home, cases.len());
    let mut results = Vec::new();
    for (args, _, _, _) in &cases {
        results.push(run(&home, &project, args));
    }
    server.join().expect("IPC joined");
    for ((args, op, expected_path, group_id), result) in cases.into_iter().zip(results) {
        assert_eq!(result["op"], op, "{args:?}");
        let path = std::path::Path::new(result["args"]["path"].as_str().expect("path"));
        assert!(path.is_absolute(), "{args:?}: relative path sent over IPC");
        assert_eq!(
            path.canonicalize().expect("requested path"),
            expected_path.canonicalize().expect("project path"),
            "{args:?}"
        );
        assert_eq!(result["args"]["group_id"], group_id, "{args:?}");
        assert_eq!(result["args"]["by"], "user", "{args:?}");
    }
}

#[test]
fn send_gives_the_daemon_a_full_path_and_leaves_the_scope_to_it() {
    let temp = tempfile::tempdir().expect("temp");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&project).expect("project");
    let store = cccc_core::GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("scoped send", "").expect("group");
    group
        .scopes
        .push(cccc_core::scope::detect(&project).expect("scope"));
    store.save(&group).expect("save group");
    let server = echo_daemon(&home, 1);

    let sent = run(
        &home,
        &project,
        &["send", "hello", "--group", &group.group_id, "--path", "."],
    );
    server.join().expect("IPC joined");

    assert_eq!(sent["op"], "message_send");
    let path = std::path::Path::new(sent["args"]["path"].as_str().expect("path"));
    assert!(path.is_absolute(), "relative path sent over IPC");
    assert_eq!(
        path.canonicalize().expect("requested path"),
        project.canonicalize().expect("project path")
    );
    assert!(sent["args"].get("scope_key").is_none());
}

/// Answers each of `count` requests with what it received.
fn echo_daemon(home: &HomeLayout, count: usize) -> std::thread::JoinHandle<()> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("IPC");
    cccc_core::fs::write_json(
        &cccc_daemon::DaemonPaths::new(home.clone()).address,
        &json!({"v":1,"transport":"tcp","path":"","host":"127.0.0.1",
            "port":listener.local_addr().expect("port").port(),"pid":std::process::id(),
            "version":env!("CARGO_PKG_VERSION"),"ts":"2026-10-10T00:00:00Z"}),
    )
    .expect("address");
    std::thread::spawn(move || {
        for stream in listener.incoming().take(count) {
            let stream = stream.expect("accept");
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .expect("timeout");
            let mut stream = BufReader::new(stream);
            let mut line = String::new();
            stream.read_line(&mut line).expect("request");
            let request: DaemonRequest = serde_json::from_str(&line).expect("JSON");
            writeln!(
                stream.get_mut(),
                "{}",
                json!({"ok":true,"result":{"op":request.op,"args":request.args}})
            )
            .expect("response");
        }
    })
}

fn run(home: &HomeLayout, directory: &std::path::Path, args: &[&str]) -> Value {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cccc"));
    for (key, _) in std::env::vars().filter(|(key, _)| key.starts_with("CCCC_")) {
        command.env_remove(key);
    }
    let output = command
        .args(args)
        .current_dir(directory)
        .env("CCCC_HOME", home.root())
        .env("CODEX_HOME", home.root().join("codex"))
        .output()
        .expect("CLI");
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice::<Value>(&output.stdout).expect("result")
}
