use anyhow::Result;
use cccc_client::DaemonClient;
use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub async fn run(home: &HomeLayout, product_version: &str) -> Result<()> {
    let browser = find_browser();
    let daemon = daemon_status(home).await;
    println!(
        "{}",
        serde_json::to_string_pretty(&report(home, product_version, browser.as_deref(), daemon,))?
    );
    Ok(())
}

async fn daemon_status(home: &HomeLayout) -> Value {
    let client = DaemonClient::new(home.clone()).with_timeout(Duration::from_millis(750));
    let request = DaemonRequest {
        v: 1,
        op: "ping".into(),
        args: Default::default(),
    };
    match client.call(&request).await {
        Ok(response) if response.ok => json!({
            "running":true,
            "pid":response.result.get("pid").cloned().unwrap_or(Value::Null),
            "version":response.result.get("version").cloned().unwrap_or(Value::Null),
            "implementation":response.result.get("implementation").cloned().unwrap_or(Value::Null),
        }),
        Ok(response) => json!({
            "running":false,
            "error":response.error.map(|error| error.message),
        }),
        Err(error) => json!({"running":false,"error":error.to_string()}),
    }
}

fn report(
    home: &HomeLayout,
    product_version: &str,
    browser: Option<&Path>,
    daemon: Value,
) -> Value {
    let xvfb = find_command("Xvfb");
    let x11vnc = find_command("x11vnc");
    json!({
        "implementation":"rust",
        "version":product_version,
        "home":home.root(),
        "daemon":daemon,
        "runtimes":cccc_runtime::detect_runtimes(),
        "pty":{
            "supported":true,
            "backend":if cfg!(windows){"ConPTY"}else{"native PTY"},
        },
        "projected_browser":{
            "mode":"hybrid",
            "web_model_mode":"system_browser_cdp",
            "other_surface_mode":"headless",
            "browser_available":browser.is_some(),
            "browser_path":browser,
            "xvfb_required":false,
            "system_browser_available":browser.is_some(),
            "system_browser_path":browser,
            "xvfb_available":xvfb.is_some(),
            "xvfb_path":xvfb,
            "x11vnc_available":x11vnc.is_some(),
            "x11vnc_path":x11vnc,
            "xvfb_required_for_linux_web_model":false,
            "note":"Web Model uses system Chrome/Edge/Chromium via CDP; Linux uses Xvfb when available and otherwise falls back to headless CDP projection."
        }
    })
}

fn find_command(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .map(|directory| directory.join(name))
        .find(|path| path.is_file())
}

fn find_browser() -> Option<PathBuf> {
    let names = if cfg!(target_os = "windows") {
        &["chrome.exe", "msedge.exe", "chromium.exe"][..]
    } else {
        &[
            "google-chrome",
            "google-chrome-stable",
            "microsoft-edge",
            "microsoft-edge-stable",
            "chromium",
            "chromium-browser",
        ][..]
    };
    if let Some(path) = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .flat_map(|directory| names.iter().map(move |name| directory.join(name)))
            .find(|path| path.is_file())
    }) {
        return Some(path);
    }
    if cfg!(target_os = "macos") {
        [
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
        ]
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_hybrid_browser_contract() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let browser = Path::new("/usr/bin/google-chrome");
        let value = report(&home, "0.4.33", Some(browser), json!({"running":false}));
        assert_eq!(value["version"], "0.4.33");
        assert_eq!(value["daemon"]["running"], false);
        assert_eq!(value["projected_browser"]["mode"], "hybrid");
        assert_eq!(
            value["projected_browser"]["web_model_mode"],
            "system_browser_cdp"
        );
        assert_eq!(value["projected_browser"]["other_surface_mode"], "headless");
        assert_eq!(value["projected_browser"]["browser_available"], true);
        assert_eq!(
            value["projected_browser"]["browser_path"],
            browser.to_string_lossy().as_ref()
        );
        assert_eq!(value["projected_browser"]["xvfb_required"], false);
        assert_eq!(value["projected_browser"]["system_browser_available"], true);
        assert_eq!(
            value["projected_browser"]["system_browser_path"],
            browser.to_string_lossy().as_ref()
        );
        assert_eq!(
            value["projected_browser"]["xvfb_required_for_linux_web_model"],
            false
        );
    }
}
