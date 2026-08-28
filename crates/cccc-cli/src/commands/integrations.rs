use anyhow::{Context, Result};
use cccc_client::DaemonClient;
use cccc_core::HomeLayout;
use cccc_core::access_tokens::AccessTokenStore;
use reqwest::Method;
use serde_json::{Value, json};

use crate::args::{
    ImAction, ImArgs, ImSetArgs, PromptArgs, SpaceAction, SpaceArgs, SpaceAuthAction,
    SpaceCredentialAction, SpaceJobsAction,
};
use crate::commands::common::{call, group, print};

pub async fn prompt(client: &DaemonClient, home: &HomeLayout, args: PromptArgs) -> Result<()> {
    let actor_id = match (args.actor_id, args.legacy_actor_id) {
        (Some(actor_id), None) | (None, Some(actor_id)) if !actor_id.trim().is_empty() => actor_id,
        (Some(_), Some(_)) => anyhow::bail!("pass actor id once, preferably with --actor-id"),
        _ => anyhow::bail!("--actor-id is required"),
    };
    print(
        call(
            client,
            "actor_prompt",
            json!({"group_id":group(home,args.group_id)?,"actor_id":actor_id}),
        )
        .await?,
    )
}

pub async fn im(
    client: &DaemonClient,
    home: &HomeLayout,
    endpoint: &str,
    args: ImArgs,
) -> Result<()> {
    if let ImAction::Logs {
        group_id,
        lines,
        follow,
    } = args.action
    {
        return im_logs(client, home, group_id, lines, follow).await;
    }
    let (method, path, value) = match args.action {
        ImAction::Set(args) => {
            let ImSetArgs {
                platform,
                group_id,
                token_env,
                bot_token_env,
                app_token_env,
                app_key_env,
                app_secret_env,
                domain,
                robot_code_env,
                robot_code,
                wecom_bot_id,
                wecom_secret,
                weixin_account_id,
                token,
            } = *args;
            (
                Method::POST,
                "/api/im/set",
                json!({
                    "group_id":group(home,group_id)?,"platform":platform,"token_env":token_env,
                    "bot_token_env":bot_token_env,"app_token_env":app_token_env,
                    "app_key_env":app_key_env,"app_secret_env":app_secret_env,"domain":domain,
                    "robot_code_env":robot_code_env,"robot_code":robot_code,"wecom_bot_id":wecom_bot_id,
                    "wecom_secret":wecom_secret,"weixin_account_id":weixin_account_id,"token":token
                }),
            )
        }
        ImAction::Unset { group_id } => {
            (Method::POST, "/api/im/unset", group_value(home, group_id)?)
        }
        ImAction::Config { group_id } => {
            (Method::GET, "/api/im/config", group_value(home, group_id)?)
        }
        ImAction::Start { group_id } => {
            (Method::POST, "/api/im/start", group_value(home, group_id)?)
        }
        ImAction::Stop { group_id } => (Method::POST, "/api/im/stop", group_value(home, group_id)?),
        ImAction::Status { group_id } => {
            (Method::GET, "/api/im/status", group_value(home, group_id)?)
        }
        ImAction::Bind { key, group_id } => (
            Method::POST,
            "/api/im/bind",
            json!({"group_id":group(home,group_id)?,"key":key}),
        ),
        ImAction::Pending { group_id } => {
            (Method::GET, "/api/im/pending", group_value(home, group_id)?)
        }
        ImAction::Authorized { group_id } => (
            Method::GET,
            "/api/im/authorized",
            group_value(home, group_id)?,
        ),
        ImAction::Reject { key, group_id } => (
            Method::POST,
            "/api/im/pending/reject",
            json!({"group_id":group(home,group_id)?,"key":key}),
        ),
        ImAction::Revoke {
            chat_id,
            thread_id,
            group_id,
        } => (
            Method::POST,
            "/api/im/revoke",
            json!({"group_id":group(home,group_id)?,"chat_id":chat_id,"thread_id":thread_id}),
        ),
        ImAction::Logs { .. } => unreachable!("logs handled above"),
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&web_call(home, endpoint, method, path, value).await?)?
    );
    Ok(())
}

async fn im_logs(
    client: &DaemonClient,
    home: &HomeLayout,
    group_id: Option<String>,
    lines: usize,
    follow: bool,
) -> Result<()> {
    let group_id = group(home, group_id)?;
    let read = || {
        call(
            client,
            "debug_tail_logs",
            json!({"component":"im","group_id":group_id,"lines":lines,"by":"user"}),
        )
    };
    if !follow {
        return print(read().await?);
    }
    let mut previous = Vec::<String>::new();
    loop {
        let response = read().await?;
        if !response.ok {
            return print(response);
        }
        let current = response.result["lines"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let overlap = (0..=previous.len().min(current.len()))
            .rev()
            .find(|size| previous[previous.len() - size..] == current[..*size])
            .unwrap_or(0);
        for line in &current[overlap..] {
            println!("{line}");
        }
        previous = current;
        tokio::select! {
            result=tokio::signal::ctrl_c()=>{ result?; return Ok(()); }
            ()=tokio::time::sleep(std::time::Duration::from_secs(1))=>{}
        }
    }
}

async fn web_call(
    home: &HomeLayout,
    endpoint: &str,
    method: Method,
    path: &str,
    value: Value,
) -> Result<Value> {
    let client = reqwest::Client::new();
    let mut request = client.request(method.clone(), format!("{endpoint}{path}"));
    if let Some(token) = AccessTokenStore::new(home.clone())?
        .list()?
        .into_iter()
        .find(|token| token.is_admin)
    {
        request = request.bearer_auth(token.token);
    }
    request = if uses_query(&method, path) {
        request.query(&value)
    } else {
        request.json(&value)
    };
    let response = request
        .send()
        .await
        .with_context(|| format!("CCCC Web is not reachable at {endpoint}; run `cccc` first"))?;
    let status = response.status();
    let body: Value = response
        .json()
        .await
        .context("invalid response from CCCC Web")?;
    if status.is_success() && body.get("ok").and_then(Value::as_bool) == Some(true) {
        return Ok(body.get("result").cloned().unwrap_or(Value::Null));
    }
    let message = body
        .pointer("/error/message")
        .and_then(Value::as_str)
        .unwrap_or("CCCC Web rejected the operation");
    anyhow::bail!("{message} ({status})")
}

fn uses_query(method: &Method, path: &str) -> bool {
    *method == Method::GET || matches!(path, "/api/im/revoke" | "/api/im/verbose")
}

pub async fn space(
    client: &DaemonClient,
    home: &HomeLayout,
    endpoint: &str,
    args: SpaceArgs,
) -> Result<()> {
    if let SpaceAction::Auth { action } = &args.action {
        let (method, path, value) = space_auth_request(action)?;
        println!(
            "{}",
            serde_json::to_string_pretty(&web_call(home, endpoint, method, &path, value).await?)?,
        );
        return Ok(());
    }
    let (op, value) = match args.action {
        SpaceAction::Status { group_id, provider } => (
            "group_space_status",
            json!({"group_id":group(home,group_id)?,"provider":provider}),
        ),
        SpaceAction::Bind {
            remote_space_id,
            group_id,
            lane,
            provider,
            by,
        } => (
            "group_space_bind",
            json!({"group_id":group(home,group_id)?,"provider":provider,"lane":lane,"remote_space_id":remote_space_id,"action":"bind","by":by}),
        ),
        SpaceAction::Unbind {
            group_id,
            lane,
            provider,
            by,
        } => (
            "group_space_bind",
            json!({"group_id":group(home,group_id)?,"provider":provider,"lane":lane,"remote_space_id":"","action":"unbind","by":by}),
        ),
        SpaceAction::Ingest {
            group_id,
            lane,
            kind,
            payload,
            idempotency_key,
            provider,
            by,
        } => (
            "group_space_ingest",
            json!({"group_id":group(home,group_id)?,"lane":lane,"kind":kind,"payload":json_object(&payload,"payload")?,"idempotency_key":idempotency_key,"provider":provider,"by":by}),
        ),
        SpaceAction::Query {
            query,
            group_id,
            lane,
            options,
            provider,
        } => (
            "group_space_query",
            json!({"group_id":group(home,group_id)?,"lane":lane,"query":query,"options":json_object(&options,"options")?,"provider":provider}),
        ),
        SpaceAction::Sources {
            group_id,
            lane,
            action,
            source_id,
            new_title,
            provider,
        } => (
            "group_space_sources",
            json!({"group_id":group(home,group_id)?,"lane":lane,"action":action,"source_id":source_id,"new_title":new_title,"provider":provider}),
        ),
        SpaceAction::Jobs { action } => match action {
            SpaceJobsAction::List {
                group_id,
                lane,
                provider,
                state,
                limit,
            } => (
                "group_space_jobs",
                json!({"group_id":group(home,group_id)?,"lane":lane,"action":"list","state":state,"limit":limit,"provider":provider}),
            ),
            SpaceJobsAction::Retry {
                job_id,
                group_id,
                lane,
                provider,
                by,
            } => (
                "group_space_jobs",
                json!({"group_id":group(home,group_id)?,"lane":lane,"action":"retry","job_id":job_id,"provider":provider,"by":by}),
            ),
            SpaceJobsAction::Cancel {
                job_id,
                group_id,
                lane,
                provider,
                by,
            } => (
                "group_space_jobs",
                json!({"group_id":group(home,group_id)?,"lane":lane,"action":"cancel","job_id":job_id,"provider":provider,"by":by}),
            ),
        },
        SpaceAction::Auth { .. } => unreachable!("provider auth handled through Web"),
        SpaceAction::Credential { action } => match action {
            SpaceCredentialAction::Status { provider, by } => (
                "group_space_provider_credential_status",
                json!({"provider":provider,"by":by}),
            ),
            SpaceCredentialAction::Set {
                provider,
                auth_json,
                auth_json_file,
                by,
            } => {
                let raw = match (auth_json, auth_json_file) {
                    (Some(value), None) => value,
                    (None, Some(path)) => std::fs::read_to_string(&path)
                        .with_context(|| format!("failed to read {path}"))?,
                    (None, None) => anyhow::bail!("--auth-json or --auth-json-file is required"),
                    (Some(_), Some(_)) => unreachable!("clap conflicts_with"),
                };
                let normalized = serde_json::to_string(&json_object(&raw, "auth_json")?)?;
                (
                    "group_space_provider_credential_update",
                    json!({"provider":provider,"by":by,"auth_json":normalized,"clear":false}),
                )
            }
            SpaceCredentialAction::Clear { provider, by } => (
                "group_space_provider_credential_update",
                json!({"provider":provider,"by":by,"auth_json":"","clear":true}),
            ),
        },
        SpaceAction::Health { provider, by } => (
            "group_space_provider_health_check",
            json!({"provider":provider,"by":by}),
        ),
    };
    print(call(client, op, value).await?)
}

fn space_auth_request(action: &SpaceAuthAction) -> Result<(Method, String, Value)> {
    let (action_name, provider, by, timeout_seconds, force_reauth) = match action {
        SpaceAuthAction::Status { provider, by } => ("status", provider, by, None, false),
        SpaceAuthAction::Start {
            provider,
            by,
            timeout_seconds,
            force_reauth,
        } => ("start", provider, by, Some(*timeout_seconds), *force_reauth),
        SpaceAuthAction::Cancel { provider, by } => ("cancel", provider, by, None, false),
        SpaceAuthAction::Disconnect { provider, by } => ("disconnect", provider, by, None, false),
    };
    if provider.is_empty()
        || !provider
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        anyhow::bail!("provider must contain only letters, numbers, '_' or '-'");
    }
    let method = if action_name == "status" {
        Method::GET
    } else {
        Method::POST
    };
    Ok((
        method,
        format!("/api/v1/space/providers/{provider}/auth"),
        json!({"action":action_name,"by":by,"timeout_seconds":timeout_seconds,"force_reauth":force_reauth}),
    ))
}

fn group_value(home: &HomeLayout, group_id: Option<String>) -> Result<Value> {
    Ok(json!({"group_id":group(home,group_id)?}))
}

fn json_object(value: &str, name: &str) -> Result<Value> {
    let parsed: Value =
        serde_json::from_str(value).with_context(|| format!("invalid {name} JSON"))?;
    if !parsed.is_object() {
        anyhow::bail!("{name} must be a JSON object");
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revoke_and_get_requests_use_query_parameters() {
        assert!(uses_query(&Method::GET, "/api/im/status"));
        assert!(uses_query(&Method::POST, "/api/im/revoke"));
        assert!(!uses_query(&Method::POST, "/api/im/start"));
    }

    #[test]
    fn space_auth_uses_the_real_web_lifecycle_route() {
        let action = SpaceAuthAction::Start {
            provider: "notebooklm".into(),
            by: "operator".into(),
            timeout_seconds: 120,
            force_reauth: true,
        };
        let (method, path, body) = space_auth_request(&action).expect("request");
        assert_eq!(method, Method::POST);
        assert_eq!(path, "/api/v1/space/providers/notebooklm/auth");
        assert_eq!(body["action"], "start");
        assert_eq!(body["by"], "operator");
        assert_eq!(body["timeout_seconds"], 120);
        assert_eq!(body["force_reauth"], true);
        let invalid = SpaceAuthAction::Status {
            provider: "../escape".into(),
            by: "user".into(),
        };
        assert!(space_auth_request(&invalid).is_err());
    }
}
