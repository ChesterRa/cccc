use super::AnalystSnapshot;
use super::analyst_runtime::actor_result_is_speakable;
use super::persistence::{
    PersistedAnalyst, TEST_ANALYST_STATE_FILE, resolve_resumable_scope, resumable_thread_id,
    validate_client_session_id,
};
use cccc_core::{GroupStore, HomeLayout, Scope, group_scope};

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
}

#[test]
fn actor_results_return_to_the_analyst_but_only_the_matching_call_may_speak() {
    assert!(actor_result_is_speakable(Some("call-2"), "call-2"));
    assert!(!actor_result_is_speakable(Some("call-2"), "call-1"));
    assert!(!actor_result_is_speakable(None, "call-1"));
}

#[test]
fn only_materialized_matching_analyst_bindings_resume() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let group = GroupStore::new(home.clone())
        .expect("store")
        .create("Voice", "")
        .expect("group");
    let root = temp.path().join("repo");
    std::fs::create_dir_all(&root).expect("root");
    let state = home.daemon_dir().join(TEST_ANALYST_STATE_FILE);
    cccc_core::fs::write_json(
        &state,
        &PersistedAnalyst {
            group_id: group.group_id.clone(),
            root: root.to_string_lossy().into_owned(),
            thread_id: "thread-1".into(),
            materialized: false,
            updated_at: cccc_contracts::utc_now(),
        },
    )
    .expect("write state");
    assert_eq!(resumable_thread_id(&home, &group.group_id, &root), None);

    let mut persisted: PersistedAnalyst = cccc_core::fs::read_json(&state).expect("state");
    persisted.materialized = true;
    cccc_core::fs::write_json(&state, &persisted).expect("write materialized state");
    assert_eq!(
        resumable_thread_id(&home, &group.group_id, &root).as_deref(),
        Some("thread-1")
    );
    assert_eq!(
        resumable_thread_id(&home, &group.group_id, &temp.path().join("other")),
        None
    );
}

#[test]
fn a_materialized_global_binding_restores_independently_of_sidebar_selection() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("Voice", "").expect("group");
    let root = temp.path().join("repo");
    std::fs::create_dir_all(&root).expect("root");
    let scope = Scope {
        scope_key: "s_voice".into(),
        url: root.to_string_lossy().into_owned(),
        label: "Voice".into(),
        git_remote: String::new(),
    };
    group_scope::attach(&store, &group.group_id, scope).expect("attach scope");
    cccc_core::fs::write_json(
        &home.daemon_dir().join(TEST_ANALYST_STATE_FILE),
        &PersistedAnalyst {
            group_id: group.group_id.clone(),
            root: root.to_string_lossy().into_owned(),
            thread_id: "thread-global".into(),
            materialized: true,
            updated_at: cccc_contracts::utc_now(),
        },
    )
    .expect("write state");

    let (resolved_group_id, resolved_title, resolved_root) = resolve_resumable_scope(&home)
        .expect("resolve global binding")
        .expect("resumable global binding");
    assert_eq!(resolved_group_id, group.group_id);
    assert_eq!(resolved_title, "Voice");
    assert_eq!(resolved_root, root.canonicalize().expect("canonical root"));
}

#[test]
fn the_latest_legacy_group_binding_migrates_into_the_global_resume_path() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("Legacy Voice", "").expect("group");
    let root = temp.path().join("legacy-repo");
    std::fs::create_dir_all(&root).expect("root");
    group_scope::attach(
        &store,
        &group.group_id,
        Scope {
            scope_key: "s_legacy_voice".into(),
            url: root.to_string_lossy().into_owned(),
            label: "Legacy Voice".into(),
            git_remote: String::new(),
        },
    )
    .expect("attach scope");
    let legacy_path = store
        .state_dir(&group.group_id)
        .expect("state")
        .join(TEST_ANALYST_STATE_FILE);
    cccc_core::fs::write_json(
        &legacy_path,
        &PersistedAnalyst {
            group_id: group.group_id.clone(),
            root: root.to_string_lossy().into_owned(),
            thread_id: "thread-legacy".into(),
            materialized: true,
            updated_at: "2026-09-01T00:00:00Z".into(),
        },
    )
    .expect("write legacy state");

    assert_eq!(
        resumable_thread_id(&home, &group.group_id, &root).as_deref(),
        Some("thread-legacy")
    );
    assert_eq!(
        resolve_resumable_scope(&home)
            .expect("resolve legacy binding")
            .map(|(group_id, _, _)| group_id)
            .as_deref(),
        Some(group.group_id.as_str())
    );
}

#[test]
fn an_invalid_materialized_binding_is_not_treated_as_missing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let root = temp.path().join("removed-repo");
    std::fs::create_dir_all(&root).expect("root");
    cccc_core::fs::write_json(
        &home.daemon_dir().join(TEST_ANALYST_STATE_FILE),
        &PersistedAnalyst {
            group_id: "g_removed".into(),
            root: root.to_string_lossy().into_owned(),
            thread_id: "thread-stranded".into(),
            materialized: true,
            updated_at: cccc_contracts::utc_now(),
        },
    )
    .expect("write state");

    let error = resolve_resumable_scope(&home).expect_err("invalid binding must be distinct");
    assert!(error.to_string().contains("persisted Voice Analyst Group"));
}
