// Included by the crate-level integration test harness.
use fs2::FileExt;
use std::fs::OpenOptions;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

// These black-box cases also run unchanged against the stock daemon as the
// missing-feature reference. Only the CLI's public IPC interface is used.
#[test]
fn host_disk_diagnostic_is_sampled_without_any_group() {
    with_disk_daemon(|home| {
        std::thread::sleep(Duration::from_millis(100));
        let diagnostic = wait_disk_sample(home);
        assert!(diagnostic["space"]["total_bytes"].as_u64().is_some());
        assert!(diagnostic["space"]["available_bytes"].as_u64().is_some());
        assert!(diagnostic["sampled_at"].as_str().is_some());
        assert_eq!(diagnostic["sample_interval_seconds"], 60);
        assert!(
            std::fs::read_dir(home.join("groups"))
                .expect("group directory")
                .next()
                .is_none(),
            "the host diagnostic must not require a Group"
        );
    });
}

#[test]
fn host_disk_diagnostic_status_polling_does_not_resample() {
    with_disk_daemon(|home| {
        let initial = wait_disk_sample(home);
        for _ in 0..8 {
            let current = disk_ping(home);
            assert_eq!(current["attempts"], initial["attempts"]);
            assert_eq!(current["sampled_at"], initial["sampled_at"]);
            assert_eq!(current["space"], initial["space"]);
        }
    });
}

fn with_disk_daemon(check: impl FnOnce(&Path)) {
    let temp = tempfile::tempdir().expect("home");
    let home = temp.path().join("home");
    let executable = Path::new(env!("CARGO_BIN_EXE_cccc"));
    let start = run(executable, &home, &["daemon", "start"]);
    assert!(start.status.success(), "start: {}", detail(&start));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check(&home)));
    let stop = run(executable, &home, &["daemon", "stop"]);
    wait_for_daemon_exit(&home);
    assert!(stop.status.success(), "stop: {}", detail(&stop));
    assert_daemon_lock_released(&home);
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}

fn disk_ping(home: &Path) -> serde_json::Value {
    let layout = cccc_core::HomeLayout::from_path(home).expect("home layout");
    let response = tokio::runtime::Runtime::new()
        .expect("RPC runtime")
        .block_on(
            cccc_client::DaemonClient::new(layout).call(&cccc_contracts::DaemonRequest {
                v: 1,
                op: "ping".into(),
                args: Default::default(),
            }),
        )
        .expect("actual daemon ping");
    assert!(response.ok, "ping succeeds");
    response.result["diagnostics"]["disk"].clone()
}

fn wait_disk_sample(home: &Path) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let value = disk_ping(home);
        if value["attempts"].as_u64().is_some_and(|count| count > 0) {
            return value;
        }
        assert!(Instant::now() < deadline, "host disk diagnostic is missing");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn installed_cli_starts_daemon_without_a_sibling_daemon_binary() {
    let temp = tempfile::tempdir().expect("tempdir");
    let bin_dir = temp.path().join("bin");
    std::fs::create_dir(&bin_dir).expect("create bin dir");

    let installed = bin_dir.join(executable_name("cccc"));
    std::fs::copy(env!("CARGO_BIN_EXE_cccc"), &installed).expect("copy cccc");

    // A decoy catches regressions to the old sibling `ccccd start` contract.
    let decoy = bin_dir.join(executable_name("ccccd"));
    std::fs::copy(env!("CARGO_BIN_EXE_cccc"), decoy).expect("copy decoy ccccd");

    let home = temp.path().join("home");
    let start = run(&installed, &home, &["daemon", "start"]);
    if !start.status.success() {
        panic!("daemon start failed: {}", detail(&start));
    }

    let status = run(&installed, &home, &["daemon", "status"]);
    let stop = run(&installed, &home, &["daemon", "stop"]);
    wait_for_daemon_exit(&home);

    assert!(
        status.status.success(),
        "daemon status failed: {}",
        detail(&status)
    );
    assert!(
        stop.status.success(),
        "daemon stop failed: {}",
        detail(&stop)
    );
    assert_daemon_lock_released(&home);
}

#[test]
fn daemon_stop_waits_for_the_combined_web_process_to_exit() {
    let temp = tempfile::tempdir().expect("tempdir");
    let installed = temp.path().join(executable_name("cccc"));
    std::fs::copy(env!("CARGO_BIN_EXE_cccc"), &installed).expect("copy cccc");
    let home = temp.path().join("home");
    let mut web = Command::new(&installed)
        .args(["--host", "127.0.0.1", "--port", "0"])
        .env("CCCC_HOME", &home)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start combined CCCC");

    wait_for_combined_web_start(&home, &mut web);
    let stop = run(&installed, &home, &["daemon", "stop"]);
    if !stop.status.success() {
        // Recover the combined process output so CI failures show which
        // shutdown stage hung instead of only the CLI timeout.
        let _ = web.kill();
        let output = web
            .wait_with_output()
            .expect("collect combined CCCC output");
        panic!(
            "daemon stop failed: {} combined CCCC output: {}",
            detail(&stop),
            detail(&output)
        );
    }
    assert!(
        web.try_wait().expect("combined CCCC status").is_some(),
        "daemon stop returned before the combined Web process exited"
    );
    let output = web
        .wait_with_output()
        .expect("collect combined CCCC output");
    assert!(
        output.status.success(),
        "combined CCCC failed: {}",
        detail(&output)
    );
    assert_daemon_lock_released(&home);
    assert_web_lock_released(&home);
}

#[test]
fn combined_web_bind_failure_stops_its_owned_daemon() {
    let temp = tempfile::tempdir().expect("tempdir");
    let installed = temp.path().join(executable_name("cccc"));
    std::fs::copy(env!("CARGO_BIN_EXE_cccc"), &installed).expect("copy cccc");
    let home = temp.path().join("home");
    let occupied = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("occupy Web port");
    let port = occupied.local_addr().expect("occupied address").port();

    let output = run(
        &installed,
        &home,
        &["--host", "127.0.0.1", "--port", &port.to_string()],
    );

    assert!(
        !output.status.success(),
        "combined CCCC unexpectedly served an occupied port: {}",
        detail(&output)
    );
    wait_for_daemon_exit(&home);
    assert_daemon_lock_released(&home);
    assert_web_lock_released(&home);
}

fn assert_daemon_lock_released(home: &Path) {
    let lock_path = home.join("daemon").join("ccccd.lock");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .expect("open daemon lock");
    lock.try_lock_exclusive().unwrap_or_else(|error| {
        panic!(
            "daemon stop returned before releasing {}: {error}",
            lock_path.display()
        )
    });
    FileExt::unlock(&lock).expect("release daemon lock probe");
}

fn assert_web_lock_released(home: &Path) {
    let lock_path = home.join("daemon").join("cccc-web.lock");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .expect("open Web lock");
    lock.try_lock_exclusive().unwrap_or_else(|error| {
        panic!(
            "daemon stop returned before releasing {}: {error}",
            lock_path.display()
        )
    });
    FileExt::unlock(&lock).expect("release Web lock probe");
}

fn executable_name(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_owned()
    }
}

fn run(executable: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(executable)
        .args(args)
        .env("CCCC_HOME", home)
        .output()
        .expect("run installed cccc")
}

fn wait_for_daemon_exit(home: &Path) {
    let address = home.join("daemon").join("ccccd.addr.json");
    let deadline = Instant::now() + Duration::from_secs(10);
    while address.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        !address.exists(),
        "daemon did not remove {}",
        address.display()
    );
}

fn wait_for_combined_web_start(home: &Path, web: &mut std::process::Child) {
    let runtime_path = home.join("daemon").join("web_runtime.json");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(web_status) = web.try_wait().expect("combined CCCC status") {
            panic!("combined CCCC exited before Web startup: {web_status}");
        }
        // Daemon IPC becomes available before its combined Web owner has
        // finished startup. Test shutdown of a listening Web, not that race.
        if let Ok(bytes) = std::fs::read(&runtime_path)
            && let Ok(runtime) = serde_json::from_slice::<serde_json::Value>(&bytes)
            && runtime["pid"].as_u64() == Some(u64::from(web.id()))
            && let Some(port) = runtime["port"]
                .as_u64()
                .and_then(|port| u16::try_from(port).ok())
            && std::net::TcpStream::connect_timeout(
                &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
                Duration::from_millis(100),
            )
            .is_ok()
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "combined CCCC Web did not start within 30 seconds"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn detail(output: &Output) -> String {
    format!(
        "status={} stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout).trim(),
        String::from_utf8_lossy(&output.stderr).trim()
    )
}
