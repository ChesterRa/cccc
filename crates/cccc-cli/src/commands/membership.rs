use anyhow::Result;
use cccc_client::DaemonClient;
use cccc_contracts::DaemonResponse;
use cccc_core::membership;
use serde_json::{Map, Value, json};
use std::time::Duration;

use super::common::{call, print};
use crate::args::ReachAction;

pub async fn login(client: &DaemonClient) -> Result<()> {
    let mut response = call(client, "membership_login", request_args()).await?;
    if !response.ok {
        return print(response);
    }
    let Some(pending) = membership_body(&response)
        .and_then(|body| body.get("pending"))
        .and_then(Value::as_object)
    else {
        print_membership_copy(&response);
        return print(response);
    };
    let origin = membership::account_origin().unwrap_or_default();
    if !origin.is_empty() {
        eprintln!("Account: {origin}");
    }
    let verification_uri_complete = text(pending, "verification_uri_complete");
    eprintln!(
        "Open: {}",
        if verification_uri_complete.is_empty() {
            text(pending, "verification_uri")
        } else {
            verification_uri_complete
        }
    );
    eprintln!("Code: {}", text(pending, "user_code"));
    let mut interval = integer(pending, "interval", 5).max(1);
    loop {
        tokio::time::sleep(Duration::from_secs(interval)).await;
        response = call(client, "membership_login_poll", request_args()).await?;
        if !response.ok {
            return print(response);
        }
        let Some(body) = membership_body(&response) else {
            return print(response);
        };
        if body
            .get("logged_in")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            print_membership_copy(&response);
            return print(response);
        }
        if let Some(pending) = body.get("pending").and_then(Value::as_object) {
            interval = integer(pending, "interval", interval).max(1);
        }
    }
}

pub async fn logout(client: &DaemonClient) -> Result<()> {
    let response = call(client, "membership_logout", request_args()).await?;
    if let Some(warning) = membership_body(&response)
        .and_then(|body| body.get("warning"))
        .and_then(Value::as_str)
    {
        eprintln!("{warning}");
    }
    print(response)
}

pub async fn reach(client: &DaemonClient, action: ReachAction) -> Result<()> {
    let request_client = if matches!(action, ReachAction::On | ReachAction::Install) {
        client.clone().with_timeout(Duration::from_secs(120))
    } else {
        client.clone()
    };
    let (op, args) = match action {
        ReachAction::On => ("membership_reach_on", request_args()),
        ReachAction::Off => ("membership_reach_off", request_args()),
        ReachAction::Status => ("membership_status", request_args()),
        ReachAction::Install => {
            let mut args = request_map();
            args.insert("upgrade".into(), Value::Bool(true));
            ("membership_reach_install", Value::Object(args))
        }
    };
    let response = call(&request_client, op, args).await?;
    if response.ok && !matches!(action, ReachAction::Install) {
        print_membership_copy(&response);
    }
    print(response)
}

fn request_args() -> Value {
    Value::Object(request_map())
}

fn request_map() -> Map<String, Value> {
    let mut args = json!({"by":"user"})
        .as_object()
        .cloned()
        .expect("membership request is an object");
    if let Some(origin) = membership::account_origin() {
        args.insert("account_origin".into(), Value::String(origin));
    }
    args
}

fn membership_body(response: &DaemonResponse) -> Option<&Map<String, Value>> {
    response.result.get("membership").and_then(Value::as_object)
}

fn print_membership_copy(response: &DaemonResponse) {
    let Some(body) = membership_body(response) else {
        return;
    };
    if body
        .get("logged_in")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        let state = if body.get("cut").and_then(Value::as_bool).unwrap_or(false) {
            "cut"
        } else if body.get("online").and_then(Value::as_bool).unwrap_or(false) {
            "online"
        } else {
            "linked, not published"
        };
        let origin = text(body, "account_origin");
        if origin.is_empty() {
            eprintln!("Remote access: {state}");
        } else {
            eprintln!("Remote access: {state}  account: {origin}");
        }
    }
    let hostname = text(body, "hostname");
    let web = text(body, "web_url");
    if hostname.is_empty() && web.is_empty() {
        return;
    }
    eprintln!(
        "Hostname (people / account page): {}",
        display_or_none(&hostname)
    );
    eprintln!(
        "Web (this machine, includes admin token): {}",
        display_or_none(&web)
    );
    eprintln!("The Web URL is a bearer credential; keep it private.");
    eprintln!("Web Model connectors are managed per actor in CCCC settings.");
}

fn display_or_none(value: &str) -> &str {
    if value.is_empty() { "(none)" } else { value }
}

fn text(values: &Map<String, Value>, key: &str) -> String {
    values
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .into()
}

fn integer(values: &Map<String, Value>, key: &str, default: u64) -> u64 {
    values
        .get(key)
        .and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
        })
        .unwrap_or(default)
}
