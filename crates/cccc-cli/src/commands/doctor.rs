use anyhow::Result;
use cccc_client::DaemonClient;
use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

mod installation;

pub async fn run(home: &HomeLayout, product_version: &str, all_runtimes: bool) -> Result<()> {
    let browser = cccc_web::system_browser_path();
    let xvfb = find_command("Xvfb");
    let x11vnc = find_command("x11vnc");
    let daemon = daemon_status(home).await;
    println!(
        "{}",
        serde_json::to_string_pretty(&report(
            home,
            product_version,
            browser.as_deref(),
            xvfb.as_deref(),
            x11vnc.as_deref(),
            daemon,
            all_runtimes,
        ))?
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
    xvfb: Option<&Path>,
    x11vnc: Option<&Path>,
    daemon: Value,
    all_runtimes: bool,
) -> Value {
    let linux = cfg!(target_os = "linux");
    let mut runtimes = cccc_runtime::detect_runtimes();
    if !all_runtimes {
        runtimes.retain(|runtime| runtime.name != "custom");
    }
    json!({
        "implementation":"rust",
        "version":product_version,
        "home":home.root(),
        "installation":installation::report(),
        "daemon":daemon,
        "runtimes":runtimes,
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
            "xvfb_required":linux,
            "system_browser_available":browser.is_some() && (!linux || xvfb.is_some()),
            "system_browser_path":browser,
            "xvfb_available":xvfb.is_some(),
            "xvfb_path":xvfb,
            "x11vnc_available":x11vnc.is_some(),
            "x11vnc_path":x11vnc,
            "xvfb_required_for_linux_web_model":linux,
            "note":if linux {
                "Web Model uses system Chrome/Edge/Chromium via an isolated Xvfb display; Xvfb is required on Linux."
            } else {
                "Web Model uses system Chrome/Edge/Chromium via CDP."
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_hybrid_browser_contract() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let browser = Path::new("/usr/bin/google-chrome");
        let xvfb = Path::new("/usr/bin/Xvfb");
        let value = report(
            &home,
            "0.4.33",
            Some(browser),
            Some(xvfb),
            None,
            json!({"running":false}),
            false,
        );
        assert_eq!(value["version"], "0.4.33");
        assert!(value["installation"]["path_status"].is_string());
        assert!(value["installation"]["command_candidates"].is_array());
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
        assert_eq!(
            value["projected_browser"]["xvfb_required"],
            cfg!(target_os = "linux")
        );
        assert_eq!(value["projected_browser"]["system_browser_available"], true);
        assert_eq!(
            value["projected_browser"]["system_browser_path"],
            browser.to_string_lossy().as_ref()
        );
        assert_eq!(
            value["projected_browser"]["xvfb_required_for_linux_web_model"],
            cfg!(target_os = "linux")
        );
        assert!(
            value["runtimes"]
                .as_array()
                .expect("runtimes")
                .iter()
                .all(|runtime| runtime["name"] != "custom")
        );
    }

    #[test]
    fn linux_system_browser_contract_requires_xvfb() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let value = report(
            &home,
            "0.4.33",
            Some(Path::new("/usr/bin/chromium")),
            None,
            None,
            json!({"running":false}),
            true,
        );
        assert_eq!(
            value["projected_browser"]["system_browser_available"],
            !cfg!(target_os = "linux")
        );
        assert!(
            value["runtimes"]
                .as_array()
                .expect("runtimes")
                .iter()
                .any(|runtime| runtime["name"] == "custom")
        );
    }
}
