use anyhow::{Context, Result, bail};
#[cfg(not(target_os = "macos"))]
use chromiumoxide::BrowserConfig;
#[cfg(not(target_os = "macos"))]
use chromiumoxide::browser::BrowserConfigBuilder;
use chromiumoxide::{Browser, Handler};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
use super::profile_owner::{browser_pid_from_singleton, terminate_browser_for_profile};

#[cfg(target_os = "macos")]
const CDP_START_TIMEOUT: Duration = Duration::from_secs(20);

pub(super) struct SystemBrowserLaunch {
    executable: PathBuf,
    channel: &'static str,
    cdp_port: u16,
    background: bool,
    width: u32,
    height: u32,
    display: Option<VirtualDisplay>,
    #[cfg(target_os = "macos")]
    managed_profile: Option<PathBuf>,
}

impl SystemBrowserLaunch {
    pub(super) async fn prepare(width: u32, height: u32, background: bool) -> Result<Self> {
        let (executable, channel) = find_system_browser().ok_or_else(|| {
            anyhow::anyhow!(
                "Chrome, Microsoft Edge, or Chromium is required for projected browser authentication"
            )
        })?;
        let cdp_port = reserve_cdp_port()?;
        let display = VirtualDisplay::start(width, height).await?;
        Ok(Self {
            executable,
            channel,
            cdp_port,
            background,
            width,
            height,
            display,
            #[cfg(target_os = "macos")]
            managed_profile: None,
        })
    }

    #[cfg(not(target_os = "macos"))]
    pub(super) fn configure(&self, mut config: BrowserConfigBuilder) -> BrowserConfigBuilder {
        config = config
            .disable_default_args()
            .chrome_executable(&self.executable)
            .port(self.cdp_port)
            .args(["--no-first-run", "--no-default-browser-check"])
            .window_size(self.width, self.height)
            .arg(window_position(self.background))
            .arg("--force-device-scale-factor=1");
        config = config.with_head();
        if let Some(display) = &self.display {
            config = config
                .env("DISPLAY", display.name())
                .arg("--ozone-platform=x11");
        }
        config
    }

    pub(super) async fn launch(
        &mut self,
        profile: &Path,
        extra_args: Vec<String>,
    ) -> Result<(Browser, Handler, u32)> {
        #[cfg(target_os = "macos")]
        {
            return self.launch_background_macos(profile, extra_args).await;
        }
        #[cfg(not(target_os = "macos"))]
        {
            let mut config = self.configure(BrowserConfig::builder().user_data_dir(profile));
            if !extra_args.is_empty() {
                config = config.args(extra_args);
            }
            let (mut browser, handler) =
                Browser::launch(config.build().map_err(anyhow::Error::msg)?).await?;
            let pid = browser
                .get_mut_child()
                .and_then(|child| child.as_mut_inner().id())
                .context("launched Chromium process has no PID")?;
            Ok((browser, handler, pid))
        }
    }

    #[cfg(target_os = "macos")]
    async fn launch_background_macos(
        &mut self,
        profile: &Path,
        extra_args: Vec<String>,
    ) -> Result<(Browser, Handler, u32)> {
        let app =
            macos_app_bundle(&self.executable).context("system browser app bundle not found")?;
        let browser_args = self.browser_args(profile, extra_args);
        let mut command = tokio::process::Command::new("/usr/bin/open");
        command.args(macos_open_args(app));
        command.args(&browser_args);
        let output = command
            .output()
            .await
            .context("launch system browser in background")?;
        if !output.status.success() {
            bail!(
                "background system browser launch failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        match self.connect_background_macos(profile).await {
            Ok(connected) => {
                self.managed_profile = Some(profile.to_owned());
                Ok(connected)
            }
            Err(error) => {
                if let Err(cleanup_error) = terminate_browser_for_profile(profile).await {
                    tracing::warn!(%cleanup_error, "failed to clean up background system browser launch");
                }
                Err(error)
            }
        }
    }

    #[cfg(target_os = "macos")]
    async fn connect_background_macos(&self, profile: &Path) -> Result<(Browser, Handler, u32)> {
        let endpoint = format!("http://127.0.0.1:{}/json/version", self.cdp_port);
        let client = reqwest::Client::builder().no_proxy().build()?;
        let deadline = Instant::now() + CDP_START_TIMEOUT;
        loop {
            if let Some(websocket_url) = cdp_websocket_url(&client, &endpoint).await {
                match Browser::connect(websocket_url).await {
                    Ok((browser, handler)) => {
                        let pid = wait_for_browser_pid(profile, deadline).await?;
                        return Ok((browser, handler, pid));
                    }
                    Err(error) if Instant::now() < deadline => {
                        tracing::debug!(%error, "system browser CDP socket is not ready");
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            if Instant::now() >= deadline {
                bail!("system browser CDP endpoint did not become ready");
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    #[cfg(target_os = "macos")]
    fn browser_args(&self, profile: &Path, extra_args: Vec<String>) -> Vec<String> {
        let mut args = vec![
            format!("--remote-debugging-port={}", self.cdp_port),
            format!("--user-data-dir={}", profile.display()),
            "--no-first-run".to_owned(),
            "--no-default-browser-check".to_owned(),
            "--disable-extensions".to_owned(),
            format!("--window-size={},{}", self.width, self.height),
            window_position(self.background).to_owned(),
            "--force-device-scale-factor=1".to_owned(),
        ];
        args.extend(extra_args);
        args
    }

    pub(super) fn strategy(&self) -> String {
        let suffix = if self.display.is_some() { "_xvfb" } else { "" };
        format!("system_browser_cdp:{}{suffix}", self.channel)
    }

    pub(super) fn metadata(&self, pid: u32, profile: &Path) -> Value {
        json!({
            "pid":pid,
            "cdp_port":self.cdp_port,
            "browser_binary":self.executable,
            "channel":self.channel,
            "profile_dir":profile,
            "visibility":if self.background||self.display.is_some(){"background"}else{"visible"},
            "display":self.display.as_ref().map_or("", VirtualDisplay::name),
            "display_owned":self.display.is_some(),
            "display_owner":self.display.as_ref().map_or("", |_| "cccc_xvfb")
        })
    }

    pub(super) async fn stop(&mut self) {
        #[cfg(target_os = "macos")]
        if let Some(profile) = self.managed_profile.take()
            && let Err(error) = terminate_browser_for_profile(&profile).await
        {
            tracing::warn!(%error, "failed to stop managed system browser process");
        }
        if let Some(display) = &mut self.display {
            display.stop().await;
        }
    }
}

#[cfg(target_os = "macos")]
fn macos_open_args(app: &Path) -> Vec<std::ffi::OsString> {
    // `-g` launches without activating the application; `-n` keeps CCCC's
    // dedicated profile isolated from an already-running personal Chrome.
    ["-g", "-n", "-a"]
        .into_iter()
        .map(std::ffi::OsString::from)
        .chain(std::iter::once(app.as_os_str().to_owned()))
        .chain(std::iter::once(std::ffi::OsString::from("--args")))
        .collect()
}

#[cfg(target_os = "macos")]
fn macos_app_bundle(executable: &Path) -> Option<&Path> {
    executable
        .ancestors()
        .find(|path| path.extension().is_some_and(|extension| extension == "app"))
}

#[cfg(target_os = "macos")]
async fn cdp_websocket_url(client: &reqwest::Client, endpoint: &str) -> Option<String> {
    client
        .get(endpoint)
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json::<Value>()
        .await
        .ok()?
        .get("webSocketDebuggerUrl")?
        .as_str()
        .map(str::to_owned)
}

#[cfg(target_os = "macos")]
async fn wait_for_browser_pid(profile: &Path, deadline: Instant) -> Result<u32> {
    loop {
        if let Ok(pid) = browser_pid_from_singleton(profile) {
            return Ok(pid);
        }
        if Instant::now() >= deadline {
            bail!("system browser profile owner PID did not become ready");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn reserve_cdp_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
    Ok(listener.local_addr()?.port())
}

fn window_position(background: bool) -> &'static str {
    if background {
        "--window-position=-32000,-32000"
    } else {
        "--window-position=0,0"
    }
}

fn find_system_browser() -> Option<(PathBuf, &'static str)> {
    fixed_browser_candidates()
        .into_iter()
        .find(|(path, _)| path.is_file())
        .or_else(|| find_on_path(path_browser_candidates()))
}

fn find_on_path(candidates: &[(&str, &'static str)]) -> Option<(PathBuf, &'static str)> {
    let path = std::env::var_os("PATH")?;
    let directories = std::env::split_paths(&path).collect::<Vec<_>>();
    candidates.iter().find_map(|(name, channel)| {
        directories.iter().find_map(|directory| {
            let candidate = directory.join(name);
            candidate.is_file().then_some((candidate, *channel))
        })
    })
}

fn path_browser_candidates() -> &'static [(&'static str, &'static str)] {
    if cfg!(target_os = "windows") {
        &[
            ("chrome.exe", "chrome"),
            ("msedge.exe", "msedge"),
            ("chromium.exe", "chromium"),
        ]
    } else {
        &[
            ("google-chrome", "chrome"),
            ("google-chrome-stable", "chrome"),
            ("microsoft-edge", "msedge"),
            ("microsoft-edge-stable", "msedge"),
            ("chromium", "chromium"),
            ("chromium-browser", "chromium"),
        ]
    }
}

fn fixed_browser_candidates() -> Vec<(PathBuf, &'static str)> {
    if cfg!(target_os = "macos") {
        return vec![
            (
                PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
                "chrome",
            ),
            (
                PathBuf::from("/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"),
                "msedge",
            ),
            (
                PathBuf::from("/Applications/Chromium.app/Contents/MacOS/Chromium"),
                "chromium",
            ),
        ];
    }
    if cfg!(target_os = "windows") {
        return ["ProgramFiles", "ProgramFiles(x86)"]
            .into_iter()
            .filter_map(std::env::var_os)
            .flat_map(|root| {
                let root = PathBuf::from(root);
                [
                    (root.join("Google/Chrome/Application/chrome.exe"), "chrome"),
                    (root.join("Microsoft/Edge/Application/msedge.exe"), "msedge"),
                    (root.join("Chromium/Application/chrome.exe"), "chromium"),
                ]
            })
            .collect();
    }
    Vec::new()
}

struct VirtualDisplay {
    #[cfg(target_os = "linux")]
    child: tokio::process::Child,
    name: String,
}

impl VirtualDisplay {
    #[cfg(not(target_os = "linux"))]
    async fn start(_width: u32, _height: u32) -> Result<Option<Self>> {
        Ok(None)
    }

    #[cfg(target_os = "linux")]
    async fn start(width: u32, height: u32) -> Result<Option<Self>> {
        use tokio::io::{AsyncBufReadExt, BufReader};
        use tokio::process::Command;

        let Some(binary) = find_executable("Xvfb") else {
            bail!(
                "Xvfb is required for projected browser authentication on Linux; install the xvfb package and retry"
            );
        };
        let mut child = Command::new(binary)
            .args(xvfb_args(width, height))
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .context("start Xvfb for projected browser authentication")?;
        let stdout = child
            .stdout
            .take()
            .context("Xvfb did not expose a display descriptor")?;
        let mut line = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            BufReader::new(stdout).read_line(&mut line),
        )
        .await
        .context("Xvfb display startup timed out")??;
        if child.try_wait()?.is_some() {
            bail!("Xvfb exited before a display became ready");
        }
        let number = line.trim().trim_start_matches(':');
        if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
            bail!("Xvfb did not report a usable display");
        }
        Ok(Some(Self {
            child,
            name: format!(":{number}"),
        }))
    }

    fn name(&self) -> &str {
        &self.name
    }

    async fn stop(&mut self) {
        #[cfg(target_os = "linux")]
        {
            if self.child.try_wait().ok().flatten().is_some() {
                return;
            }
            let _ = self.child.start_kill();
            let _ =
                tokio::time::timeout(std::time::Duration::from_secs(3), self.child.wait()).await;
        }
    }
}

#[cfg(any(target_os = "linux", test))]
fn xvfb_args(width: u32, height: u32) -> Vec<String> {
    vec![
        "-displayfd".into(),
        "1".into(),
        "-screen".into(),
        "0".into(),
        format!("{}x{}x24", width.max(1024), height.max(768)),
        "-nolisten".into(),
        "tcp".into(),
    ]
}

#[cfg(target_os = "linux")]
fn find_executable(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xvfb_keeps_local_unix_transport_enabled() {
        let args = xvfb_args(800, 600);
        assert!(args.windows(2).any(|pair| pair == ["-nolisten", "tcp"]));
        assert!(!args.iter().any(|arg| arg == "unix"));
        assert!(args.iter().any(|arg| arg == "1024x768x24"));
    }

    #[test]
    fn system_browser_candidates_prefer_chrome_then_edge_then_chromium() {
        let candidates = path_browser_candidates();
        let chrome = candidates
            .iter()
            .position(|(_, channel)| *channel == "chrome")
            .expect("chrome candidate");
        let edge = candidates
            .iter()
            .position(|(_, channel)| *channel == "msedge")
            .expect("edge candidate");
        let chromium = candidates
            .iter()
            .position(|(_, channel)| *channel == "chromium")
            .expect("chromium candidate");

        assert!(chrome < edge);
        assert!(edge < chromium);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn fixed_browser_candidates_keep_chromium_compatibility() {
        assert!(
            fixed_browser_candidates()
                .iter()
                .any(|(path, channel)| *channel == "chromium"
                    && path.ends_with("Chromium.app/Contents/MacOS/Chromium"))
        );
    }

    #[test]
    fn system_browser_state_describes_visible_persistent_profile() {
        let launch = SystemBrowserLaunch {
            executable: PathBuf::from("/Applications/Google Chrome"),
            channel: "chrome",
            cdp_port: 9222,
            background: false,
            width: 1366,
            height: 900,
            display: None,
            #[cfg(target_os = "macos")]
            managed_profile: None,
        };
        let profile = Path::new("/tmp/cccc-web-model-profile");

        assert_eq!(launch.strategy(), "system_browser_cdp:chrome");
        let metadata = launch.metadata(42, profile);
        assert_eq!(metadata["pid"], 42);
        assert_eq!(metadata["cdp_port"], 9222);
        assert_eq!(metadata["channel"], "chrome");
        assert_eq!(metadata["visibility"], "visible");
        assert_eq!(metadata["display_owned"], false);
        assert_eq!(metadata["profile_dir"], profile.to_string_lossy().as_ref());
    }

    #[test]
    fn reserves_a_nonzero_loopback_cdp_port() {
        assert_ne!(reserve_cdp_port().expect("CDP port"), 0);
    }

    #[test]
    fn background_browser_stays_outside_the_host_desktop() {
        assert_eq!(window_position(true), "--window-position=-32000,-32000");
        assert_eq!(window_position(false), "--window-position=0,0");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_launches_a_new_browser_instance_without_activating_it() {
        let args = macos_open_args(Path::new("/Applications/Google Chrome.app"));

        assert_eq!(
            args,
            [
                "-g",
                "-n",
                "-a",
                "/Applications/Google Chrome.app",
                "--args"
            ]
            .into_iter()
            .map(std::ffi::OsString::from)
            .collect::<Vec<_>>()
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_browser_keeps_requested_size_without_maximizing() {
        let launch = SystemBrowserLaunch {
            executable: PathBuf::from(
                "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            ),
            channel: "chrome",
            cdp_port: 9222,
            background: false,
            width: 1366,
            height: 900,
            display: None,
            managed_profile: None,
        };
        let args = launch.browser_args(Path::new("/tmp/profile"), Vec::new());

        assert!(args.iter().any(|arg| arg == "--window-size=1366,900"));
        assert!(args.iter().any(|arg| arg == "--window-position=0,0"));
        assert!(!args.iter().any(|arg| arg.contains("maximiz")));
        assert_eq!(
            macos_app_bundle(&launch.executable),
            Some(Path::new("/Applications/Google Chrome.app"))
        );
    }
}
