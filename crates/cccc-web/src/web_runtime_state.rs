use cccc_core::{HomeLayout, fs};
use serde_json::{Value, json};
use std::io;

fn path(home: &HomeLayout) -> std::path::PathBuf {
    home.daemon_dir().join("web_runtime.json")
}

pub(super) fn write(
    home: &HomeLayout,
    host: &str,
    port: u16,
    mode: &str,
    supervisor_managed: bool,
    runtime_id: &str,
) -> io::Result<()> {
    fs::write_json(
        &path(home),
        &json!({
            "pid":std::process::id(),
            "runtime_id":runtime_id,
            "host":host,
            "port":port,
            "mode":mode,
            "started_at":cccc_contracts::utc_now(),
            "supervisor_managed":supervisor_managed,
            "supervisor_pid":Value::Null,
            "launcher_pid":Value::Null,
            "launch_source":"rust",
            "last_apply_error":Value::Null,
        }),
    )
}

pub(super) fn clear_if_owner(home: &HomeLayout) -> io::Result<()> {
    let runtime: Value = match fs::read_json(&path(home)) {
        Ok(runtime) => runtime,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if runtime.get("pid").and_then(Value::as_u64) == Some(u64::from(std::process::id())) {
        match std::fs::remove_file(path(home)) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
