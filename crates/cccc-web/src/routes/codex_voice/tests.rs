use super::handlers::codex_credentials_available;
use super::payload::{analyst_info_value, info_value};
use super::terminal::{parsed_terminal_size, valid_terminal_size};
use crate::codex_voice::{AnalystInfo, SessionInfo};
use serde_json::json;

#[test]
fn public_voice_payloads_do_not_expose_local_paths_or_codex_commands() {
    let value = info_value(SessionInfo {
        group_id: "g_alpha".into(),
        group_title: "Alpha".into(),
        generation: "voice-1".into(),
        analyst_generation: "analyst-1".into(),
        voice: "cove".into(),
        connected: true,
    });
    let analyst = analyst_info_value(AnalystInfo {
        group_id: "g_alpha".into(),
        group_title: "Alpha".into(),
        generation: "analyst-1".into(),
        tui_ready: true,
        phase: "ready".into(),
        last_result: "done".into(),
        warning: String::new(),
    });

    assert_eq!(value["group_id"], "g_alpha");
    assert_eq!(analyst["tui_ready"], true);
    for forbidden in ["root", "analyst_thread_id", "analyst_tui_command"] {
        assert!(value.get(forbidden).is_none());
        assert!(analyst.get(forbidden).is_none());
    }
}

#[test]
fn readiness_requires_both_codex_token_fields() {
    assert!(codex_credentials_available(
        br#"{"tokens":{"access_token":"access","account_id":"account"}}"#
    ));
    assert!(!codex_credentials_available(
        br#"{"tokens":{"access_token":"access"}}"#
    ));
    assert!(!codex_credentials_available(b"not-json"));
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
