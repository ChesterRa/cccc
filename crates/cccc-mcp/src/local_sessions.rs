use cccc_contracts::RunnerKind;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

pub fn start(root: &Path, args: &Map<String, Value>) -> Result<Value, String> {
    let session_id = format!("s_{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
    let group_id = args
        .get("group_id")
        .and_then(Value::as_str)
        .ok_or("group_id is required")?
        .to_owned();
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
    sessions()
        .lock()
        .map_err(|_| "session lock poisoned")?
        .insert(session_id.clone(), group_id);
    Ok(json!({"session_id":session_id,"status":status}))
}

pub fn write(args: &Map<String, Value>) -> Result<Value, String> {
    let (session_id, group_id) = session(args)?;
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

fn session(args: &Map<String, Value>) -> Result<(String, String), String> {
    let id = args
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or("session_id is required")?
        .to_owned();
    let group = sessions()
        .lock()
        .map_err(|_| "session lock poisoned")?
        .get(&id)
        .cloned()
        .ok_or("session not found")?;
    if args
        .get("group_id")
        .and_then(Value::as_str)
        .is_some_and(|requested| requested != group)
    {
        return Err("session does not belong to the requested group".into());
    }
    Ok((id, group))
}

fn sessions() -> &'static Mutex<HashMap<String, String>> {
    static SESSIONS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}
