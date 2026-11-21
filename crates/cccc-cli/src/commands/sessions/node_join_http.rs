//! Keep enrollment credentials inside bounded same-origin POST bodies.
use anyhow::{Result, bail};
use reqwest::{Client, StatusCode, Url};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::time::Duration;
use tokio::time::Instant;

const MAX_RESPONSE: usize = 32 * 1024;

#[derive(Clone, Copy)]
pub(super) enum Operation {
    Create,
    Poll,
}

impl Operation {
    fn path(self) -> &'static str {
        match self {
            Self::Create => "/api/v1/session_nodes/join/requests",
            Self::Poll => "/api/v1/session_nodes/join/poll",
        }
    }

    fn timeout_error(self) -> &'static str {
        match self {
            Self::Create => {
                "Could not reach the device join service. Check the hub address and proxy settings, then retry"
            }
            Self::Poll => "Device approval expired. Run join again for a new request",
        }
    }
}

#[derive(Deserialize)]
struct Envelope<T> {
    ok: bool,
    result: Option<T>,
}

pub(super) async fn post<T: DeserializeOwned>(
    client: &Client,
    origin: &Url,
    op: Operation,
    body: &impl Serialize,
    deadline: Instant,
) -> Result<T> {
    let mut url = origin.clone();
    url.set_path(op.path());
    let mut delay = Duration::from_secs(1);
    loop {
        if Instant::now() >= deadline {
            bail!("{}", op.timeout_error());
        }
        let attempt = tokio::time::timeout_at(deadline, once(client, &url, op, body)).await;
        if Instant::now() >= deadline {
            bail!("{}", op.timeout_error());
        }
        match attempt {
            Ok(Ok(Some(result))) => return Ok(result),
            Ok(Err(error)) => return Err(error),
            Err(_) => bail!("{}", op.timeout_error()),
            Ok(Ok(None)) => {}
        }
        if delay == Duration::from_secs(1) {
            eprintln!(
                "Hub temporarily unavailable. Retrying this device request; press Ctrl-C to cancel."
            );
        }
        tokio::time::sleep_until((Instant::now() + delay).min(deadline)).await;
        delay = (delay * 2).min(Duration::from_secs(8));
    }
}

async fn once<T: DeserializeOwned>(
    client: &Client,
    url: &Url,
    op: Operation,
    body: &impl Serialize,
) -> Result<Option<T>> {
    let mut response = match client.post(url.clone()).json(body).send().await {
        Ok(response) => response,
        Err(_) => return Ok(None),
    };
    let status = response.status();
    if status.is_redirection() {
        bail!("The hub redirected the device join request. Use its final trusted HTTPS origin");
    }
    match status {
        StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED | StatusCode::NOT_IMPLEMENTED => {
            if matches!(op, Operation::Poll) && status == StatusCode::NOT_FOUND {
                bail!("Device approval is no longer available. Run join again for a new request");
            }
            bail!(
                "This hub does not support device join. Update the hub or use sessions node connect --ticket-stdin"
            );
        }
        StatusCode::GONE => bail!("Device approval expired. Run join again for a new request"),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            bail!("The hub denied this device request. Contact its administrator")
        }
        StatusCode::TOO_MANY_REQUESTS | StatusCode::REQUEST_TIMEOUT => return Ok(None),
        s if s.is_server_error() => return Ok(None),
        s if !s.is_success() => bail!(
            "The hub refused the device join request (HTTP {})",
            s.as_u16()
        ),
        _ => {}
    }
    let invalid = || {
        anyhow::anyhow!(
            "Invalid device join response. Verify the hub URL, update the hub or use sessions node connect --ticket-stdin"
        )
    };
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE as u64)
    {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) if chunk.len() <= MAX_RESPONSE - bytes.len() => {
                bytes.extend_from_slice(&chunk)
            }
            Ok(Some(_)) => return Err(invalid()),
            Ok(None) => break,
            Err(_) => return Ok(None),
        }
    }
    let reply: Envelope<T> = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    match (reply.ok, reply.result) {
        (true, Some(value)) => Ok(Some(value)),
        _ => Err(invalid()),
    }
}
