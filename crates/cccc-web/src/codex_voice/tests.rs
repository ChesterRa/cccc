use super::AnalystSnapshot;
use super::analyst_runtime::actor_result_is_speakable;
use super::persistence::{
    PersistedAnalyst, TEST_ANALYST_STATE_FILE, TEST_ANALYST_WORKSPACE_VERSION, resumable_thread,
    validate_client_session_id,
};
use cccc_contracts::ActorRuntime;
use cccc_core::{GroupStore, HomeLayout};
use std::collections::BTreeMap;

fn test_identity(codex_home: &str) -> String {
    cccc_core::codex_voice_settings::ResolvedAgentRuntime {
        runtime: ActorRuntime::Codex,
        command: vec!["codex".into()],
        environment: BTreeMap::from([("CODEX_HOME".into(), codex_home.into())]),
    }
    .identity_fingerprint()
}

#[test]
fn client_session_ids_are_bounded_and_path_safe() {
    assert_eq!(
        validate_client_session_id("call_123-abc").expect("valid client session id"),
        "call_123-abc"
    );
    assert!(validate_client_session_id("../call").is_err());
    assert!(validate_client_session_id(&"x".repeat(129)).is_err());
}

#[test]
fn a_disconnected_analyst_is_replaced_before_the_next_call() {
    assert!(
        AnalystSnapshot {
            phase: "ready".into(),
            last_result: String::new(),
            warning: String::new(),
        }
        .reusable_for_call()
    );
    assert!(
        !AnalystSnapshot {
            phase: "needs_attention".into(),
            last_result: String::new(),
            warning: "analyst_disconnected".into(),
        }
        .reusable_for_call()
    );
    assert!(
        !AnalystSnapshot {
            phase: "needs_attention".into(),
            last_result: String::new(),
            warning: "analyst_event_gap".into(),
        }
        .reusable_for_call()
    );
}

#[test]
fn actor_results_return_to_the_analyst_but_only_the_matching_call_may_speak() {
    assert!(actor_result_is_speakable(Some("call-2"), "call-2"));
    assert!(!actor_result_is_speakable(Some("call-2"), "call-1"));
    assert!(!actor_result_is_speakable(None, "call-1"));
}

#[test]
fn only_a_materialized_current_workspace_receipt_resumes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let workdir = cccc_core::codex_voice_settings::workdir(&home).expect("workdir");
    let identity = test_identity("/tmp/codex-current");
    let state = home.daemon_dir().join(TEST_ANALYST_STATE_FILE);
    cccc_core::fs::write_json(
        &state,
        &PersistedAnalyst {
            workspace_version: TEST_ANALYST_WORKSPACE_VERSION,
            group_id: String::new(),
            root: workdir.to_string_lossy().into_owned(),
            thread_id: "thread-1".into(),
            identity_fingerprint: identity.clone(),
            materialized: false,
            updated_at: cccc_contracts::utc_now(),
        },
    )
    .expect("write state");
    assert_eq!(
        resumable_thread(&home, &workdir, &identity)
            .expect("receipt")
            .0,
        None
    );

    let mut persisted: PersistedAnalyst = cccc_core::fs::read_json(&state).expect("state");
    persisted.materialized = true;
    cccc_core::fs::write_json(&state, &persisted).expect("write materialized state");
    assert_eq!(
        resumable_thread(&home, &workdir, &identity)
            .expect("resumable receipt")
            .0
            .as_deref(),
        Some("thread-1")
    );
}

#[test]
fn a_legacy_repo_bound_receipt_starts_fresh_with_a_migration_warning() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let legacy_root = temp.path().join("legacy-repo");
    std::fs::create_dir_all(&legacy_root).expect("legacy root");
    cccc_core::fs::write_json(
        &home.daemon_dir().join(TEST_ANALYST_STATE_FILE),
        &PersistedAnalyst {
            workspace_version: 0,
            group_id: "g_legacy".into(),
            root: legacy_root.to_string_lossy().into_owned(),
            thread_id: "thread-legacy".into(),
            identity_fingerprint: String::new(),
            materialized: true,
            updated_at: cccc_contracts::utc_now(),
        },
    )
    .expect("write legacy state");

    let workdir = cccc_core::codex_voice_settings::workdir(&home).expect("workdir");
    assert_eq!(
        resumable_thread(&home, &workdir, &test_identity("/tmp/codex-current"))
            .expect("migration result"),
        (None, "analyst_workspace_migrated".into())
    );
}

#[test]
fn the_latest_legacy_group_receipt_is_detected_when_global_state_is_missing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("Legacy Voice", "").expect("group");
    let legacy_path = store
        .state_dir(&group.group_id)
        .expect("state")
        .join(TEST_ANALYST_STATE_FILE);
    cccc_core::fs::write_json(
        &legacy_path,
        &PersistedAnalyst {
            workspace_version: 0,
            group_id: group.group_id,
            root: temp
                .path()
                .join("legacy-repo")
                .to_string_lossy()
                .into_owned(),
            thread_id: "thread-legacy".into(),
            identity_fingerprint: String::new(),
            materialized: true,
            updated_at: "2026-09-01T00:00:00Z".into(),
        },
    )
    .expect("write legacy state");

    let workdir = cccc_core::codex_voice_settings::workdir(&home).expect("workdir");
    assert_eq!(
        resumable_thread(&home, &workdir, &test_identity("/tmp/codex-current"))
            .expect("legacy migration"),
        (None, "analyst_workspace_migrated".into())
    );
}

#[test]
fn a_current_receipt_for_another_workdir_starts_fresh_with_a_warning() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let stale = temp.path().join("stale-workdir");
    std::fs::create_dir_all(&stale).expect("stale workdir");
    cccc_core::fs::write_json(
        &home.daemon_dir().join(TEST_ANALYST_STATE_FILE),
        &PersistedAnalyst {
            workspace_version: TEST_ANALYST_WORKSPACE_VERSION,
            group_id: String::new(),
            root: stale.to_string_lossy().into_owned(),
            thread_id: "thread-stranded".into(),
            identity_fingerprint: test_identity("/tmp/codex-current"),
            materialized: true,
            updated_at: cccc_contracts::utc_now(),
        },
    )
    .expect("write state");

    let workdir = cccc_core::codex_voice_settings::workdir(&home).expect("workdir");
    assert_eq!(
        resumable_thread(&home, &workdir, &test_identity("/tmp/codex-current"))
            .expect("replacement warning"),
        (None, "analyst_resume_replaced".into())
    );
}

#[test]
fn a_current_receipt_from_another_codex_identity_starts_fresh() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let workdir = cccc_core::codex_voice_settings::workdir(&home).expect("workdir");
    cccc_core::fs::write_json(
        &home.daemon_dir().join(TEST_ANALYST_STATE_FILE),
        &PersistedAnalyst {
            workspace_version: TEST_ANALYST_WORKSPACE_VERSION,
            group_id: String::new(),
            root: workdir.to_string_lossy().into_owned(),
            thread_id: "thread-old-identity".into(),
            identity_fingerprint: test_identity("/tmp/codex-old"),
            materialized: true,
            updated_at: cccc_contracts::utc_now(),
        },
    )
    .expect("write state");

    assert_eq!(
        resumable_thread(&home, &workdir, &test_identity("/tmp/codex-new"))
            .expect("identity replacement"),
        (None, "analyst_configuration_started_new_session".into())
    );
}
