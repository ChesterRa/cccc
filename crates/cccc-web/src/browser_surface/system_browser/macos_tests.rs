use super::*;
use crate::browser_surface::{browser_surface_tests::local_page, chrome_test_guard};
use chromiumoxide::cdp::browser_protocol::system_info::GetProcessInfoParams;
use futures_util::StreamExt;

/// The launcher records subprocess ownership in a process-global guard. Other
/// web tests run daemons whose temporary guard homes end before these launches.
async fn run_in_fresh_process(name: &str) -> Result<bool> {
    const CHILD: &str = "CCCC_TEST_MACOS_BROWSER_LAUNCH_CHILD";
    if std::env::var(CHILD).as_deref() == Ok(name) {
        return Ok(false);
    }
    let test = format!("browser_surface::system_browser::macos_tests::{name}");
    let output = cccc_runtime::background_tokio_command(std::env::current_exe()?)
        .args(["--exact", &test, "--nocapture"])
        .env(CHILD, name)
        .kill_on_drop(true)
        .output()
        .await?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    anyhow::ensure!(
        output.status.success()
            && stdout
                .lines()
                .any(|line| line.starts_with("test result: ok. 1 passed;")),
        "isolated launcher test {name} failed or did not run:\n{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(true)
}

#[tokio::test]
async fn unresponsive_macos_launcher_is_reaped_and_reports_an_unconfirmed_browser() -> Result<()> {
    if run_in_fresh_process(
        "unresponsive_macos_launcher_is_reaped_and_reports_an_unconfirmed_browser",
    )
    .await?
    {
        return Ok(());
    }
    let profile = tempfile::tempdir()?;
    let mut launch = SystemBrowserLaunch {
        executable: "/bin/sh".into(),
        channel: "fixture".to_owned(),
        cdp_port: 0,
        background: true,
        width: 800,
        height: 600,
        display: None,
        vnc_error: String::new(),
        opened: None,
    };
    let mut command = cccc_runtime::background_command("/bin/sh");
    command
        .current_dir(profile.path())
        .args(["-c", "echo $$ > launcher.pid && exec /bin/sleep 120"]);
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        launch.launch_with_macos_open(profile.path(), &mut command, Duration::from_millis(500)),
    )
    .await?;
    let error = result.err().context("expected a launcher timeout")?;
    assert!(
        error.to_string().contains("macOS did not respond"),
        "{error:#}"
    );
    let pid = std::fs::read_to_string(profile.path().join("launcher.pid"))?
        .trim()
        .parse()?;
    assert!(
        process_identity(pid)?.is_none(),
        "launcher is still running"
    );
    let opened = launch
        .opened()
        .context("launch outcome must remain visible")?;
    assert_eq!(opened.profile, profile.path());
    assert!(opened.process.is_none());
    Ok(())
}

#[tokio::test]
async fn system_browser_answers_cdp_and_stops_its_original_process() -> Result<()> {
    let Some(_guard) = chrome_test_guard().await else {
        return Ok(());
    };
    if run_in_fresh_process("system_browser_answers_cdp_and_stops_its_original_process").await? {
        return Ok(());
    }
    let profile = tempfile::tempdir()?;
    let aliases = tempfile::tempdir()?;
    let profile_alias = aliases.path().join("browser profile");
    std::os::unix::fs::symlink(profile.path(), &profile_alias)?;
    let mut launch = SystemBrowserLaunch::prepare(
        800,
        600,
        true,
        programs(BrowserMode::System { background: true }),
    )
    .await?;
    let (browser, mut handler, pid) = launch
        .launch(
            &profile_alias,
            vec![
                "--headless=new".into(),
                "--disable-background-networking".into(),
            ],
        )
        .await?;
    let driver = tokio::spawn(async move { while let Some(Ok(_)) = handler.next().await {} });
    let birth = process_identity(pid);
    let recorded = launch.opened().and_then(|opened| opened.process.clone());
    let recorded_profile = launch.opened().map(|opened| opened.profile.clone());
    let processes = tokio::time::timeout(
        Duration::from_secs(5),
        browser.execute(GetProcessInfoParams::default()),
    )
    .await;
    let stopped = launch.stop().await;
    driver.abort();
    stopped?;
    assert!(
        processes??
            .result
            .process_info
            .iter()
            .any(|p| p.r#type == "browser" && p.id == i64::from(pid)),
        "CDP must report the browser PID from the private profile"
    );
    let birth = birth?.context("the browser needs a native process identity")?;
    assert_ne!(process_identity(pid)?.as_ref(), Some(&birth));
    assert_eq!(recorded, Some((pid, birth)));
    assert_eq!(recorded_profile, Some(profile.path().canonicalize()?));
    Ok(())
}

#[tokio::test]
async fn failed_cdp_connection_stops_the_browser_opened_for_its_profile() -> Result<()> {
    let Some(_guard) = chrome_test_guard().await else {
        return Ok(());
    };
    if run_in_fresh_process("failed_cdp_connection_stops_the_browser_opened_for_its_profile")
        .await?
    {
        return Ok(());
    }
    let temp = tempfile::tempdir()?;
    let profile = temp.path().canonicalize()?;
    let mut launch = SystemBrowserLaunch::prepare(
        800,
        600,
        true,
        programs(BrowserMode::System { background: true }),
    )
    .await?;
    // Start a real browser before inducing the connection failure. This
    // exercises cleanup even on a host where LaunchServices cannot open apps.
    launch.cdp_port = 0;
    let args = launch.browser_args(
        &profile,
        vec![
            "--headless=new".into(),
            "--disable-background-networking".into(),
        ],
    );
    let mut chrome = cccc_runtime::background_tokio_command(&launch.executable)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let pid = chrome.id().context("missing fixture browser PID")?;
    assert_eq!(
        wait_for_browser_pid(&profile, Instant::now() + CDP_START_TIMEOUT).await?,
        pid
    );
    let birth = process_identity(pid)?.context("Chrome ended before the CDP failure")?;
    let (url, server) = local_page("No debugger here").await;
    launch.cdp_port = reqwest::Url::parse(&url)?.port().context("fixture port")?;
    // The launcher has completed, but CCCC has not yet recorded the PID.
    // Its normal failure path must identify and stop Chrome from the profile.
    let mut completed_launcher = cccc_runtime::background_command("/usr/bin/true");
    let (result, exit) = tokio::join!(
        launch.launch_with_macos_open(&profile, &mut completed_launcher, CDP_START_TIMEOUT),
        tokio::time::timeout(CDP_START_TIMEOUT + Duration::from_secs(10), chrome.wait()),
    );
    server.abort();
    let error = result
        .err()
        .context("expected the CDP connection to fail")?;
    assert!(
        error
            .to_string()
            .contains("CDP endpoint did not become ready"),
        "{error:#}"
    );
    exit.context("the failed connection left its Chrome process running")??;
    assert_ne!(process_identity(pid)?.as_ref(), Some(&birth));
    assert!(
        launch.opened().is_none(),
        "the confirmed stop must clear the launch record"
    );
    Ok(())
}
