use super::*;
use crate::browser_surface::{browser_surface_tests::local_page, chrome_test_guard};
use chromiumoxide::cdp::browser_protocol::system_info::GetProcessInfoParams;
use futures_util::StreamExt;

#[tokio::test]
async fn unresponsive_macos_launcher_is_reaped_and_reports_an_unconfirmed_browser() -> Result<()> {
    let profile = tempfile::tempdir()?;
    let mut launch = SystemBrowserLaunch {
        executable: "/bin/sh".into(),
        channel: "fixture",
        cdp_port: 0,
        background: true,
        width: 800,
        height: 600,
        display: None,
        vnc_error: String::new(),
        opened: None,
    };
    let mut command = std::process::Command::new("/bin/sh");
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
    let _guard = chrome_test_guard().await;
    let profile = tempfile::tempdir()?;
    let aliases = tempfile::tempdir()?;
    let profile_alias = aliases.path().join("browser profile");
    std::os::unix::fs::symlink(profile.path(), &profile_alias)?;
    let mut launch = SystemBrowserLaunch::prepare(800, 600, true).await?;
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
    let _guard = chrome_test_guard().await;
    let profile = tempfile::tempdir()?;
    let mut launch = SystemBrowserLaunch::prepare(800, 600, true).await?;
    // Keep the address CCCC probes under this test's control. Chrome starts on
    // its own free port, while this HTTP server never advertises a CDP socket.
    let (url, server) = local_page("No debugger here").await;
    launch.cdp_port = reqwest::Url::parse(&url)?.port().context("fixture port")?;
    let args = vec![
        "--headless=new".into(),
        "--disable-background-networking".into(),
        "--remote-debugging-port=0".into(),
    ];
    let (result, observed) = tokio::join!(launch.launch(profile.path(), args), async {
        let pid = wait_for_browser_pid(profile.path(), Instant::now() + CDP_START_TIMEOUT).await?;
        let birth = process_identity(pid)?.context("Chrome ended before the CDP failure")?;
        anyhow::Ok((pid, birth))
    });
    server.abort();

    let (pid, birth) = observed?;
    let still_running = process_identity(pid)?.as_ref() == Some(&birth);
    if still_running {
        stop_opened_browser(profile.path(), Some(&(pid, birth))).await?;
    }
    let error = result
        .err()
        .context("expected the CDP connection to fail")?;
    assert!(
        error
            .to_string()
            .contains("CDP endpoint did not become ready"),
        "{error:#}"
    );
    assert!(
        !still_running,
        "the failed launch left its Chrome process running"
    );
    assert!(
        launch.opened().is_none(),
        "the confirmed stop must clear the launch record"
    );
    Ok(())
}
