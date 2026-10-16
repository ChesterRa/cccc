use super::*;

#[test]
fn only_first_party_oauth_is_eligible() {
    let mut entry =
        json!({"auth_mode":"oidc", "key":"fixture-access", "oidc_issuer":"https://auth.x.ai"});
    assert!(valid_login(&entry));
    entry["auth_mode"] = json!("api_key");
    assert!(!valid_login(&entry));
    entry["auth_mode"] = json!("oidc");
    entry["oidc_issuer"] = json!("https://foreign.example");
    assert!(!valid_login(&entry));
    entry["oidc_issuer"] = json!("https://auth.x.ai");
    entry["oidc_client_id"] = json!("another-client");
    assert!(!valid_login(&entry));
}

#[cfg(unix)]
fn fixture(mode: &str) -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().expect("fixture home");
    let auth = temp.path().join("auth.json");
    std::fs::write(&auth, serde_json::to_vec(&json!({AUTH_SCOPE:{"auth_mode":"oidc","key":"fixture-access","user_id":"fixture-user","refresh_token":"fixture-refresh"}})).expect("Grok fixture")).expect("Grok fixture");
    let executable = temp.path().join("native-auth");
    let script = r#"#!/usr/bin/env python3
import json, os, pathlib, sys, subprocess, time
auth = pathlib.Path(os.environ['GROK_AUTH_PATH'])
audit = auth.with_suffix('.audit')
record = {'args': sys.argv[1:], 'home': os.environ['HOME'], 'methods': [],
          'api_disabled': os.environ.get('GROK_DISABLE_API_KEY_AUTH'),
          'isolated': os.environ['HOME'] == os.environ['GROK_HOME'] == os.getcwd(),
          'has_api_key': any(os.environ.get(k) for k in ['XAI_API_KEY','GROK_API_KEY'])}
audit.write_text(json.dumps(record))
for line in sys.stdin:
    request = json.loads(line)
    if 'id' not in request: continue
    method = request['method']
    record['methods'].append(method)
    audit.write_text(json.dumps(record))
    if method == 'initialize': result = {'protocolVersion':1, 'agentCapabilities':{}}
    elif method == '_x.ai/auth/getBearerToken':
        if MODE == 'hang':
            child = subprocess.Popen([sys.executable, '-c', 'import time;time.sleep(60)'])
            record['pid'] = os.getpid(); record['child'] = child.pid
            audit.write_text(json.dumps(record))
            time.sleep(60)
        store = json.loads(auth.read_text()); entry = next(iter(store.values()))
        if MODE == 'rotate': entry['key'] = 'fixture-rotated'; auth.write_text(json.dumps(store))
        if MODE == 'switch_user': entry['user_id'] = 'different-user'; auth.write_text(json.dumps(store))
        result = {'result':{'token':'fixture-foreign' if MODE == 'foreign' else entry['key']}}
    else: raise RuntimeError('unexpected native operation')
    print(json.dumps({'jsonrpc':'2.0','id':request['id'],'result':result}), flush=True)
"#;
    let script = script.replacen("import json,", &format!("MODE = {mode:?}\nimport json,"), 1);
    std::fs::write(&executable, script).expect("Grok fixture");
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
        .expect("Grok fixture");
    (temp, executable, auth)
}

#[cfg(unix)]
#[tokio::test]
async fn native_owner_refreshes_without_opening_a_model_session_and_cleans_its_home() {
    for mode in ["success", "rotate"] {
        let (_temp, executable, auth) = fixture(mode);
        let token = grok_voice_bearer(&executable, &auth)
            .await
            .expect("native OAuth");
        assert!(
            token
                == if mode == "rotate" {
                    "fixture-rotated"
                } else {
                    "fixture-access"
                }
        );
        let audit: Value = serde_json::from_slice(
            &std::fs::read(auth.with_extension("audit")).expect("Grok fixture"),
        )
        .expect("Grok fixture");
        assert_eq!(audit["args"], json!(["agent", "--no-leader", "stdio"]));
        assert_eq!(
            audit["methods"],
            json!(["initialize", "_x.ai/auth/getBearerToken"])
        );
        assert_eq!(audit["api_disabled"], "1");
        assert_eq!(audit["has_api_key"], false);
        assert_eq!(audit["isolated"], true);
        assert!(!Path::new(audit["home"].as_str().expect("Grok fixture")).exists());
    }
}

#[cfg(unix)]
#[tokio::test]
async fn native_token_must_match_the_selected_oauth_identity() {
    for mode in ["foreign", "switch_user"] {
        let (_temp, executable, auth) = fixture(mode);
        let Err(error) = grok_voice_bearer(&executable, &auth).await else {
            panic!("identity change must be rejected");
        };
        assert!(matches!(
            error.downcast_ref::<RealtimeCallError>(),
            Some(RealtimeCallError::GrokAuthentication)
        ));
        assert!(!format!("{error:#}").contains("fixture-"));
    }
}

#[cfg(unix)]
#[tokio::test]
async fn static_key_or_missing_login_never_launches_the_native_helper() {
    let (_temp, executable, auth) = fixture("success");
    for content in [
        json!({AUTH_SCOPE:{"auth_mode":"api_key","key":"fixture-key"}}),
        json!({}),
    ] {
        std::fs::write(&auth, serde_json::to_vec(&content).expect("Grok fixture"))
            .expect("Grok fixture");
        assert!(!grok_voice_credentials_available(&auth).await);
        assert!(grok_voice_bearer(&executable, &auth).await.is_err());
        assert!(!auth.with_extension("audit").exists());
    }
}

#[cfg(unix)]
#[tokio::test]
async fn cancelled_auth_reaps_the_owned_process_tree_and_temporary_home() {
    let (_temp, executable, auth) = fixture("hang");
    let auth_clone = auth.clone();
    let task = tokio::spawn(async move { grok_voice_bearer(&executable, &auth_clone).await });
    let audit = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let value = std::fs::read(auth.with_extension("audit"))
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
            if let Some(value) = value.filter(|value| value["child"].is_u64()) {
                break value;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("helper waiting");
    task.abort();
    let _ = task.await;
    assert!(!Path::new(audit["home"].as_str().expect("Grok fixture")).exists());
    for key in ["pid", "child"] {
        let pid = audit[key].as_u64().expect("Grok fixture").to_string();
        // An adopted zombie has exited as well; no running native process may remain.
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let state = std::process::Command::new("ps")
                    .args(["-o", "stat=", "-p", &pid])
                    .output()
                    .expect("Grok fixture");
                let state = String::from_utf8_lossy(&state.stdout);
                if state.trim().is_empty() || state.trim().starts_with('Z') {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("owned process exited");
    }
}
