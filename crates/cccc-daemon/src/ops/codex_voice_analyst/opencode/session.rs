use super::AcpClient;
use serde_json::{Value, json};
use std::io;
use std::path::Path;
use std::time::Duration;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) async fn initialize(
    protocol: &AcpClient,
    cwd: &Path,
    resume_session_id: Option<&str>,
    mcp_server: Value,
    model: Option<&str>,
    agent: Option<&str>,
) -> io::Result<(String, bool)> {
    let cwd = cwd.to_string_lossy().into_owned();
    let session_params = |session_id: Option<&str>| {
        let mut params = json!({"cwd":cwd,"mcpServers":[mcp_server.clone()]});
        if let Some(session_id) = session_id {
            params["sessionId"] = json!(session_id);
        }
        params
    };
    let (session_id, resumed) =
        if let Some(session_id) = resume_session_id.map(str::trim).filter(|id| !id.is_empty()) {
            match protocol
                .request(
                    "session/load",
                    session_params(Some(session_id)),
                    HANDSHAKE_TIMEOUT,
                )
                .await
            {
                Ok(_) => (session_id.to_owned(), true),
                Err(error) => return Err(error),
            }
        } else {
            (new_session(protocol, session_params(None)).await?, false)
        };
    if let Some(model) = model.map(str::trim).filter(|value| !value.is_empty()) {
        set_option(protocol, &session_id, "model", model).await?;
    }
    if let Some(agent) = agent.map(str::trim).filter(|value| !value.is_empty()) {
        set_option(protocol, &session_id, "mode", agent).await?;
    }
    Ok((session_id, resumed))
}

async fn new_session(protocol: &AcpClient, params: Value) -> io::Result<String> {
    let result = protocol
        .request("session/new", params, HANDSHAKE_TIMEOUT)
        .await?;
    result
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| io::Error::other("OpenCode ACP returned an empty session id"))
}

async fn set_option(
    protocol: &AcpClient,
    session_id: &str,
    config_id: &str,
    value: &str,
) -> io::Result<()> {
    protocol
        .request(
            "session/set_config_option",
            json!({"sessionId":session_id,"configId":config_id,"value":value}),
            HANDSHAKE_TIMEOUT,
        )
        .await
        .map(|_| ())
}

#[cfg(all(test, unix))]
mod tests {
    use super::super::super::{
        acp::{PermissionPolicy, PromptCompletion},
        process,
    };
    use super::*;
    use std::collections::BTreeMap;

    #[tokio::test]
    async fn rejected_resume_never_creates_a_replacement_opencode_or_kilo_session() {
        for runtime in ["opencode", "kilo"] {
            let temp = tempfile::tempdir().expect("isolated fixture");
            let script = r#"import json,sys
from pathlib import Path
for line in sys.stdin:
 r=json.loads(line)
 with open(Path.cwd()/'requests.jsonl','a') as f: f.write(json.dumps(r)+'\n')
 reply={'error':{'code':-32602,'message':'saved session unavailable'}} if r['method']=='session/load' else {'result':{'sessionId':'unwanted-new-session'}}
 print(json.dumps({'jsonrpc':'2.0','id':r['id'],**reply}),flush=True)
"#;
            let (owner, stdin, stdout) = process::spawn_piped(
                &["python3".into(), "-u".into(), "-c".into(), script.into()],
                temp.path(),
                &BTreeMap::new(),
                "resume-fixture",
            )
            .expect("fixture process");
            let protocol = AcpClient::new(
                stdin,
                stdout,
                "resume-rejected".into(),
                runtime,
                PermissionPolicy::Reject,
                PromptCompletion::SessionEvents,
            )
            .expect("protocol");
            let result = initialize(
                &protocol,
                temp.path(),
                Some("ses_retained"),
                json!({}),
                None,
                None,
            )
            .await;
            protocol.close().await;
            owner.stop().expect("cleanup");
            assert!(result.is_err());
            let requests: Vec<Value> = std::fs::read_to_string(temp.path().join("requests.jsonl"))
                .expect("requests")
                .lines()
                .map(|line| serde_json::from_str(line).expect("JSON"))
                .collect();
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0]["method"], "session/load");
            assert_eq!(requests[0]["params"]["sessionId"], "ses_retained");
        }
    }
}
