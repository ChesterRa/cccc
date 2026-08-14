#[cfg(any(target_os = "linux", target_os = "macos"))]
use anyhow::bail;
use anyhow::{Context, Result};
#[cfg(not(target_os = "macos"))]
use chromiumoxide::BrowserConfig;
#[cfg(not(target_os = "macos"))]
use chromiumoxide::browser::BrowserConfigBuilder;
use chromiumoxide::{Browser, Handler};
use serde_json::{Value, json};
#[cfg(target_os = "linux")]
use std::fs::File;
#[cfg(target_os = "linux")]
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
const XVFB_FIRST_DISPLAY: u16 = 99;
#[cfg(target_os = "linux")]
const XVFB_LAST_DISPLAY: u16 = 199;
#[cfg(target_os = "linux")]
const XVFB_START_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
#[cfg(target_os = "linux")]
const VNC_START_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

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
    #[cfg(target_os = "linux")]
    vnc: Option<ProjectedVncServer>,
    vnc_error: String,
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
        let cdp_port = initial_cdp_port()?;
        let display = VirtualDisplay::start(width, height).await?;
        #[cfg(target_os = "linux")]
        let (vnc, vnc_error) = match &display {
            Some(display) => ProjectedVncServer::start(display.name()).await,
            None => (None, "missing_display".to_owned()),
        };
        #[cfg(not(target_os = "linux"))]
        let vnc_error = "unsupported_platform".to_owned();
        Ok(Self {
            executable,
            channel,
            cdp_port,
            background,
            width,
            height,
            display,
            #[cfg(target_os = "linux")]
            vnc,
            vnc_error,
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
            .viewport(None)
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
            #[cfg(target_os = "linux")]
            let vnc_port = self.vnc.as_ref().map(|vnc| vnc.port);
            #[cfg(not(target_os = "linux"))]
            let vnc_port: Option<u16> = None;
            let (mut browser, handler) =
                Browser::launch(config.build().map_err(anyhow::Error::msg)?)
                    .await
                    .with_context(|| {
                        format!(
                            "launch projected browser with cdp_port={} vnc_port={vnc_port:?}",
                            self.cdp_port
                        )
                    })?;
            self.cdp_port = browser_assigned_cdp_port(browser.websocket_address())
                .context("projected browser did not report a usable CDP port")?;
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

    pub(super) fn viewer(&self) -> Value {
        #[cfg(target_os = "linux")]
        if let Some(vnc) = &self.vnc {
            return json!({
                "kind":"vnc",
                "vnc":{
                    "available":true,
                    "display":vnc.display,
                    "port":vnc.port,
                    "pid":vnc.pid(),
                    "started_at":vnc.started_at
                }
            });
        }
        json!({
            "kind":"screencast",
            "vnc":{"available":false,"error":self.vnc_error}
        })
    }

    pub(super) fn vnc_port(&self) -> Option<u16> {
        #[cfg(target_os = "linux")]
        {
            self.vnc.as_ref().map(|vnc| vnc.port)
        }
        #[cfg(not(target_os = "linux"))]
        None
    }

    pub(super) async fn stop(&mut self) {
        #[cfg(target_os = "macos")]
        if let Some(profile) = self.managed_profile.take()
            && let Err(error) = terminate_browser_for_profile(&profile).await
        {
            tracing::warn!(%error, "failed to stop managed system browser process");
        }
        #[cfg(target_os = "linux")]
        if let Some(vnc) = &mut self.vnc {
            vnc.stop().await;
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

#[cfg(any(target_os = "linux", target_os = "macos", test))]
fn reserve_cdp_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
    Ok(listener.local_addr()?.port())
}

#[cfg(target_os = "macos")]
fn initial_cdp_port() -> Result<u16> {
    reserve_cdp_port()
}

#[cfg(not(target_os = "macos"))]
fn initial_cdp_port() -> Result<u16> {
    Ok(0)
}

#[cfg(not(target_os = "macos"))]
fn browser_assigned_cdp_port(websocket_address: &str) -> Option<u16> {
    let authority = websocket_address.strip_prefix("ws://")?.split('/').next()?;
    let port = authority.parse::<std::net::SocketAddr>().ok()?.port();
    (port != 0).then_some(port)
}

fn window_position(background: bool) -> &'static str {
    if background {
        "--window-position=-32000,-32000"
    } else {
        "--window-position=0,0"
    }
}

pub(super) fn find_system_browser() -> Option<(PathBuf, &'static str)> {
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
        return windows_fixed_browser_candidates(
            ["ProgramFiles", "ProgramFiles(x86)"]
                .into_iter()
                .filter_map(std::env::var_os)
                .map(PathBuf::from),
        );
    }
    Vec::new()
}

fn windows_fixed_browser_candidates(
    roots: impl IntoIterator<Item = PathBuf>,
) -> Vec<(PathBuf, &'static str)> {
    roots
        .into_iter()
        .flat_map(|root| {
            [
                (root.join("Google/Chrome/Application/chrome.exe"), "chrome"),
                (root.join("Microsoft/Edge/Application/msedge.exe"), "msedge"),
                (root.join("Chromium/Application/chrome.exe"), "chromium"),
            ]
        })
        .collect()
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
        let Some(binary) = find_executable("Xvfb") else {
            bail!(
                "Xvfb is required for projected browser authentication on Linux; install the xvfb package and retry"
            );
        };

        for number in XVFB_FIRST_DISPLAY..=XVFB_LAST_DISPLAY {
            if xvfb_display_in_use(number) {
                continue;
            }
            if let Some(display) = start_xvfb_candidate(&binary, number, width, height).await? {
                return Ok(Some(display));
            }
        }
        bail!("no free Xvfb display is available in :{XVFB_FIRST_DISPLAY}-:{XVFB_LAST_DISPLAY}")
    }

    fn name(&self) -> &str {
        &self.name
    }

    async fn stop(&mut self) {
        #[cfg(target_os = "linux")]
        stop_process_child(&mut self.child).await;
    }
}

#[cfg(target_os = "linux")]
struct ProjectedVncServer {
    child: tokio::process::Child,
    display: String,
    port: u16,
    started_at: String,
}

#[cfg(target_os = "linux")]
impl ProjectedVncServer {
    async fn start(display: &str) -> (Option<Self>, String) {
        if !vnc_viewer_enabled() {
            return (None, "disabled".to_owned());
        }
        let Some(binary) = find_executable("x11vnc") else {
            return (None, "x11vnc_not_found".to_owned());
        };
        match start_x11vnc(&binary, display).await {
            Ok(server) => (Some(server), String::new()),
            Err(error) => (None, x11vnc_start_error(&error.to_string())),
        }
    }

    fn pid(&self) -> u32 {
        self.child.id().unwrap_or_default()
    }

    async fn stop(&mut self) {
        stop_process_child(&mut self.child).await;
    }
}

#[cfg(target_os = "linux")]
fn vnc_viewer_enabled() -> bool {
    !matches!(
        std::env::var("CCCC_PROJECTED_BROWSER_VNC")
            .unwrap_or_else(|_| "1".to_owned())
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "0" | "false" | "no" | "off" | "disabled"
    )
}

#[cfg(target_os = "linux")]
fn x11vnc_args(display: &str, port: u16) -> Vec<String> {
    vec![
        "-display".into(),
        display.into(),
        "-localhost".into(),
        "-nopw".into(),
        "-shared".into(),
        "-forever".into(),
        "-rfbport".into(),
        port.to_string(),
        "-quiet".into(),
    ]
}

#[cfg(target_os = "linux")]
async fn start_x11vnc(binary: &Path, display: &str) -> Result<ProjectedVncServer> {
    use tokio::process::Command;

    let port = reserve_cdp_port()?;
    let mut stderr_log = tempfile::tempfile().context("create x11vnc diagnostic buffer")?;
    let stderr_target = stderr_log
        .try_clone()
        .context("clone x11vnc diagnostic buffer")?;
    let mut child = Command::new(binary)
        .args(x11vnc_args(display, port))
        .env("DISPLAY", display)
        .env("XDG_SESSION_TYPE", "x11")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("WAYLAND_SOCKET")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::from(stderr_target))
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("start x11vnc for display {display}"))?;
    let deadline = std::time::Instant::now() + VNC_START_TIMEOUT;
    loop {
        if let Some(status) = child.try_wait()? {
            let detail = read_process_stderr(&mut stderr_log);
            bail!("x11vnc exited before becoming ready ({status}); {detail}");
        }
        if tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
            .await
            .is_ok()
        {
            return Ok(ProjectedVncServer {
                child,
                display: display.to_owned(),
                port,
                started_at: cccc_contracts::utc_now(),
            });
        }
        if std::time::Instant::now() >= deadline {
            stop_process_child(&mut child).await;
            let detail = read_process_stderr(&mut stderr_log);
            bail!("x11vnc endpoint did not become ready; {detail}");
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

#[cfg(target_os = "linux")]
fn x11vnc_start_error(detail: &str) -> String {
    let compact = detail.split_whitespace().collect::<Vec<_>>().join(" ");
    let lower = compact.to_ascii_lowercase();
    if lower.contains("wayland") {
        return "x11vnc_wayland_env_detected: x11vnc saw a Wayland session instead of the Xvfb display".to_owned();
    }
    if lower.contains("endpoint did not become ready") {
        return format!(
            "x11vnc_startup_timeout: {}",
            compact.chars().take(220).collect::<String>()
        );
    }
    compact.chars().take(300).collect()
}

#[cfg(any(target_os = "linux", test))]
fn xvfb_args(number: u16, width: u32, height: u32) -> Vec<String> {
    vec![
        format!(":{number}"),
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
async fn start_xvfb_candidate(
    binary: &Path,
    number: u16,
    width: u32,
    height: u32,
) -> Result<Option<VirtualDisplay>> {
    use tokio::io::{AsyncBufReadExt, BufReader};
    use tokio::process::Command;

    let mut stderr_log = tempfile::tempfile().context("create Xvfb diagnostic buffer")?;
    let stderr_target = stderr_log
        .try_clone()
        .context("clone Xvfb diagnostic buffer")?;
    let mut child = Command::new(binary)
        .args(xvfb_args(number, width, height))
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::from(stderr_target))
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("start Xvfb display :{number}"))?;
    let stdout = child
        .stdout
        .take()
        .context("Xvfb did not expose a display descriptor")?;
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    let result = tokio::time::timeout(XVFB_START_TIMEOUT, reader.read_line(&mut line)).await;

    if let Ok(Ok(read)) = result
        && read > 0
        && child.try_wait()?.is_none()
    {
        let reported = line.trim().trim_start_matches(':');
        if reported == number.to_string() {
            return Ok(Some(VirtualDisplay {
                child,
                name: format!(":{number}"),
            }));
        }
    }

    drop(reader);
    let timed_out = result.is_err();
    let read_error = match result {
        Ok(Err(error)) => Some(error),
        _ => None,
    };
    let status = child.try_wait()?;
    stop_process_child(&mut child).await;
    let detail = read_process_stderr(&mut stderr_log);
    if xvfb_display_in_use(number) || xvfb_display_conflict(&detail) {
        return Ok(None);
    }
    let detail = if detail.is_empty() {
        String::new()
    } else {
        format!("; {detail}")
    };
    if timed_out {
        bail!("Xvfb display :{number} startup timed out{detail}");
    }
    if let Some(error) = read_error {
        bail!("read Xvfb display :{number} descriptor: {error}{detail}");
    }
    if let Some(status) = status {
        bail!("Xvfb display :{number} exited before becoming ready ({status}){detail}");
    }
    bail!("Xvfb display :{number} did not report a usable display{detail}")
}

#[cfg(target_os = "linux")]
async fn stop_process_child(child: &mut tokio::process::Child) {
    if child.try_wait().ok().flatten().is_some() {
        return;
    }
    let _ = child.start_kill();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(3), child.wait()).await;
}

#[cfg(target_os = "linux")]
fn read_process_stderr(file: &mut File) -> String {
    const MAX_BYTES: u64 = 4096;
    let length = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
    let _ = file.seek(SeekFrom::Start(length.saturating_sub(MAX_BYTES)));
    let mut bytes = Vec::new();
    let _ = file.read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(800)
        .collect()
}

#[cfg(any(target_os = "linux", test))]
fn xvfb_display_in_use_at(temp_root: &Path, number: u16) -> bool {
    temp_root.join(format!(".X{number}-lock")).exists()
        || temp_root
            .join(".X11-unix")
            .join(format!("X{number}"))
            .exists()
}

#[cfg(target_os = "linux")]
fn xvfb_display_in_use(number: u16) -> bool {
    xvfb_display_in_use_at(Path::new("/tmp"), number)
}

#[cfg(any(target_os = "linux", test))]
fn xvfb_display_conflict(detail: &str) -> bool {
    let detail = detail.to_ascii_lowercase();
    [
        "server already active",
        "server already running",
        "failed to bind listener",
        "cannot establish any listening sockets",
    ]
    .iter()
    .any(|needle| detail.contains(needle))
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
        let args = xvfb_args(99, 800, 600);
        assert_eq!(args.first().map(String::as_str), Some(":99"));
        assert!(args.windows(2).any(|pair| pair == ["-displayfd", "1"]));
        assert!(args.windows(2).any(|pair| pair == ["-nolisten", "tcp"]));
        assert!(!args.iter().any(|arg| arg == "unix"));
        assert!(args.iter().any(|arg| arg == "1024x768x24"));
    }

    #[test]
    fn xvfb_detects_a_wsl_style_socket_without_a_legacy_lock() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(temp.path().join(".X11-unix")).expect("socket directory");
        std::fs::write(temp.path().join(".X11-unix/X0"), "socket fixture").expect("socket fixture");

        assert!(xvfb_display_in_use_at(temp.path(), 0));
        assert!(!xvfb_display_in_use_at(temp.path(), 99));
    }

    #[test]
    fn xvfb_recognizes_display_collision_diagnostics() {
        assert!(xvfb_display_conflict(
            "_XSERVTransSocketCreateListener: failed to bind listener"
        ));
        assert!(!xvfb_display_conflict("could not load a required font"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn x11vnc_is_bound_to_the_owned_display_and_loopback() {
        let args = x11vnc_args(":99", 5901);
        assert!(args.windows(2).any(|pair| pair == ["-display", ":99"]));
        assert!(args.windows(2).any(|pair| pair == ["-rfbport", "5901"]));
        assert!(args.iter().any(|arg| arg == "-localhost"));
        assert!(args.iter().any(|arg| arg == "-forever"));
        assert!(args.iter().any(|arg| arg == "-shared"));
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn x11vnc_projects_a_cccc_owned_xvfb_display() {
        let (Some(xvfb), Some(x11vnc)) = (find_executable("Xvfb"), find_executable("x11vnc"))
        else {
            return;
        };
        assert!(xvfb.is_file());
        let mut display = VirtualDisplay::start(1024, 768)
            .await
            .expect("Xvfb start")
            .expect("virtual display");
        let mut vnc = start_x11vnc(&x11vnc, display.name())
            .await
            .expect("x11vnc start");

        assert_ne!(vnc.pid(), 0);
        assert!(
            tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, vnc.port))
                .await
                .is_ok()
        );

        vnc.stop().await;
        display.stop().await;
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn concurrent_xvfb_sessions_choose_distinct_high_displays() {
        if find_executable("Xvfb").is_none() {
            return;
        }
        let (first, second) = tokio::join!(
            VirtualDisplay::start(1024, 768),
            VirtualDisplay::start(1024, 768)
        );
        let mut first = first.expect("first Xvfb start").expect("first display");
        let mut second = second.expect("second Xvfb start").expect("second display");
        assert_ne!(first.name(), second.name());
        for display in [first.name(), second.name()] {
            let number = display
                .trim_start_matches(':')
                .parse::<u16>()
                .expect("number");
            assert!(number >= XVFB_FIRST_DISPLAY);
        }
        first.stop().await;
        second.stop().await;
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn xvfb_start_failure_preserves_bounded_stderr() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let fake = temp.path().join("Xvfb");
        std::fs::write(
            &fake,
            "#!/bin/sh\necho synthetic-xvfb-failure >&2\nexit 23\n",
        )
        .expect("fake Xvfb");
        let mut permissions = std::fs::metadata(&fake).expect("metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fake, permissions).expect("permissions");

        let error = match start_xvfb_candidate(&fake, 299, 1024, 768).await {
            Err(error) => error,
            Ok(_) => panic!("fake Xvfb should fail"),
        };
        assert!(error.to_string().contains("synthetic-xvfb-failure"));
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

    #[test]
    fn windows_fixed_candidates_include_edge_under_program_files_x86() {
        let root = PathBuf::from(r"C:\Program Files (x86)");
        assert!(
            windows_fixed_browser_candidates([root.clone()])
                .iter()
                .any(|(path, channel)| *channel == "msedge"
                    && path == &root.join("Microsoft/Edge/Application/msedge.exe"))
        );
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
            #[cfg(target_os = "linux")]
            vnc: None,
            vnc_error: "unsupported_platform".to_owned(),
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

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn non_macos_browser_lets_chromium_assign_the_cdp_port_atomically() {
        assert_eq!(initial_cdp_port().expect("initial CDP port"), 0);
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn reads_the_browser_assigned_cdp_port_from_its_websocket_address() {
        assert_eq!(
            browser_assigned_cdp_port("ws://127.0.0.1:41234/devtools/browser/example"),
            Some(41234)
        );
        assert_eq!(
            browser_assigned_cdp_port("ws://[::1]:41235/devtools/browser/example"),
            Some(41235)
        );
        assert_eq!(browser_assigned_cdp_port("https://127.0.0.1:41234"), None);
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
            #[cfg(target_os = "linux")]
            vnc: None,
            vnc_error: "unsupported_platform".to_owned(),
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
