use cccc_contracts::utc_now;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::PathBuf;

use crate::actors::validate_actor_id;
use crate::fs::{read_json, with_exclusive_lock, write_json};
use crate::{GroupStore, HomeLayout};

const STATE_VERSION: u8 = 1;
const STATE_FILENAME: &str = "runtime-state.json";

#[derive(Debug, Clone, Deserialize, Serialize)]
struct RestartGateState {
    v: u8,
    group_id: String,
    actor_id: String,
    actor_created_at: String,
    generation: String,
    manual_restart_required: bool,
    reason_code: String,
    updated_at: String,
}

pub fn record_running_generation(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    actor_created_at: &str,
    generation: &str,
) -> io::Result<()> {
    let generation = generation.trim();
    if generation.is_empty() {
        return Err(io::Error::other("DeepSeek launch generation is required"));
    }
    let (state_path, lock_path) = paths(home, group_id, actor_id)?;
    with_exclusive_lock(&lock_path, || {
        write_json(
            &state_path,
            &RestartGateState {
                v: STATE_VERSION,
                group_id: group_id.to_owned(),
                actor_id: actor_id.to_owned(),
                actor_created_at: actor_created_at.trim().to_owned(),
                generation: generation.to_owned(),
                manual_restart_required: false,
                reason_code: String::new(),
                updated_at: utc_now(),
            },
        )
    })
}

pub fn require_manual_restart(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    actor_created_at: &str,
    expected_generation: &str,
    reason_code: &str,
) -> io::Result<bool> {
    let expected_generation = expected_generation.trim();
    if expected_generation.is_empty() {
        return Err(io::Error::other("DeepSeek launch generation is required"));
    }
    let (state_path, lock_path) = paths(home, group_id, actor_id)?;
    with_exclusive_lock(&lock_path, || {
        let Some(mut state) = read_optional(&state_path)? else {
            return Ok(false);
        };
        if state.v != STATE_VERSION
            || state.group_id != group_id
            || state.actor_id != actor_id
            || state.actor_created_at != actor_created_at.trim()
            || state.generation != expected_generation
        {
            return Ok(false);
        }
        state.manual_restart_required = true;
        state.reason_code = reason_code.trim().to_owned();
        state.updated_at = utc_now();
        write_json(&state_path, &state)?;
        Ok(true)
    })
}

pub fn manual_restart_required(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    actor_created_at: &str,
) -> io::Result<bool> {
    let (state_path, lock_path) = paths(home, group_id, actor_id)?;
    with_exclusive_lock(&lock_path, || {
        let Some(state) = read_optional(&state_path)? else {
            return Ok(false);
        };
        if state.v != STATE_VERSION {
            return Err(io::Error::other(format!(
                "unsupported DeepSeek runtime state version {}",
                state.v
            )));
        }
        Ok(state.group_id == group_id
            && state.actor_id == actor_id
            && state.actor_created_at == actor_created_at.trim()
            && state.manual_restart_required)
    })
}

fn read_optional(path: &std::path::Path) -> io::Result<Option<RestartGateState>> {
    match read_json(path) {
        Ok(state) => Ok(Some(state)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn paths(home: &HomeLayout, group_id: &str, actor_id: &str) -> io::Result<(PathBuf, PathBuf)> {
    let actor_id = validate_actor_id(actor_id)?;
    let directory = GroupStore::new(home.clone())?
        .state_dir(group_id)?
        .join("deepseek")
        .join(actor_id);
    let state_path = directory.join(STATE_FILENAME);
    let lock_path = directory.join(format!("{STATE_FILENAME}.lock"));
    Ok((state_path, lock_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_survives_reads_and_rejects_stale_generations() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("deepseek gate", "").expect("group");

        record_running_generation(&home, &group.group_id, "peer1", "actor-v1", "launch-1")
            .expect("record launch");
        assert!(
            !manual_restart_required(&home, &group.group_id, "peer1", "actor-v1")
                .expect("read open gate")
        );
        assert!(
            require_manual_restart(
                &home,
                &group.group_id,
                "peer1",
                "actor-v1",
                "launch-1",
                "credential_unavailable",
            )
            .expect("close gate")
        );
        assert!(
            manual_restart_required(&home, &group.group_id, "peer1", "actor-v1")
                .expect("read closed gate")
        );

        record_running_generation(&home, &group.group_id, "peer1", "actor-v1", "launch-2")
            .expect("record replacement launch");
        assert!(
            !require_manual_restart(
                &home,
                &group.group_id,
                "peer1",
                "actor-v1",
                "launch-1",
                "stale_failure",
            )
            .expect("reject stale failure")
        );
        assert!(
            !manual_restart_required(&home, &group.group_id, "peer1", "actor-v1")
                .expect("replacement remains open")
        );
    }

    #[test]
    fn recreated_actor_does_not_inherit_the_old_gate() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("deepseek gate", "").expect("group");

        record_running_generation(&home, &group.group_id, "peer1", "actor-v1", "launch-1")
            .expect("record launch");
        require_manual_restart(
            &home,
            &group.group_id,
            "peer1",
            "actor-v1",
            "launch-1",
            "context_window_exceeded",
        )
        .expect("close gate");

        assert!(
            !manual_restart_required(&home, &group.group_id, "peer1", "actor-v2")
                .expect("read recreated actor")
        );
    }
}
