//! Antigravity native TUI lifecycle hook state.
//!
//! Antigravity's default terminal mode runs inside a PTY without a managed
//! protocol connection. Its official lifecycle hooks (`SessionStart`,
//! `PreInvocation`, `Stop`) inherit the Actor launch environment and report
//! turn activity here so CCCC can project working/idle state and admit early
//! Mail notices without interrupting active work.
//!
//! State files are isolated per launch (`<actor_hash>/<launch_id>.state.json`),
//! while `<actor_hash>.launch.json` binds the live PTY's `(pid, started_at)` to
//! the active `launch_id`. This prevents a new launch's early `SessionStart`
//! from being rejected while an older binding is still on disk, and prevents
//! late hooks from an older launch from ever mutating or revoking a newer
//! launch's state.

use crate::HomeLayout;
use cccc_contracts::utc_now;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};

pub const ENV_STATE_FILE: &str = "CCCC_ANTIGRAVITY_HOOK_STATE_FILE";
pub const ENV_LAUNCH_ID: &str = "CCCC_ANTIGRAVITY_HOOK_LAUNCH_ID";
pub const HOOK_ENTRY_NAME: &str = "cccc-runtime-state";
pub const MAX_HOOK_STDIN_BYTES: usize = 1024 * 1024;

const STATE_SCHEMA_VERSION: u32 = 1;
const STATE_FILE_SUFFIX: &str = ".state.json";
const DEFAULT_CONVERSATION_KEY: &str = "__default__";
const MAX_ACTIVE_CONVERSATIONS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchBinding {
    pub v: u32,
    pub group_id: String,
    pub actor_id: String,
    pub launch_id: String,
    pub pid: Option<u32>,
    pub started_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookState {
    pub v: u32,
    pub group_id: String,
    pub actor_id: String,
    pub launch_id: String,
    pub status: String,
    pub event: String,
    #[serde(default)]
    pub active_conversations: BTreeSet<String>,
    /// True once this launch has observed an event it could not track exactly
    /// (for example an oversized/malformed hook payload or more than
    /// `MAX_ACTIVE_CONVERSATIONS` concurrent active conversations). While set,
    /// `read_live_state` withholds idle/working projection so CCCC stays on its
    /// conservative fallback instead of acting on stale idle evidence.
    #[serde(default)]
    pub uncertain: bool,
    pub updated_at: String,
}

/// Shared global command registered in `~/.gemini/config/hooks.json`.
///
/// Sessions launched outside CCCC do not set `CCCC_ANTIGRAVITY_HOOK_STATE_FILE`
/// and return `{}` immediately without spawning `cccc`.
#[must_use]
pub const fn hook_command() -> &'static str {
    #[cfg(windows)]
    {
        r#"if defined CCCC_ANTIGRAVITY_HOOK_STATE_FILE (cccc runtime antigravity-hook || echo {}) else (echo {})"#
    }
    #[cfg(not(windows))]
    {
        r#"if [ -n "$CCCC_ANTIGRAVITY_HOOK_STATE_FILE" ]; then cccc runtime antigravity-hook || printf '{}'; else printf '{}'; fi"#
    }
}

/// Single command handler object inserted into each tracked lifecycle event.
#[must_use]
pub fn hook_handler() -> Value {
    json!({
        "type": "command",
        "command": hook_command(),
        "timeout": 5
    })
}

/// Global hook definition stored under `HOOK_ENTRY_NAME` in
/// `~/.gemini/config/hooks.json`.
#[must_use]
pub fn hook_entry() -> Value {
    let handler = hook_handler();
    json!({
        "SessionStart": [handler.clone()],
        "PreInvocation": [handler.clone()],
        "Stop": [handler]
    })
}

fn actor_key_hash(actor_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(actor_id.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn valid_launch_id(launch_id: &str) -> bool {
    !launch_id.is_empty()
        && launch_id.len() <= 128
        && launch_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn state_dir(home: &HomeLayout, group_id: &str) -> PathBuf {
    home.root()
        .join("groups")
        .join(group_id)
        .join("state")
        .join("antigravity_hooks")
}

fn actor_launches_dir(home: &HomeLayout, group_id: &str, actor_id: &str) -> PathBuf {
    state_dir(home, group_id).join(actor_key_hash(actor_id))
}

#[must_use]
pub fn state_file_path(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    launch_id: &str,
) -> PathBuf {
    let key = if valid_launch_id(launch_id) {
        launch_id
    } else {
        "invalid"
    };
    actor_launches_dir(home, group_id, actor_id).join(format!("{key}{STATE_FILE_SUFFIX}"))
}

#[must_use]
pub fn launch_file_path(home: &HomeLayout, group_id: &str, actor_id: &str) -> PathBuf {
    state_dir(home, group_id).join(format!("{}.launch.json", actor_key_hash(actor_id)))
}

fn lock_file_for(state_file: &Path, launch_id: &str) -> io::Result<PathBuf> {
    if !valid_launch_id(launch_id) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid Antigravity hook launch ID",
        ));
    }
    let name = state_file
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid Antigravity hook state path",
            )
        })?;
    let stem = name.strip_suffix(STATE_FILE_SUFFIX).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Antigravity hook state path must end with .state.json",
        )
    })?;
    if stem != launch_id {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Antigravity hook state filename must match its launch ID",
        ));
    }
    let parent = state_file.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Antigravity hook state path has no parent directory",
        )
    })?;
    Ok(parent.join(format!("{stem}.lock")))
}

/// Bind the live PTY process identity `(pid, started_at)` to this launch's hook
/// token so stale state from earlier launches is never read as current.
pub fn record_launch(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    launch_id: &str,
    pid: Option<u32>,
    started_at: &str,
) -> io::Result<()> {
    if !valid_launch_id(launch_id) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid Antigravity hook launch ID",
        ));
    }
    let binding = LaunchBinding {
        v: STATE_SCHEMA_VERSION,
        group_id: group_id.to_owned(),
        actor_id: actor_id.to_owned(),
        launch_id: launch_id.to_owned(),
        pid,
        started_at: started_at.to_owned(),
    };
    crate::fs::write_json(&launch_file_path(home, group_id, actor_id), &binding)?;
    prune_superseded_launch_files(home, group_id, actor_id, launch_id);
    Ok(())
}

fn prune_superseded_launch_files(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    current_launch_id: &str,
) {
    let dir = actor_launches_dir(home, group_id, actor_id);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let keep_state = format!("{current_launch_id}{STATE_FILE_SUFFIX}");
    let keep_lock = format!("{current_launch_id}.lock");
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if name != keep_state
            && name != keep_lock
            && (name.ends_with(STATE_FILE_SUFFIX) || name.ends_with(".lock"))
        {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Read hook state only when it is bound to the exact running PTY launch and
/// has not lost exact tracking (`uncertain == false`).
#[must_use]
pub fn read_live_state(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    pid: Option<u32>,
    started_at: &str,
) -> Option<HookState> {
    if started_at.is_empty() {
        return None;
    }
    let binding: LaunchBinding =
        crate::fs::read_json(&launch_file_path(home, group_id, actor_id)).ok()?;
    if binding.v != STATE_SCHEMA_VERSION
        || binding.group_id != group_id
        || binding.actor_id != actor_id
        || !valid_launch_id(&binding.launch_id)
        || binding.pid != pid
        || binding.started_at != started_at
    {
        return None;
    }
    let state: HookState = crate::fs::read_json(&state_file_path(
        home,
        group_id,
        actor_id,
        &binding.launch_id,
    ))
    .ok()?;
    if state.v != STATE_SCHEMA_VERSION
        || state.group_id != group_id
        || state.actor_id != actor_id
        || state.launch_id != binding.launch_id
        || state.uncertain
        || !matches!(state.status.as_str(), "idle" | "working")
    {
        return None;
    }
    Some(state)
}

/// Revoke idle evidence for this launch when a hook payload could not be read
/// or parsed (for example when `lastUserInput` exceeds `MAX_HOOK_STDIN_BYTES`).
/// Scoped exclusively to `<launch_id>.state.json` so a late hook from a prior
/// launch can never revoke a newer launch.
pub fn revoke_idle(
    state_file: &Path,
    launch_id: &str,
    group_id: &str,
    actor_id: &str,
    reason: &str,
) -> io::Result<Option<HookState>> {
    if group_id.trim().is_empty() || actor_id.trim().is_empty() {
        return Ok(None);
    }
    let lock_file = lock_file_for(state_file, launch_id)?;
    crate::fs::with_exclusive_lock(&lock_file, || {
        let active_conversations = crate::fs::read_json::<HookState>(state_file)
            .ok()
            .filter(|existing| {
                existing.v == STATE_SCHEMA_VERSION
                    && existing.launch_id == launch_id
                    && existing.group_id == group_id
                    && existing.actor_id == actor_id
            })
            .map(|existing| existing.active_conversations)
            .unwrap_or_default();
        let next = HookState {
            v: STATE_SCHEMA_VERSION,
            group_id: group_id.to_owned(),
            actor_id: actor_id.to_owned(),
            launch_id: launch_id.to_owned(),
            status: "unknown".to_owned(),
            event: reason.to_owned(),
            active_conversations,
            uncertain: true,
            updated_at: utc_now(),
        };
        crate::fs::write_json(state_file, &next)?;
        Ok(Some(next))
    })
}

/// Apply one Antigravity lifecycle hook payload from stdin to this launch's
/// isolated state file. Returns `Ok(None)` when the event is ignored.
///
/// Note on `/new` / `/clear`: `SessionStart` never clears `active_conversations`
/// because subagents also emit `SessionStart` while the parent conversation is
/// still running. Normal turn endings, errors, and cancellations all emit
/// `Stop` with `fullyIdle: true` once background work finishes.
pub fn apply_hook_payload(
    state_file: &Path,
    launch_id: &str,
    group_id: &str,
    actor_id: &str,
    payload: &Value,
) -> io::Result<Option<HookState>> {
    if group_id.trim().is_empty() || actor_id.trim().is_empty() {
        return Ok(None);
    }
    let Some(event_name) = payload.get("hookEventName").and_then(Value::as_str) else {
        return Ok(None);
    };
    if !matches!(event_name, "SessionStart" | "PreInvocation" | "Stop") {
        return Ok(None);
    }
    let lock_file = lock_file_for(state_file, launch_id)?;
    let conversation_id = payload
        .get("conversationId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_CONVERSATION_KEY)
        .to_owned();
    // Protojson omits `false` booleans by default; only an explicit `true`
    // proves all background tasks and subagents have finished.
    let fully_idle = payload.get("fullyIdle").and_then(Value::as_bool) == Some(true);

    crate::fs::with_exclusive_lock(&lock_file, || {
        let existing = crate::fs::read_json::<HookState>(state_file)
            .ok()
            .filter(|existing| {
                existing.v == STATE_SCHEMA_VERSION
                    && existing.launch_id == launch_id
                    && existing.group_id == group_id
                    && existing.actor_id == actor_id
            });
        let mut uncertain = existing.as_ref().is_some_and(|state| state.uncertain);
        let mut active_conversations = existing
            .map(|state| state.active_conversations)
            .unwrap_or_default();

        let status = if uncertain {
            "unknown"
        } else {
            match event_name {
                "SessionStart" => {
                    if active_conversations.is_empty() {
                        "idle"
                    } else {
                        "working"
                    }
                }
                "PreInvocation" => {
                    if !active_conversations.contains(&conversation_id)
                        && active_conversations.len() >= MAX_ACTIVE_CONVERSATIONS
                    {
                        uncertain = true;
                        "unknown"
                    } else {
                        active_conversations.insert(conversation_id);
                        "working"
                    }
                }
                "Stop" => {
                    if fully_idle {
                        active_conversations.remove(&conversation_id);
                        if active_conversations.is_empty() {
                            "idle"
                        } else {
                            "working"
                        }
                    } else if !active_conversations.contains(&conversation_id)
                        && active_conversations.len() >= MAX_ACTIVE_CONVERSATIONS
                    {
                        uncertain = true;
                        "unknown"
                    } else {
                        active_conversations.insert(conversation_id);
                        "working"
                    }
                }
                _ => unreachable!("validated event name"),
            }
        };

        let next = HookState {
            v: STATE_SCHEMA_VERSION,
            group_id: group_id.to_owned(),
            actor_id: actor_id.to_owned(),
            launch_id: launch_id.to_owned(),
            status: status.to_owned(),
            event: event_name.to_owned(),
            active_conversations,
            uncertain,
            updated_at: utc_now(),
        };
        crate::fs::write_json(state_file, &next)?;
        Ok(Some(next))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_transitions_track_idle_working_and_protojson_fully_idle() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let group_id = "g_hook_test";
        let actor_id = "小助手";
        let launch_id = "launch-1";
        let state_file = state_file_path(&home, group_id, actor_id, launch_id);
        record_launch(
            &home,
            group_id,
            actor_id,
            launch_id,
            Some(42),
            "2026-11-25T00:00:00Z",
        )
        .expect("record launch");

        assert!(
            read_live_state(&home, group_id, actor_id, Some(42), "2026-11-25T00:00:00Z").is_none()
        );

        let started = apply_hook_payload(
            &state_file,
            launch_id,
            group_id,
            actor_id,
            &json!({"hookEventName":"SessionStart","conversationId":"main-1"}),
        )
        .expect("session start")
        .expect("state written");
        assert_eq!(started.status, "idle");

        let working = apply_hook_payload(
            &state_file,
            launch_id,
            group_id,
            actor_id,
            &json!({"hookEventName":"PreInvocation","conversationId":"main-1"}),
        )
        .expect("pre invocation")
        .expect("state written");
        assert_eq!(working.status, "working");

        // Protojson omits `false` booleans: Stop without `fullyIdle: true` stays working.
        let omitted_idle = apply_hook_payload(
            &state_file,
            launch_id,
            group_id,
            actor_id,
            &json!({"hookEventName":"Stop","conversationId":"main-1","terminationReason":"NO_TOOL_CALL"}),
        )
        .expect("stop omitted fullyIdle")
        .expect("state written");
        assert_eq!(omitted_idle.status, "working");

        let false_idle = apply_hook_payload(
            &state_file,
            launch_id,
            group_id,
            actor_id,
            &json!({"hookEventName":"Stop","conversationId":"main-1","fullyIdle":false}),
        )
        .expect("stop false fullyIdle")
        .expect("state written");
        assert_eq!(false_idle.status, "working");

        let idle = apply_hook_payload(
            &state_file,
            launch_id,
            group_id,
            actor_id,
            &json!({"hookEventName":"Stop","conversationId":"main-1","fullyIdle":true}),
        )
        .expect("stop fullyIdle")
        .expect("state written");
        assert_eq!(idle.status, "idle");

        let live = read_live_state(&home, group_id, actor_id, Some(42), "2026-11-25T00:00:00Z")
            .expect("live state");
        assert_eq!(live.status, "idle");
        assert_eq!(live.event, "Stop");
    }

    #[test]
    fn subagent_hooks_do_not_mark_parent_conversation_idle_early() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let state_file = state_file_path(&home, "g_sub", "project", "L1");
        record_launch(
            &home,
            "g_sub",
            "project",
            "L1",
            Some(100),
            "2026-11-25T00:00:00Z",
        )
        .expect("launch");

        apply_hook_payload(
            &state_file,
            "L1",
            "g_sub",
            "project",
            &json!({"hookEventName":"PreInvocation","conversationId":"parent"}),
        )
        .expect("parent start");

        let sub_start = apply_hook_payload(
            &state_file,
            "L1",
            "g_sub",
            "project",
            &json!({"hookEventName":"SessionStart","conversationId":"sub-1"}),
        )
        .expect("sub start")
        .expect("state");
        assert_eq!(sub_start.status, "working");

        apply_hook_payload(
            &state_file,
            "L1",
            "g_sub",
            "project",
            &json!({"hookEventName":"PreInvocation","conversationId":"sub-1"}),
        )
        .expect("sub invoke");

        let sub_stop = apply_hook_payload(
            &state_file,
            "L1",
            "g_sub",
            "project",
            &json!({"hookEventName":"Stop","conversationId":"sub-1","fullyIdle":true}),
        )
        .expect("sub stop")
        .expect("state");
        assert_eq!(sub_stop.status, "working");

        let parent_stop = apply_hook_payload(
            &state_file,
            "L1",
            "g_sub",
            "project",
            &json!({"hookEventName":"Stop","conversationId":"parent","fullyIdle":true}),
        )
        .expect("parent stop")
        .expect("state");
        assert_eq!(parent_stop.status, "idle");
    }

    #[test]
    fn new_launch_early_session_start_survives_prior_launch_binding_and_late_l1_hooks() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let l1_file = state_file_path(&home, "g_restart", "peer", "L1");
        let l2_file = state_file_path(&home, "g_restart", "peer", "L2");

        // Launch L1 is bound and idle.
        record_launch(
            &home,
            "g_restart",
            "peer",
            "L1",
            Some(10),
            "2026-11-25T00:00:01Z",
        )
        .expect("launch 1");
        apply_hook_payload(
            &l1_file,
            "L1",
            "g_restart",
            "peer",
            &json!({"hookEventName":"Stop","conversationId":"c1","fullyIdle":true}),
        )
        .expect("L1 idle");

        // Launch L2 spawns and fires SessionStart BEFORE record_launch(L2) replaces L1's binding.
        apply_hook_payload(
            &l2_file,
            "L2",
            "g_restart",
            "peer",
            &json!({"hookEventName":"SessionStart","conversationId":"c2"}),
        )
        .expect("L2 early session start")
        .expect("L2 state written");

        // Once L2's launch binding is recorded, L2's early SessionStart is immediately visible.
        record_launch(
            &home,
            "g_restart",
            "peer",
            "L2",
            Some(20),
            "2026-11-25T00:01:00Z",
        )
        .expect("launch 2");
        assert_eq!(
            read_live_state(&home, "g_restart", "peer", Some(20), "2026-11-25T00:01:00Z")
                .expect("L2 early idle")
                .status,
            "idle"
        );

        // L2 starts working; late Stop and late revoke_idle from L1 cannot affect L2.
        apply_hook_payload(
            &l2_file,
            "L2",
            "g_restart",
            "peer",
            &json!({"hookEventName":"PreInvocation","conversationId":"c2"}),
        )
        .expect("L2 working");
        apply_hook_payload(
            &l1_file,
            "L1",
            "g_restart",
            "peer",
            &json!({"hookEventName":"Stop","conversationId":"c1","fullyIdle":true}),
        )
        .expect("late L1 stop isolated");
        revoke_idle(&l1_file, "L1", "g_restart", "peer", "oversized_payload")
            .expect("late L1 revoke isolated");
        assert_eq!(
            read_live_state(&home, "g_restart", "peer", Some(20), "2026-11-25T00:01:00Z")
                .expect("L2 still working")
                .status,
            "working"
        );
    }

    #[test]
    fn revoke_idle_clears_stale_idle_evidence_and_cannot_be_undone_by_later_stop() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let state_file = state_file_path(&home, "g_revoke", "peer", "L1");
        record_launch(
            &home,
            "g_revoke",
            "peer",
            "L1",
            Some(77),
            "2026-11-25T00:00:00Z",
        )
        .expect("launch");
        apply_hook_payload(
            &state_file,
            "L1",
            "g_revoke",
            "peer",
            &json!({"hookEventName":"SessionStart","conversationId":"main"}),
        )
        .expect("idle");
        assert_eq!(
            read_live_state(&home, "g_revoke", "peer", Some(77), "2026-11-25T00:00:00Z")
                .expect("idle before revoke")
                .status,
            "idle"
        );

        revoke_idle(&state_file, "L1", "g_revoke", "peer", "oversized_payload")
            .expect("revoke idle");
        assert!(
            read_live_state(&home, "g_revoke", "peer", Some(77), "2026-11-25T00:00:00Z").is_none(),
            "revoked launch must not expose stale idle evidence"
        );

        // Even a subagent Stop(fullyIdle: true) must not flip an uncertain launch back to idle.
        apply_hook_payload(
            &state_file,
            "L1",
            "g_revoke",
            "peer",
            &json!({"hookEventName":"Stop","conversationId":"sub","fullyIdle":true}),
        )
        .expect("sub stop");
        assert!(
            read_live_state(&home, "g_revoke", "peer", Some(77), "2026-11-25T00:00:00Z").is_none()
        );
    }

    #[test]
    fn active_conversation_overflow_persists_uncertain_instead_of_dropping_65th() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let state_file = state_file_path(&home, "g_overflow", "peer", "L1");
        record_launch(
            &home,
            "g_overflow",
            "peer",
            "L1",
            Some(88),
            "2026-11-25T00:00:00Z",
        )
        .expect("launch");

        for i in 0..MAX_ACTIVE_CONVERSATIONS {
            apply_hook_payload(
                &state_file,
                "L1",
                "g_overflow",
                "peer",
                &json!({"hookEventName":"PreInvocation","conversationId":format!("c-{i}")}),
            )
            .expect("pre invocation");
        }
        assert_eq!(
            read_live_state(
                &home,
                "g_overflow",
                "peer",
                Some(88),
                "2026-11-25T00:00:00Z"
            )
            .expect("64 active")
            .status,
            "working"
        );

        // 65th concurrent active conversation overflows the tracked set.
        let overflowed = apply_hook_payload(
            &state_file,
            "L1",
            "g_overflow",
            "peer",
            &json!({"hookEventName":"PreInvocation","conversationId":"c-64"}),
        )
        .expect("65th pre invocation")
        .expect("state");
        assert!(overflowed.uncertain);

        // Stopping all first 64 conversations must NOT flip the launch to idle.
        for i in 0..MAX_ACTIVE_CONVERSATIONS {
            apply_hook_payload(
                &state_file,
                "L1",
                "g_overflow",
                "peer",
                &json!({"hookEventName":"Stop","conversationId":format!("c-{i}"),"fullyIdle":true}),
            )
            .expect("stop");
        }
        assert!(
            read_live_state(
                &home,
                "g_overflow",
                "peer",
                Some(88),
                "2026-11-25T00:00:00Z"
            )
            .is_none(),
            "overflowed launch must stay uncertain rather than falsely claiming idle"
        );
    }
}
