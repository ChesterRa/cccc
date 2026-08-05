use crate::HomeLayout;
use crate::fs::{read_json, write_json_committed};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeHookLaunchIdentity {
    pub v: u8,
    pub group_id: String,
    pub actor_id: String,
    pub runtime: String,
    pub launch_token: String,
    pub hook_enabled: bool,
    pub pid: u32,
}

impl RuntimeHookLaunchIdentity {
    #[must_use]
    pub fn new(
        group_id: &str,
        actor_id: &str,
        runtime: &str,
        launch_token: &str,
        hook_enabled: bool,
        pid: u32,
    ) -> Self {
        Self {
            v: 1,
            group_id: group_id.to_owned(),
            actor_id: actor_id.to_owned(),
            runtime: runtime.to_owned(),
            launch_token: launch_token.to_owned(),
            hook_enabled,
            pid,
        }
    }
}

#[must_use]
pub fn path(home: &HomeLayout, group_id: &str, actor_id: &str) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(group_id.as_bytes());
    hasher.update([0]);
    hasher.update(actor_id.as_bytes());
    let key = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    home.daemon_dir()
        .join("runtime-hook-launch")
        .join(format!("{key}.json"))
}

pub fn write(home: &HomeLayout, identity: &RuntimeHookLaunchIdentity) -> io::Result<()> {
    validate(identity)?;
    write_json_committed(
        &path(home, &identity.group_id, &identity.actor_id),
        identity,
    )
}

#[must_use]
pub fn read(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
) -> Option<RuntimeHookLaunchIdentity> {
    let identity = read_json::<RuntimeHookLaunchIdentity>(&path(home, group_id, actor_id)).ok()?;
    (validate(&identity).is_ok() && identity.group_id == group_id && identity.actor_id == actor_id)
        .then_some(identity)
}

pub fn remove(home: &HomeLayout, group_id: &str, actor_id: &str) -> io::Result<()> {
    match std::fs::remove_file(path(home, group_id, actor_id)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn validate(identity: &RuntimeHookLaunchIdentity) -> io::Result<()> {
    if identity.v != 1
        || identity.group_id.trim().is_empty()
        || identity.actor_id.trim().is_empty()
        || !matches!(identity.runtime.as_str(), "codex" | "claude")
        || identity.launch_token.trim().is_empty()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid runtime hook launch identity",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_python_compatible_identity_and_rejects_invalid_pid() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let identity = RuntimeHookLaunchIdentity::new("g_test", "peer", "codex", "token", true, 42);
        write(&home, &identity).expect("write identity");
        assert_eq!(read(&home, "g_test", "peer"), Some(identity));

        std::fs::write(
            path(&home, "g_test", "peer"),
            br#"{"v":1,"group_id":"g_test","actor_id":"peer","runtime":"codex","launch_token":"token","hook_enabled":true,"pid":"corrupt"}"#,
        )
        .expect("corrupt identity");
        assert!(read(&home, "g_test", "peer").is_none());
    }
}
