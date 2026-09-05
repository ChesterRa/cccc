use super::*;

// Exercises the real OpenCode ACP/SSE bridge with isolated local models and
// noReply submissions (the same committed user-message shape as native TUI).
// No provider credentials or paid inference are required.
#[tokio::test]
async fn live_opencode_submitted_model_reaches_acp_when_enabled() {
    if std::env::var("CCCC_OPENCODE_MODEL_SYNC_LIVE").as_deref() != Ok("1") {
        return;
    }
    submitted_model_reaches_acp("opencode", "OPENCODE").await;
}

#[tokio::test]
async fn live_kilo_submitted_model_reaches_acp_when_enabled() {
    if std::env::var("CCCC_KILO_MODEL_SYNC_LIVE").as_deref() != Ok("1") {
        return;
    }
    submitted_model_reaches_acp("kilo", "KILO").await;
}

async fn submitted_model_reaches_acp(runtime: &'static str, prefix: &str) {
    let temp = tempfile::tempdir().expect("isolated home");
    let root = temp.path();
    let port = lifecycle::reserve_loopback_port().expect("port");
    let endpoint = format!("http://127.0.0.1:{port}");
    let password = uuid::Uuid::new_v4().to_string();
    let executable =
        std::env::var(format!("CCCC_{prefix}_EXECUTABLE")).unwrap_or_else(|_| runtime.into());
    let environment = BTreeMap::from([
        ("HOME".into(), root.to_string_lossy().into_owned()),
        ("XDG_CONFIG_HOME".into(), root.join("config").to_string_lossy().into_owned()),
        ("XDG_DATA_HOME".into(), root.join("data").to_string_lossy().into_owned()),
        ("XDG_STATE_HOME".into(), root.join("state").to_string_lossy().into_owned()),
        ("XDG_CACHE_HOME".into(), root.join("cache").to_string_lossy().into_owned()),
        (format!("{prefix}_DB"), root.join("provider.db").to_string_lossy().into_owned()),
        (format!("{prefix}_SERVER_USERNAME"), runtime.into()),
        (format!("{prefix}_SERVER_PASSWORD"), password.clone()),
        (format!("{prefix}_NO_DAEMON"), "1".into()),
        (format!("{prefix}_CONFIG_CONTENT"), json!({
            "model":"cccc-probe/first",
            "provider":{"cccc-probe":{
                "npm":"@ai-sdk/openai-compatible",
                "options":{"baseURL":"http://127.0.0.1:9/v1","apiKey":"local-test"},
                "models":{
                    "first":{"name":"First"},
                    "second":{"name":"Second","variants":{"default":{},"high":{"reasoningEffort":"high"}}}
                }
            }}
        }).to_string()),
    ]);
    let command = vec![
        executable,
        "acp".into(),
        "--hostname".into(),
        "127.0.0.1".into(),
        "--port".into(),
        port.to_string(),
        "--cwd".into(),
        root.to_string_lossy().into_owned(),
    ];
    let (owner, stdin, stdout) =
        process::spawn_piped(&command, root, &environment, "model-sync-test")
            .expect("start OpenCode");
    let owner = Arc::new(owner);
    lifecycle::wait_for_authenticated_backend(&endpoint, runtime, &password, Arc::clone(&owner))
        .await
        .expect("authenticated backend");
    let protocol = AcpClient::new(
        stdin,
        stdout,
        "model-sync-test".into(),
        runtime,
        PermissionPolicy::Reject,
        PromptCompletion::Response,
    )
    .expect("ACP client");
    protocol
        .request(
            "initialize",
            json!({"protocolVersion":1,"clientCapabilities":{}}),
            Duration::from_secs(20),
        )
        .await
        .expect("initialize");
    let created = protocol
        .request(
            "session/new",
            json!({"cwd":root,"mcpServers":[]}),
            Duration::from_secs(30),
        )
        .await
        .expect("session");
    let id = created["sessionId"].as_str().expect("session id");
    let mode = option(&created, "mode").expect("current native mode");
    assert_eq!(option(&created, "model"), Some("cccc-probe/first"));
    lifecycle::attach(&protocol, &endpoint, runtime, &password, id)
        .await
        .expect("SSE bridge");
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("HTTP client");

    for (model, variant) in [
        ("second", "high"),
        ("first", "default"),
        ("second", "default"),
    ] {
        let submitted = client
            .post(format!("{endpoint}/session/{id}/message"))
            .basic_auth(runtime, Some(&password))
            .json(&json!({
                "model":{"providerID":"cccc-probe","modelID":model},
                "variant":variant,
                "noReply":true,
                "parts":[{"type":"text","text":format!("selected {model}/{variant}")}]
            }))
            .send()
            .await
            .expect("submit native message");
        let status = submitted.status();
        let body = submitted.text().await.expect("submission response");
        assert!(status.is_success(), "native submission {status}: {body}");
        let expected = format!("cccc-probe/{model}");
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                // Setting mode returns all current options without changing the model.
                let config = protocol
                    .request(
                        "session/set_config_option",
                        json!({"sessionId":id,"configId":"mode","value":mode}),
                        Duration::from_secs(5),
                    )
                    .await
                    .expect("inspect current ACP model");
                if option(&config, "model") == Some(expected.as_str())
                    && (model == "first" || option(&config, "effort") == Some(variant))
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("submitted model/variant must reach ACP");
    }
    protocol.close().await;
    owner.stop().expect("stop isolated OpenCode");
}

fn option<'a>(config: &'a Value, id: &str) -> Option<&'a str> {
    config
        .get("configOptions")?
        .as_array()?
        .iter()
        .find(|option| option["id"] == id)?["currentValue"]
        .as_str()
}
