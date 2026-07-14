mod actions;
mod local_sessions;
mod local_tools;
mod mapping;
mod repo;
mod router;
mod schemas;
mod tools;

#[cfg(test)]
mod repo_tests;

use anyhow::Result;
use cccc_client::DaemonClient;
use cccc_core::HomeLayout;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub async fn run_stdio(home: HomeLayout) -> Result<()> {
    let client = DaemonClient::new(home.clone());
    let mut input = BufReader::new(tokio::io::stdin()).lines();
    let mut output = tokio::io::stdout();
    while let Some(line) = input.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(error) => {
                write_response(&mut output, &json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":error.to_string()}})).await?;
                continue;
            }
        };
        if request.get("id").is_none() {
            continue;
        }
        let response = handle(&home, &client, &request).await;
        write_response(&mut output, &response).await?;
    }
    Ok(())
}

pub async fn handle_request(home: &HomeLayout, request: &Value) -> Value {
    let client = DaemonClient::new(home.clone());
    handle(home, &client, request).await
}

async fn handle(home: &HomeLayout, client: &DaemonClient, request: &Value) -> Value {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {"tools": {"listChanged": false}},
            "serverInfo": {"name": "cccc-mcp", "version": env!("CARGO_PKG_VERSION")},
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tools::catalog()})),
        "tools/call" => {
            let params = request.get("params").and_then(Value::as_object);
            let name = params
                .and_then(|value| value.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let arguments = params
                .and_then(|value| value.get("arguments"))
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            router::call(home, client, name, arguments).await
        }
        _ => Err(format!("unknown method: {method}")),
    };
    match result {
        Ok(value) => json!({"jsonrpc":"2.0","id":id,"result":value}),
        Err(message) => json!({"jsonrpc":"2.0","id":id,"error":{"code":-32602,"message":message}}),
    }
}

async fn write_response(output: &mut tokio::io::Stdout, response: &Value) -> Result<()> {
    let mut bytes = serde_json::to_vec(response)?;
    bytes.push(b'\n');
    output.write_all(&bytes).await?;
    output.flush().await?;
    Ok(())
}
