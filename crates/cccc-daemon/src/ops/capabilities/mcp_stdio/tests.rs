use super::{StderrTail, call_tool, list_tools};
use serde_json::{Map, Value, json};
use std::path::PathBuf;
use std::time::{Duration, Instant};

struct Fixture {
    _temp: tempfile::TempDir,
    command: Vec<String>,
    evidence: PathBuf,
}

impl Fixture {
    fn new(mode: &str) -> Self {
        let temp = tempfile::tempdir().expect("fixture dir");
        let script = temp.path().join("server.py");
        std::fs::write(&script, super::STDIO_FIXTURE).expect("fixture");
        let evidence = temp.path().join("evidence.jsonl");
        let python = ["python3", "python"]
            .into_iter()
            .find(|name| {
                std::process::Command::new(name)
                    .arg("--version")
                    .output()
                    .is_ok_and(|result| result.status.success())
            })
            .expect("Python is required for the stdio fixture");
        Self {
            command: vec![
                python.into(),
                "-u".into(),
                script.to_string_lossy().into_owned(),
                mode.into(),
                evidence.to_string_lossy().into_owned(),
            ],
            evidence,
            _temp: temp,
        }
    }

    fn rows(&self) -> Vec<Value> {
        std::fs::read_to_string(&self.evidence)
            .expect("evidence")
            .lines()
            .map(|line| serde_json::from_str(line).expect("JSON evidence"))
            .collect()
    }
}

#[test]
fn waits_for_handshake_and_response_then_closes_a_live_server() {
    let fixture = Fixture::new("normal");
    let tools = list_tools(&fixture.command, &Map::new(), Duration::from_secs(4)).expect("list");
    assert_eq!(tools.len(), 2, "all discovery pages must be read");
    let methods = fixture
        .rows()
        .iter()
        .filter_map(|row| row["method"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    assert_eq!(
        methods,
        [
            "initialize",
            "notifications/initialized",
            "tools/list",
            "tools/list"
        ]
    );
    assert!(fixture.rows().iter().any(|row| row["eof"] == true));
    let called = call_tool(
        &fixture.command,
        &Map::new(),
        "echo",
        Map::new(),
        Duration::from_secs(4),
    )
    .expect("call");
    assert_eq!(called["content"][0]["text"], "stdio-fixture-result");
}

#[test]
fn failed_install_preserves_the_reason_and_redacts_private_values() {
    let fixture = Fixture::new("exit");
    let error = list_tools(
        &fixture.command,
        &json!({"FIXTURE_TOKEN":"fixture-private-token"})
            .as_object()
            .expect("env")
            .clone(),
        Duration::from_secs(4),
    )
    .expect_err("exit");
    assert!(!error.remote);
    assert!(error.message.contains("enable prereleases"));
    assert!(error.message.contains("<redacted>"));
    assert!(!error.message.contains("fixture-private-token"));
    assert!(error.message.contains("7"));
}

#[test]
fn initialization_and_call_deadlines_do_not_repeat_or_leave_the_server_waiting() {
    for mode in ["hang_init", "hang_call"] {
        let fixture = Fixture::new(mode);
        let started = Instant::now();
        let timeout = if mode == "hang_call" {
            Duration::from_secs(3)
        } else {
            Duration::from_millis(200)
        };
        let result = call_tool(&fixture.command, &Map::new(), "echo", Map::new(), timeout);
        assert!(result.is_err());
        assert!(started.elapsed() < timeout + Duration::from_secs(7));
        let rows = fixture.rows();
        assert_eq!(
            rows.iter()
                .filter(|row| row["method"] == "initialize")
                .count(),
            1
        );
        assert_eq!(
            rows.iter()
                .filter(|row| row["method"] == "tools/call")
                .count(),
            usize::from(mode == "hang_call")
        );
        assert!(rows.iter().any(|row| row["eof"] == true));
    }
}

#[test]
fn stderr_tail_is_bounded_and_omits_the_cut_first_line() {
    let mut tail = StderrTail::default();
    tail.append(&vec![b'x'; 16 * 1024]);
    tail.append(b"\nuseful final diagnostic\n");
    assert!(tail.bytes.len() <= super::STDERR_BYTES);
    assert_eq!(tail.text(), "useful final diagnostic\n");
}

#[test]
fn diagnostics_redact_known_values_once_and_recognize_credential_keys() {
    assert!(!super::sensitive_key("TOKENIZERS_PARALLELISM"));
    assert!(super::sensitive_key("NOTION_KEY"));
    assert!(super::sensitive_key("--api-token"));
    assert!(super::sensitive_key("--api-key"));
    assert!(super::sensitive_key("--accessKey"));
    assert!(super::sensitive_key("--privateKey"));
    assert!(super::sensitive_key("--clientSecret"));
    assert!(super::sensitive_key("authToken"));
    assert!(super::sensitive_key("GOOGLE_APPLICATION_CREDENTIALS"));
    assert_eq!(
        super::redact_diagnostic("original", &[String::new()]),
        "original"
    );
    let secrets = super::diagnostic_secrets(
        &[
            "fixture".into(),
            "https://user:password@example.test/?access=fixture-query".into(),
            "Authorization: Bearer fixture-bearer".into(),
            "--accessKey".into(),
            "fixture-access-value".into(),
            "--clientSecret=fixture-client-value".into(),
        ],
        &json!({"FIXTURE_TOKEN":"act", "OTHER_KEY":"short"})
            .as_object()
            .expect("env")
            .clone(),
    );
    assert_eq!(
        super::redact_diagnostic(
            "act password fixture-query fixture-bearer short fixture-access-value fixture-client-value; exit status: 1; false",
            &secrets,
        ),
        "<redacted> <redacted> <redacted> <redacted> <redacted> <redacted> <redacted>; exit status: 1; false"
    );
}

#[cfg(unix)]
#[test]
fn known_result_survives_cleanup_of_a_server_ignoring_eof() {
    let fixture = Fixture::new("ignore_eof");
    let result = call_tool(
        &fixture.command,
        &Map::new(),
        "echo",
        Map::new(),
        Duration::from_secs(4),
    )
    .expect("known result");
    assert_eq!(result["isError"], false);
    let pid = fixture.rows()[0]["pid"].as_i64().expect("pid") as i32;
    let missing = nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None);
    assert_eq!(
        missing,
        Err(nix::errno::Errno::ESRCH),
        "direct server must be reaped"
    );
}

#[cfg(unix)]
#[test]
fn a_path_given_to_the_server_chooses_the_program_it_runs() {
    use std::os::unix::fs::PermissionsExt;
    // Resolving the program against the daemon's own PATH would start the system python3
    // instead of the one the recipe's environment points to.
    let temp = tempfile::tempdir().expect("program dir");
    let program = temp.path().join("python3");
    std::fs::write(
        &program,
        "#!/bin/sh\necho stand-in-program-ran >&2\nexit 3\n",
    )
    .expect("stand-in program");
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).expect("mode");
    let mut env = Map::new();
    env.insert(
        "PATH".into(),
        Value::String(temp.path().to_string_lossy().into_owned()),
    );
    let error = list_tools(
        &["python3".into(), "-V".into()],
        &env,
        Duration::from_secs(10),
    )
    .expect_err("the stand-in program exits");
    assert!(
        error.message.contains("stand-in-program-ran"),
        "{}",
        error.message
    );
}
