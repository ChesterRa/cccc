use super::payload::{analyst_info_value, info_value};
use super::terminal::{parsed_terminal_size, valid_terminal_size};
use crate::codex_voice::{AnalystInfo, SessionInfo};
use crate::{WebMode, app_with_mode};
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_core::HomeLayout;
use cccc_core::access_tokens::AccessTokenStore;
use serde_json::json;
use tower::ServiceExt;

#[test]
fn public_voice_payloads_do_not_expose_local_paths_or_codex_commands() {
    let value = info_value(SessionInfo {
        generation: "voice-1".into(),
        analyst_generation: "analyst-1".into(),
        voice: "cove".into(),
        connected: true,
    });
    let analyst = analyst_info_value(AnalystInfo {
        generation: "analyst-1".into(),
        tui_ready: true,
        phase: "ready".into(),
        last_result: "done".into(),
        warning: String::new(),
    });

    assert!(value.get("group_id").is_none());
    assert_eq!(analyst["tui_ready"], true);
    for forbidden in ["root", "analyst_thread_id", "analyst_tui_command"] {
        assert!(value.get(forbidden).is_none());
        assert!(analyst.get(forbidden).is_none());
    }
}

#[test]
fn voice_terminal_sizes_are_bounded() {
    assert_eq!(valid_terminal_size(120, 32), Some((120, 32)));
    assert_eq!(valid_terminal_size(9, 32), None);
    assert_eq!(
        parsed_terminal_size(&json!({"cols":144,"rows":40})),
        Some((144, 40))
    );
    assert_eq!(parsed_terminal_size(&json!({"cols":144,"rows":1})), None);
}

#[tokio::test]
async fn admin_can_update_voice_analyst_launch_settings_without_secret_echo() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let token = AccessTokenStore::new(home.clone())
        .expect("tokens")
        .create("admin", Vec::new(), true, None)
        .expect("admin token");
    let authorization = format!("Bearer {}", token.token);
    let router = app_with_mode(home.clone(), WebMode::Normal);
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/v1/codex_voice/analyst-settings")
                .header(header::AUTHORIZATION, &authorization)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "settings":{
                            "runtime":"codex",
                            "command":"codex --profile voice -c 'model=\"gpt-test\"'"
                        },
                        "environment_set":{"OPENAI_API_KEY":"private-value"}
                    })
                    .to_string(),
                ))
                .expect("update request"),
        )
        .await
        .expect("update response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        cccc_core::codex_voice_settings::load(&home)
            .expect("saved settings")
            .command,
        ["codex", "--profile", "voice", "-c", "model=\"gpt-test\""]
    );
    assert_eq!(
        cccc_core::codex_voice_settings::private_environment(&home)
            .expect("saved private environment")
            .get("OPENAI_API_KEY")
            .map(String::as_str),
        Some("private-value")
    );

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/codex_voice/analyst-settings")
                .header(header::AUTHORIZATION, &authorization)
                .body(Body::empty())
                .expect("settings request"),
        )
        .await
        .expect("settings response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("settings body");
    let text = String::from_utf8(body.to_vec()).expect("UTF-8 settings body");
    assert!(text.contains("OPENAI_API_KEY"));
    assert!(!text.contains("private-value"));

    let response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/v1/codex_voice/analyst-settings")
                .header(header::AUTHORIZATION, authorization)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "settings":{
                            "runtime":"codex",
                            "command":["codex","--profile","voice","-c","model=\"gpt-test\""]
                        },
                        "environment_clear":true
                    })
                    .to_string(),
                ))
                .expect("clear request"),
        )
        .await
        .expect("clear response");
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        cccc_core::codex_voice_settings::private_environment(&home)
            .expect("cleared private environment")
            .is_empty()
    );
}

#[tokio::test]
async fn admin_can_recover_from_a_deleted_runtime_profile_binding() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    cccc_core::codex_voice_settings::save(
        &home,
        &cccc_contracts::CodexVoiceAnalystSettings {
            profile_id: "deleted-profile".into(),
            ..Default::default()
        },
    )
    .expect("dangling settings");
    let token = AccessTokenStore::new(home.clone())
        .expect("tokens")
        .create("admin", Vec::new(), true, None)
        .expect("admin token");
    let response = app_with_mode(home.clone(), WebMode::Normal)
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/v1/codex_voice/analyst-settings")
                .header(header::AUTHORIZATION, format!("Bearer {}", token.token))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "settings":{"runtime":"codex","command":[]}
                    })
                    .to_string(),
                ))
                .expect("recovery request"),
        )
        .await
        .expect("recovery response");
    assert_eq!(response.status(), StatusCode::OK);
    let saved = cccc_core::codex_voice_settings::load(&home).expect("saved settings");
    assert!(saved.profile_id.is_empty());
}
