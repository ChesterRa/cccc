use anyhow::Result;
use cccc_client::DaemonClient;
use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use serde_json::{Value, json};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::time::Duration;

mod installation;

pub async fn run(
    home: &HomeLayout,
    product_version: &str,
    all_runtimes: bool,
    json: bool,
) -> Result<()> {
    let browser = cccc_web::system_browser_path();
    let xvfb = find_command("Xvfb");
    let x11vnc = find_command("x11vnc");
    let daemon = daemon_status(home).await;
    let report = report(
        home,
        product_version,
        browser.as_deref(),
        xvfb.as_deref(),
        x11vnc.as_deref(),
        daemon,
        all_runtimes,
    );
    // Scripts and bug reports keep the full report; a person reads the summary.
    if json || !std::io::stdout().is_terminal() {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("{}", summary(&report));
    }
    Ok(())
}

fn summary(report: &Value) -> String {
    let text = |value: &Value| value.as_str().unwrap_or("unknown").to_owned();
    let source_id = |build: &Value| build["source_id"].as_str().map(str::to_owned);
    let short = |id: Option<&str>| id.unwrap_or("unknown").chars().take(8).collect::<String>();
    let installation = &report["installation"];
    let command = match installation["path_status"].as_str() {
        Some("ok") => text(&installation["resolved_command"]),
        Some("conflict") => format!(
            "`cccc` runs {}, not this copy ({})",
            text(&installation["resolved_command"]),
            text(&installation["current_executable"]),
        ),
        Some("missing") => "`cccc` is not on PATH".to_owned(),
        _ => "unknown".to_owned(),
    };
    let build = source_id(&report["build"]);
    let daemon = &report["daemon"];
    let daemon = if daemon["running"] == true {
        let daemon_build = source_id(&daemon["build"]);
        // A daemon that names no build proves nothing about being out of date.
        let restart = match (&daemon_build, &build) {
            (Some(daemon), Some(cli)) if daemon != cli => "; not this build, restart it to update",
            _ => "",
        };
        format!(
            "running, pid {}, build {}{restart}",
            daemon["pid"],
            short(daemon_build.as_deref())
        )
    } else {
        "not reachable; normal before CCCC is first started".to_owned()
    };
    let agents = report["runtimes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|runtime| !runtime["path"].is_null())
        .map(|runtime| text(&runtime["display_name"]))
        .collect::<Vec<_>>()
        .join(", ");
    let agents = if agents.is_empty() {
        "no agent CLI found".to_owned()
    } else {
        agents
    };
    let browser = &report["projected_browser"];
    let web_model = if browser["system_browser_available"] == true {
        "browser available"
    } else if browser["browser_available"] == true {
        "browser found, Xvfb missing"
    } else {
        "no Chrome, Edge or Chromium found"
    };
    format!(
        "CCCC {} (build {})\n  Home       {}\n  Command    {command}\n  Daemon     {daemon}\n  \
         Agents     {agents}\n  Web Model  {web_model}\n\nAdd --json for the full report.",
        text(&report["version"]),
        short(build.as_deref()),
        text(&report["home"]),
    )
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
            "build":response.result.get("build").cloned().unwrap_or(Value::Null),
            "executable":response.result.get("executable").cloned().unwrap_or(Value::Null),
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
        "version":product_version,
        "build":cccc_core::build_info::current(),
        "web_assets":cccc_web::web_assets_info().map(|info| info.assets_id),
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
    fn summary_does_not_read_as_a_failure_before_the_first_start() {
        let text = summary(&json!({
            "version":"1.2.3",
            "build":{"source_id":"abcdef0123456789"},
            "home":"/home/a/.cccc",
            "installation":{"path_status":"ok","resolved_command":"/usr/bin/cccc"},
            "daemon":{"running":false,"error":"daemon address unavailable"},
            "runtimes":[
                {"display_name":"Claude Code","path":"/usr/bin/claude"},
                {"display_name":"Codex CLI","path":null},
            ],
            "projected_browser":{"browser_available":true,"system_browser_available":true},
        }));

        assert!(text.starts_with("CCCC 1.2.3 (build abcdef01)\n"));
        assert!(text.contains("Command    /usr/bin/cccc\n"));
        assert!(text.contains("Daemon     not reachable; normal before"));
        assert!(text.contains("Agents     Claude Code\n"));
        assert!(text.contains("Web Model  browser available"));
        assert!(!text.contains("error"));
    }

    #[test]
    fn summary_names_what_is_missing_or_out_of_date() {
        let text = summary(&json!({
            "version":"1.2.3",
            "build":{"source_id":"abcdef0123456789"},
            "home":"/home/a/.cccc",
            "installation":{
                "path_status":"conflict",
                "resolved_command":"/opt/old/cccc",
                "current_executable":"/usr/bin/cccc",
            },
            "daemon":{"running":true,"pid":42,"build":{"source_id":"1234567890abcdef"}},
            "runtimes":[],
            "projected_browser":{"browser_available":true,"system_browser_available":false},
        }));

        assert!(text.contains("`cccc` runs /opt/old/cccc, not this copy (/usr/bin/cccc)"));
        assert!(text.contains("running, pid 42, build 12345678; not this build"));
        assert!(text.contains("no agent CLI found"));
        assert!(text.contains("Web Model  browser found, Xvfb missing"));
    }

    #[test]
    fn summary_does_not_call_a_daemon_without_a_build_out_of_date() {
        let text = summary(&json!({
            "version":"1.2.3",
            "build":{"source_id":"abcdef0123456789"},
            "home":"/home/a/.cccc",
            "installation":{"path_status":"missing"},
            "daemon":{"running":true,"pid":42,"build":null},
            "runtimes":[],
            "projected_browser":{"browser_available":false,"system_browser_available":false},
        }));

        assert!(text.contains("running, pid 42, build unknown\n"));
        assert!(!text.contains("restart"));
        assert!(text.contains("no Chrome, Edge or Chromium found"));
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
