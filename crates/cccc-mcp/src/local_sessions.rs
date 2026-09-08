use cccc_contracts::RunnerKind;
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

#[derive(Clone)]
struct SessionOwner {
    home: PathBuf,
    group_id: String,
}

pub fn start(home: &HomeLayout, root: &Path, args: &Map<String, Value>) -> Result<Value, String> {
    let session_id = format!("s_{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
    let group_id = args
        .get("group_id")
        .and_then(Value::as_str)
        .ok_or("group_id is required")?
        .to_owned();
    let mut owned = sessions().lock().map_err(|_| "session lock poisoned")?;
    let status = cccc_runtime::start(cccc_runtime::LaunchSpec {
        group_id: group_id.clone(),
        actor_id: session_id.clone(),
        runner: RunnerKind::Headless,
        command: super::local_tools::command(args)?,
        cwd: root.into(),
        env: Default::default(),
        cols: 120,
        rows: 40,
    })
    .map_err(|error| error.to_string())?;
    owned.insert(
        session_id.clone(),
        SessionOwner {
            home: home.root().to_owned(),
            group_id,
        },
    );
    Ok(json!({"session_id":session_id,"status":status}))
}

pub fn write(home: &HomeLayout, args: &Map<String, Value>) -> Result<Value, String> {
    let (session_id, group_id) = session(home, args)?;
    if let Some(data) = args.get("chars").and_then(Value::as_str) {
        cccc_runtime::write(&group_id, &session_id, data.as_bytes())
            .map_err(|error| error.to_string())?;
    }
    if args
        .get("terminate")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        let status =
            cccc_runtime::stop(&group_id, &session_id).map_err(|error| error.to_string())?;
        remove_session(&session_id)?;
        return Ok(json!({"session_id":session_id,"status":status}));
    }
    payload(&group_id, &session_id)
}

fn payload(group_id: &str, session_id: &str) -> Result<Value, String> {
    let status = cccc_runtime::status(group_id, session_id).map_err(|error| error.to_string())?;
    let history = cccc_runtime::history(group_id, session_id, None, 2_000_000)
        .map_err(|error| error.to_string())?;
    if !status.running {
        cccc_runtime::stop(group_id, session_id).map_err(|error| error.to_string())?;
        remove_session(session_id)?;
    }
    Ok(
        json!({"session_id":session_id,"status":status,"output":history.data,"cursor":history.end_cursor}),
    )
}

fn remove_session(session_id: &str) -> Result<(), String> {
    sessions()
        .lock()
        .map_err(|_| "session lock poisoned")?
        .remove(session_id);
    Ok(())
}

fn session(home: &HomeLayout, args: &Map<String, Value>) -> Result<(String, String), String> {
    let id = args
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or("session_id is required")?
        .to_owned();
    let owner = sessions()
        .lock()
        .map_err(|_| "session lock poisoned")?
        .get(&id)
        .cloned()
        .ok_or("session not found")?;
    if owner.home != home.root() {
        return Err("session does not belong to the requested home".into());
    }
    let group = owner.group_id;
    if args
        .get("group_id")
        .and_then(Value::as_str)
        .is_some_and(|requested| requested != group)
    {
        return Err("session does not belong to the requested group".into());
    }
    Ok((id, group))
}

fn sessions() -> &'static Mutex<HashMap<String, SessionOwner>> {
    static SESSIONS: OnceLock<Mutex<HashMap<String, SessionOwner>>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The host drains requests before shutdown. Remove only this Home's local tools;
/// the same process can also contain a Voice Analyst or another host's sessions.
pub fn shutdown(home: &HomeLayout) -> Result<(), String> {
    let owned = {
        let mut sessions = sessions().lock().map_err(|_| "session lock poisoned")?;
        let ids = sessions
            .iter()
            .filter(|(_, owner)| owner.home == home.root())
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        ids.into_iter()
            .filter_map(|id| sessions.remove(&id).map(|owner| (id, owner)))
            .collect::<Vec<_>>()
    };
    let mut errors = Vec::new();
    for (id, owner) in owned {
        match cccc_runtime::stop(&owner.group_id, &id) {
            Ok(_) | Err(cccc_runtime::RuntimeError::NotFound(..)) => {}
            Err(error) => errors.push(error.to_string()),
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn run_isolated(name: &str) -> bool {
        const CASE_ENV: &str = "CCCC_MCP_LOCAL_SESSION_TEST";
        if std::env::var(CASE_ENV).as_deref() == Ok(name) {
            return false;
        }
        // Other MCP tests embed a daemon, whose shutdown intentionally stops
        // its process-wide Runtime registry. Test ownership in a separate host.
        let output = std::process::Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                &format!("local_sessions::tests::{name}"),
                "--nocapture",
            ])
            .env(CASE_ENV, name)
            .output()
            .expect("isolated ownership test");
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        true
    }

    #[tokio::test]
    async fn host_shutdown_releases_its_local_command_session() {
        if run_isolated("host_shutdown_releases_its_local_command_session") {
            return;
        }
        let temp = tempfile::tempdir().expect("tempdir");
        let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
        let group_id = format!("g_{}", uuid::Uuid::new_v4().simple());
        let args = json!({"group_id":group_id,"command":["sh","-c","sleep 60"]})
            .as_object()
            .cloned()
            .expect("args");
        let started = start(&home, temp.path(), &args).expect("start local command");
        let id = started["session_id"].as_str().expect("session id");
        crate::shutdown(&home).await;
        let still_running = cccc_runtime::status(&group_id, id).is_ok_and(|status| status.running);
        let _ = cccc_runtime::stop(&group_id, id);
        let _ = remove_session(id);
        assert!(
            !still_running,
            "MCP shutdown left its command session running"
        );
    }

    #[tokio::test]
    async fn shutdown_and_access_are_scoped_to_the_local_tool_owner() {
        if run_isolated("shutdown_and_access_are_scoped_to_the_local_tool_owner") {
            return;
        }
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home-one")).expect("home");
        let other = HomeLayout::from_path(temp.path().join("home-two")).expect("other home");
        let group_id = format!("g_{}", uuid::Uuid::new_v4().simple());
        let args = json!({"group_id":group_id,"command":["sh","-c","sleep 60"]})
            .as_object()
            .cloned()
            .expect("args");
        let first = start(&home, temp.path(), &args).expect("first tool");
        let second = start(&other, temp.path(), &args).expect("second tool");
        let first_id = first["session_id"].as_str().expect("first id");
        let second_id = second["session_id"].as_str().expect("second id");
        let unrelated = format!("actor_{}", uuid::Uuid::new_v4().simple());
        cccc_runtime::start(cccc_runtime::LaunchSpec {
            group_id: group_id.clone(),
            actor_id: unrelated.clone(),
            runner: RunnerKind::Headless,
            command: vec!["sh".into(), "-c".into(), "sleep 60".into()],
            cwd: temp.path().into(),
            env: Default::default(),
            cols: 80,
            rows: 24,
        })
        .expect("unrelated host runtime");
        let foreign = write(
            &other,
            &json!({"group_id":group_id,"session_id":first_id})
                .as_object()
                .cloned()
                .expect("args"),
        );
        crate::shutdown(&home).await;
        let first_gone = cccc_runtime::status(&group_id, first_id).is_err();
        let second_running =
            cccc_runtime::status(&group_id, second_id).is_ok_and(|status| status.running);
        let unrelated_running =
            cccc_runtime::status(&group_id, &unrelated).is_ok_and(|status| status.running);
        crate::shutdown(&other).await;
        let _ = cccc_runtime::stop(&group_id, first_id);
        let _ = cccc_runtime::stop(&group_id, &unrelated);
        assert!(
            foreign
                .expect_err("foreign Home cannot access a local session")
                .contains("requested home")
        );
        assert!(first_gone);
        assert!(
            second_running && unrelated_running,
            "shutdown stopped a different owner"
        );
    }

    #[tokio::test]
    async fn observing_command_exit_releases_the_runtime_but_returns_its_final_output() {
        if run_isolated("observing_command_exit_releases_the_runtime_but_returns_its_final_output")
        {
            return;
        }
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let group_id = format!("g_{}", uuid::Uuid::new_v4().simple());
        let args = json!({"group_id":group_id,"command":["sh","-c","printf local-finished"]})
            .as_object()
            .cloned()
            .expect("args");
        let started = start(&home, temp.path(), &args).expect("command");
        let id = started["session_id"].as_str().expect("session id");
        let query = json!({"group_id":group_id,"session_id":id})
            .as_object()
            .cloned()
            .expect("query");
        let final_output = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let output = write(&home, &query).expect("poll command");
                if output["status"]["running"] == false {
                    break output;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await;
        let removed = cccc_runtime::status(&group_id, id).is_err();
        crate::shutdown(&home).await;
        let output = final_output.expect("command finished");
        assert!(
            output["output"]
                .as_str()
                .is_some_and(|text| text.contains("local-finished"))
        );
        assert!(
            removed,
            "completed local command stayed in the runtime registry"
        );
        assert!(write(&home, &query).is_err());
    }
}
