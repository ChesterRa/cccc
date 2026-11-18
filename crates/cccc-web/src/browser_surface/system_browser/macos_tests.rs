use super::*;
use crate::browser_surface::{browser_surface_tests::local_page, chrome_test_guard};
use chromiumoxide::cdp::browser_protocol::system_info::GetProcessInfoParams;
use futures_util::StreamExt;

// Match Tokio Command::output's pipes and concurrent wait, exposing which part completes.
pub(super) async fn open_with_trace(
    command: &mut tokio::process::Command,
) -> std::io::Result<std::process::Output> {
    use std::process::{Output, Stdio};
    use tokio::io::{AsyncRead, AsyncReadExt};
    async fn collect(pipe: &mut (impl AsyncRead + Unpin), label: &str) -> std::io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        pipe.read_to_end(&mut bytes).await?;
        eprintln!("[native-browser] open {label} reached EOF");
        Ok(bytes)
    }

    eprintln!("[native-browser] spawning macOS open");
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    eprintln!("[native-browser] open started, pid {:?}", child.id());
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let wait = async {
        let status = child.wait().await?;
        eprintln!("[native-browser] open process exited with {status}");
        Ok::<_, std::io::Error>(status)
    };
    let (status, stdout, stderr) = tokio::try_join!(
        wait,
        collect(&mut stdout, "stdout"),
        collect(&mut stderr, "stderr")
    )?;
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

#[tokio::test]
async fn system_browser_answers_cdp_and_stops_its_original_process() -> Result<()> {
    eprintln!("[native-browser] test entered");
    let _guard = chrome_test_guard().await;
    eprintln!("[native-browser] test guard acquired");
    let profile = tempfile::tempdir()?;
    let mut launch = SystemBrowserLaunch::prepare(800, 600, true).await?;
    let (browser, mut handler, pid) = launch
        .launch(
            profile.path(),
            vec![
                "--headless=new".into(),
                "--disable-background-networking".into(),
            ],
        )
        .await?;
    let driver = tokio::spawn(async move { while let Some(Ok(_)) = handler.next().await {} });
    let birth = process_identity(pid);
    let recorded = launch.opened().and_then(|opened| opened.process.clone());
    let processes = tokio::time::timeout(
        Duration::from_secs(5),
        browser.execute(GetProcessInfoParams::default()),
    )
    .await;
    eprintln!("[native-browser] stopping connected Chrome");
    let stopped = launch.stop().await;
    eprintln!("[native-browser] connected Chrome stop returned");
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
    eprintln!("[native-browser] test body finished");
    Ok(())
}

#[tokio::test]
async fn failed_cdp_connection_stops_the_browser_opened_for_its_profile() -> Result<()> {
    eprintln!("[native-browser] test entered");
    let _guard = chrome_test_guard().await;
    eprintln!("[native-browser] test guard acquired");
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
    eprintln!("[native-browser] controlled HTTP endpoint is listening");
    let (result, observed) = tokio::join!(launch.launch(profile.path(), args), async {
        let pid = wait_for_browser_pid(profile.path(), Instant::now() + CDP_START_TIMEOUT).await?;
        eprintln!("[native-browser] observed profile lock for process {pid}");
        let birth = process_identity(pid)?.context("Chrome ended before the CDP failure")?;
        anyhow::Ok((pid, birth))
    });
    eprintln!("[native-browser] launch and profile observation returned");
    server.abort();

    let (pid, birth) = observed?;
    let still_running = process_identity(pid)?.as_ref() == Some(&birth);
    if still_running {
        eprintln!("[native-browser] test cleaning up its observed Chrome");
        stop_opened_browser(profile.path(), Some(&(pid, birth))).await?;
        eprintln!("[native-browser] test cleanup returned");
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
    eprintln!("[native-browser] test body finished");
    Ok(())
}
