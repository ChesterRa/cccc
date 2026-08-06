use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use cccc_core::fs::with_exclusive_lock;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::dispatch::{OpError, OpResult, bool_arg, object, string_arg};

mod config;
mod process;

use config::{
    cccc_command, find_executable, hermes_home, inspect_mcp, load_config, normalize_placeholders,
};
use process::run;

pub(super) const SERVER: &str = "cccc";
pub(super) const PLACEHOLDERS: [(&str, &str); 3] = [
    ("CCCC_HOME", "${CCCC_HOME}"),
    ("CCCC_GROUP_ID", "${CCCC_GROUP_ID}"),
    ("CCCC_ACTOR_ID", "${CCCC_ACTOR_ID}"),
];

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    Some(match request.op.as_str() {
        "runtime_hermes_status" => status(home),
        "runtime_hermes_prepare" => prepare(home, request),
        "runtime_hermes_mcp_test" => mcp_test(home, request),
        _ => return None,
    })
}

pub(super) fn ensure_for_actor(
    home: &HomeLayout,
    cwd: &Path,
    env: &BTreeMap<String, String>,
) -> Result<(), OpError> {
    let profile = env
        .get("HERMES_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(hermes_home);
    let config_path = profile.join("config.yaml");
    let command = cccc_command();
    if inspect_mcp(&load_config(&config_path), &command)["status"] == "ready" {
        return Ok(());
    }
    let hermes = find_executable_in_env("hermes", env, cwd)
        .or_else(|| find_executable("hermes"))
        .ok_or_else(|| {
            OpError::new(
                "runtime_mcp_cli_missing",
                "Hermes CLI is not installed or not in PATH",
            )
        })?;
    let mut argv = vec![
        "mcp".into(),
        "add".into(),
        SERVER.into(),
        "--command".into(),
        command[0].clone(),
    ];
    if command.len() > 1 {
        argv.push("--args".into());
        argv.extend(command[1..].iter().cloned());
    }
    argv.push("--env".into());
    argv.extend([
        format!("CCCC_HOME={}", home.root().display()),
        "CCCC_GROUP_ID=${CCCC_GROUP_ID}".into(),
        "CCCC_ACTOR_ID=${CCCC_ACTOR_ID}".into(),
    ]);
    let values = env
        .iter()
        .map(|(key, value)| (key.as_str(), value.clone()))
        .collect::<Vec<_>>();
    let result = run(
        &hermes,
        &argv,
        Some(cwd),
        Some("Y\n"),
        &values,
        Duration::from_secs(120),
    )
    .map_err(OpError::io)?;
    if result["returncode"] != 0 {
        return Err(OpError::new(
            "runtime_mcp_setup_failed",
            result["stderr"]
                .as_str()
                .unwrap_or("Hermes MCP setup failed"),
        ));
    }
    normalize_placeholders(&config_path).map_err(OpError::io)?;
    if inspect_mcp(&load_config(&config_path), &command)["status"] != "ready" {
        return Err(OpError::new(
            "runtime_mcp_verification_failed",
            "Hermes MCP setup completed but the CCCC entry is still stale",
        ));
    }
    Ok(())
}

fn find_executable_in_env(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Option<PathBuf> {
    let inherited_path = std::env::var_os("PATH");
    let search_path = env
        .get("PATH")
        .map(std::ffi::OsStr::new)
        .or(inherited_path.as_deref());
    cccc_core::runtime_mcp::find_program_in(name, search_path, cwd)
}

fn status(home: &HomeLayout) -> OpResult {
    let hermes = find_executable("hermes");
    let profile = hermes_home();
    let config_path = profile.join("config.yaml");
    let config = load_config(&config_path);
    let mcp = inspect_mcp(&config, &cccc_command());
    let auth_path = profile.join("auth.json");
    let auth_present = std::fs::read_to_string(&auth_path)
        .is_ok_and(|text| text.to_ascii_lowercase().contains("xai-oauth"));
    let mut issues = Vec::new();
    if hermes.is_none() {
        issues.push("hermes_cli_missing");
    }
    if !config_path.exists() {
        issues.push("config_missing");
    }
    if mcp["status"] != "ready" {
        issues.push("cccc_mcp_config_not_ready");
    }
    if !auth_present {
        issues.push("xai_oauth_missing");
    }
    let version = hermes
        .as_ref()
        .and_then(|path| {
            run(
                path,
                &["--version".into()],
                None,
                None,
                &[],
                Duration::from_secs(10),
            )
            .ok()
        })
        .filter(|output| output["returncode"] == 0)
        .and_then(|output| {
            output["stdout"]
                .as_str()
                .map(|value| value.trim().lines().next().unwrap_or("").to_owned())
        })
        .unwrap_or_default();
    object(json!({
        "runtime":"hermes","phase":"phase1_pty_runtime_mvp",
        "setup_ready":mcp["status"]=="ready","auth_ready":auth_present,
        "launch_ready":hermes.is_some() && mcp["status"]=="ready",
        "hermes_cli":{"available":hermes.is_some(),"path":hermes,"version":version},
        "hermes_home":profile,"profile":{"name":"default","dir":profile,"exists":profile.exists(),"config_path":config_path,"config_exists":config_path.exists()},
        "mcp":mcp,"auth":{"provider":"xai-oauth","auth_path":auth_path,"status":if auth_present{"present"}else{"missing"}},
        "issues":issues,
        "commands":{"prepare":"cccc runtime hermes prepare --yes","mcp_test":"cccc runtime hermes mcp-test","auth_add":"hermes auth add xai-oauth","launch":"hermes --tui --yolo"},
        "cccc_home":home.root()
    }))
}

fn prepare(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let force = bool_arg(request, "force_mcp", bool_arg(request, "force", false));
    let yes = bool_arg(
        request,
        "auto_enable_tools",
        bool_arg(request, "yes", false),
    );
    let current = status(home)?;
    if !force && current["mcp"]["status"] == "ready" {
        return object(json!({"ok":true,"commands_run":[],"status":current}));
    }
    if !yes {
        return Err(OpError::new(
            "hermes_mcp_setup_requires_confirmation",
            "rerun with --yes to enable discovered CCCC tools",
        ));
    }
    let hermes = find_executable("hermes").ok_or_else(|| {
        OpError::new(
            "hermes_cli_missing",
            "Hermes CLI is not installed or not in PATH",
        )
    })?;
    let command = cccc_command();
    let mut argv = vec![
        "mcp".into(),
        "add".into(),
        SERVER.into(),
        "--command".into(),
        command[0].clone(),
    ];
    if command.len() > 1 {
        argv.push("--args".into());
        argv.extend(command[1..].iter().cloned());
    }
    argv.push("--env".into());
    argv.extend([
        format!("CCCC_HOME={}", home.root().display()),
        "CCCC_GROUP_ID=g_probe".into(),
        "CCCC_ACTOR_ID=hermes-probe".into(),
    ]);
    let cwd = string_arg(request, "cwd").map(PathBuf::from);
    let result = with_exclusive_lock(&home.daemon_dir().join("hermes-runtime-setup.lock"), || {
        run(
            &hermes,
            &argv,
            cwd.as_deref(),
            Some("Y\n"),
            &[],
            Duration::from_secs(120),
        )
    })
    .map_err(OpError::io)?;
    if result["returncode"] != 0 {
        return Err(OpError::new(
            "hermes_mcp_add_failed",
            result["stderr"]
                .as_str()
                .unwrap_or("Hermes MCP setup failed"),
        ));
    }
    normalize_placeholders(&hermes_home().join("config.yaml")).map_err(OpError::io)?;
    object(
        json!({"ok":true,"commands_run":[{"name":"mcp_add","argv":argv,"result":result}],"status":status(home)?}),
    )
}

fn mcp_test(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    let hermes = find_executable("hermes").ok_or_else(|| {
        OpError::new(
            "hermes_cli_missing",
            "Hermes CLI is not installed or not in PATH",
        )
    })?;
    let argv = vec!["mcp".into(), "test".into(), SERVER.into()];
    let group = string_arg(request, "group_id").unwrap_or_else(|| "g_probe".into());
    let actor = string_arg(request, "actor_id").unwrap_or_else(|| "hermes-probe".into());
    let env = [
        ("CCCC_HOME", home.root().to_string_lossy().into_owned()),
        ("CCCC_GROUP_ID", group),
        ("CCCC_ACTOR_ID", actor),
    ];
    let result = run(
        &hermes,
        &argv,
        string_arg(request, "cwd").as_deref().map(Path::new),
        None,
        &env,
        Duration::from_secs(60),
    )
    .map_err(OpError::io)?;
    if result["returncode"] != 0 {
        return Err(OpError::new(
            "hermes_mcp_test_failed",
            result["stderr"]
                .as_str()
                .unwrap_or("Hermes MCP test failed"),
        ));
    }
    object(json!({"ok":true,"argv":argv,"result":result}))
}
