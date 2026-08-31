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
    runtime_proof_key: &str,
) -> io::Result<()> {
    fs::write_secret_json(
        &path(home),
        &json!({
            "pid":std::process::id(),
            "runtime_id":runtime_id,
            "runtime_proof_key":runtime_proof_key,
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn runtime_proof_key_is_written_in_an_owner_only_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");

        write(
            &home,
            "127.0.0.1",
            9123,
            "normal",
            false,
            "web-test",
            "proof-secret",
        )
        .expect("write runtime state");

        let runtime_path = path(&home);
        let runtime: Value = fs::read_json(&runtime_path).expect("runtime state");
        assert_eq!(runtime["runtime_proof_key"], "proof-secret");
        assert_eq!(
            std::fs::metadata(runtime_path)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
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
