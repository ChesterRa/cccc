//! Shared browsers the session owner keeps for this Web, reached through the
//! daemon. The owner keeps the browser, its profile and its display across Web
//! restarts; this Web keeps only its CDP connection and records which pages
//! its surfaces use, so that the next Web takes those pages back.

use anyhow::{Context, Result, anyhow};
use cccc_client::DaemonClient;
use cccc_contracts::DaemonRequest;
use cccc_daemon::session_owner::browser::{
    BrowserConnection, BrowserLaunchSpec, BrowserMode, BrowserPage, BrowserResource,
};
use serde_json::{Map, Value, json};
use std::time::{Duration, Instant};

/// Another member's start holds the daemon's start permission for a few
/// seconds; the daemon answers busy instead of queueing the browser behind it.
const START_BUSY_WAIT: Duration = Duration::from_secs(15);
const START_BUSY_RETRY: Duration = Duration::from_millis(250);

pub(crate) struct OwnerResources {
    client: DaemonClient,
}

/// The browser an adopted connection belongs to.
#[derive(Clone, Debug)]
pub(crate) struct HeldResource {
    pub resource_id: String,
    pub instance: String,
    /// What the owner started it with; connecting again asks for the same.
    pub spec: BrowserLaunchSpec,
    pub vnc_port: Option<u16>,
}

impl HeldResource {
    /// Whether it draws on a display rather than headless.
    pub fn system(&self) -> bool {
        matches!(self.spec.mode, BrowserMode::System { .. })
    }
}

/// A request the daemon refused, with its stable code.
#[derive(Debug)]
pub(crate) struct Refused {
    pub code: String,
    pub message: String,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{} ({})", self.message, self.code)
    }
}

impl std::error::Error for Refused {}

/// Whether `error` is the daemon refusing with `code`.
fn refused_with(error: &anyhow::Error, code: &str) -> bool {
    error
        .downcast_ref::<Refused>()
        .is_some_and(|refused| refused.code == code)
}

impl OwnerResources {
    pub fn new(client: DaemonClient) -> Self {
        Self { client }
    }

    /// The running browser for this profile, started by the owner when it
    /// keeps none. Never falls back to a browser this Web starts itself.
    pub async fn ensure(
        &self,
        spec: &BrowserLaunchSpec,
    ) -> Result<(BrowserResource, BrowserConnection)> {
        let args = serde_json::to_value(spec).context("encode the browser launch")?;
        let deadline = Instant::now() + START_BUSY_WAIT;
        let mut result = loop {
            match self.call("browser_resource_ensure", args.clone()).await {
                Err(error) if refused_with(&error, "runtime_busy") && Instant::now() < deadline => {
                    tokio::time::sleep(START_BUSY_RETRY).await;
                }
                result => break result?,
            }
        };
        // Decode errors can quote values, and the endpoint grants control of
        // the browser: keep them out of messages that reach a client.
        let mut resource: BrowserResource = result
            .remove("resource")
            .and_then(|value| serde_json::from_value(value).ok())
            .ok_or_else(|| anyhow!("the daemon returned no usable browser resource"))?;
        let connection = resource
            .connection
            .take()
            .ok_or_else(|| anyhow!("the session owner returned a browser that is not ready"))?;
        Ok((resource, connection))
    }

    /// Replaces the owner's record of the pages this Web uses in `held`.
    pub async fn set_pages(&self, held: &HeldResource, pages: Vec<BrowserPage>) -> Result<()> {
        self.call(
            "browser_resource_pages_set",
            json!({"resource_id":held.resource_id,"instance":held.instance,"sessions":pages}),
        )
        .await
        .map(drop)
    }

    async fn call(&self, op: &str, args: Value) -> Result<Map<String, Value>> {
        let Value::Object(mut args) = args else {
            return Err(anyhow!("{op} arguments are not an object"));
        };
        // A private operation of the Web itself, never a member's request.
        args.insert("by".into(), json!("user"));
        let response = self
            .client
            .call(&DaemonRequest {
                v: 1,
                op: op.into(),
                args,
            })
            .await
            .with_context(|| format!("reach the daemon for {op}"))?;
        if response.ok {
            return Ok(response.result);
        }
        Err(response.error.map_or_else(
            || anyhow!("{op} failed without an error"),
            |error| {
                Refused {
                    code: error.code,
                    message: error.message,
                }
                .into()
            },
        ))
    }
}
