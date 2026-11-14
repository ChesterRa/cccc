use cccc_contracts::local_session::LocalSessionRuntime;
use cccc_core::{HomeLayout, local_sessions::NativeSession};
use serde_json::{Value, json};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::io::AsyncWriteExt;

pub async fn mcp_for(
    root: &Path,
    home: &HomeLayout,
    source: &NativeSession,
    name: &str,
    args: Value,
) -> Value {
    let mut command = super::fixture::command(root, home);
    command.args(["mcp", "--local-session-runtime", source.runtime.as_str()]);
    if source.runtime == LocalSessionRuntime::Claude {
        command.args(["--local-session-host-pid", &source.host.pid.to_string()]);
    }
    let mut process = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("real native MCP");
    process.stdin.take().expect("MCP input").write_all(format!("{}\n", json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args,"_meta":{"threadId":source.native_session_id}}})).as_bytes()).await.expect("MCP request");
    let output = tokio::time::timeout(Duration::from_secs(20), process.wait_with_output())
        .await
        .expect("MCP completion")
        .expect("MCP output");
    assert!(output.status.success(), "MCP process failed");
    serde_json::from_slice(&output.stdout).expect("MCP response")
}
