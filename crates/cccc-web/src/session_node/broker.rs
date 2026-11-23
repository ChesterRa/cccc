use super::protocol::{self, Request};
use cccc_client::session_node::{self as framing, ToolRequest};
use cccc_core::{HomeLayout, local_sessions, session_nodes};
use iroh::endpoint::Connection;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::{
    net::TcpListener,
    sync::{RwLock, Semaphore},
    task::JoinSet,
};

pub(super) type Link = Arc<RwLock<Option<Connection>>>;

pub(super) async fn serve(home: HomeLayout, secret: String, listener: TcpListener, link: Link) {
    let permits = Arc::new(Semaphore::new(8));
    let mut calls = JoinSet::new();
    loop {
        tokio::select! {
            Some(_) = calls.join_next(), if !calls.is_empty() => {},
            incoming = listener.accept() => {
                let Ok((mut stream, peer)) = incoming else { break; };
                if !peer.ip().is_loopback() { continue; }
                let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else { continue; };
                let home = home.clone(); let secret = secret.clone(); let link = Arc::clone(&link);
                calls.spawn(async move {
                    let _permit = permit;
                    let request = match tokio::time::timeout(Duration::from_secs(5), framing::read::<ToolRequest>(&mut stream)).await { Ok(Ok(request)) => request, _ => return };
                    let id = request.request["id"].clone();
                    let response = execute(&home, &secret, &link, request).await.unwrap_or_else(|_| json!({"jsonrpc":"2.0","id":id,"result":{"isError":true,"content":[{"type":"text","text":"Remote tool was rejected or its outcome is unknown. Check the Group state before repeating a write."}]}}));
                    let _ = tokio::time::timeout(Duration::from_secs(5), framing::write(&mut stream, &response)).await;
                });
            }
        }
    }
}

async fn execute(
    home: &HomeLayout,
    secret: &str,
    link: &Link,
    request: ToolRequest,
) -> anyhow::Result<Value> {
    anyhow::ensure!(
        request.secret == secret && request.request["method"] == "tools/call",
        "Invalid local connector request"
    );
    let client = session_nodes::client(home)?
        .filter(|client| client.secret == secret)
        .ok_or_else(|| anyhow::anyhow!("Connector unavailable"))?;
    local_sessions::validate_source(home, &request.source_id, &request.revision)?;
    let group_id = request.request["params"]["arguments"]["group_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Explicit Group required"))?;
    anyhow::ensure!(
        client
            .memberships
            .iter()
            .any(|member| member.source_id == request.source_id
                && member.source_revision == request.revision
                && member.group_id == group_id
                && member.enabled),
        "Remote membership unavailable"
    );
    let connection = link
        .read()
        .await
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Connector offline"))?;
    protocol::call(
        &connection,
        &Request::Tool {
            source_id: request.source_id,
            revision: request.revision,
            request: request.request,
        },
    )
    .await
}
