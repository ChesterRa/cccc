use cccc_contracts::DaemonRequest;
use cccc_core::{
    GroupStore, HomeLayout, Registry, access_tokens::AccessTokenStore, active, assistant_state,
    group_bridge_legacy, im_state, inbox, ledger, membership, nomcp, presentation,
    profiles::ProfileStore, settings, space_credentials, voice_recording_lease,
    web_model_connectors,
};
use serde_json::{Map, Value, json};
use std::path::Path;

const FIXTURE_ROOT: &str = "fixtures/python-0.4.35";

const F0_FILES: &[(&str, &str)] = &[
    (
        ".initialized",
        include_str!("fixtures/python-0.4.35/.initialized"),
    ),
    (
        "active.json",
        include_str!("fixtures/python-0.4.35/active.json"),
    ),
    (
        "registry.json",
        include_str!("fixtures/python-0.4.35/registry.json"),
    ),
    (
        "settings.yaml",
        include_str!("fixtures/python-0.4.35/settings.yaml"),
    ),
    (
        "access_tokens.yaml",
        include_str!("fixtures/python-0.4.35/access_tokens.yaml"),
    ),
    (
        "secrets/membership.json",
        include_str!("fixtures/python-0.4.35/secrets/membership.json"),
    ),
    (
        "groups/g_python0435/group.yaml",
        include_str!("fixtures/python-0.4.35/groups/g_python0435/group.yaml"),
    ),
    (
        "groups/g_python0435/ledger.jsonl",
        include_str!("fixtures/python-0.4.35/groups/g_python0435/ledger.jsonl"),
    ),
];

const F1_FILES: &[(&str, &str)] = &[
    (
        "groups/g_python0435/group.yaml",
        include_str!(
            "fixtures/python-0.4.35/f1-identity-integrations/groups/g_python0435/group.yaml"
        ),
    ),
    (
        "groups/g_python0435/state/im_authorized_chats.json",
        include_str!(
            "fixtures/python-0.4.35/f1-identity-integrations/groups/g_python0435/state/im_authorized_chats.json"
        ),
    ),
    (
        "groups/g_python0435/state/im_pending_keys.json",
        include_str!(
            "fixtures/python-0.4.35/f1-identity-integrations/groups/g_python0435/state/im_pending_keys.json"
        ),
    ),
    (
        "groups/g_python0435/state/im_subscribers.json",
        include_str!(
            "fixtures/python-0.4.35/f1-identity-integrations/groups/g_python0435/state/im_subscribers.json"
        ),
    ),
    (
        "state/actor_profiles/.rust-profiles-migrated-v1",
        include_str!(
            "fixtures/python-0.4.35/f1-identity-integrations/state/actor_profiles/.rust-profiles-migrated-v1"
        ),
    ),
    (
        "state/actor_profiles/profiles.json",
        include_str!(
            "fixtures/python-0.4.35/f1-identity-integrations/state/actor_profiles/profiles.json"
        ),
    ),
    (
        "state/secrets/actor_profiles/fixture-profile.4d5fd5739bc8977b.json",
        include_str!(
            "fixtures/python-0.4.35/f1-identity-integrations/state/secrets/actor_profiles/fixture-profile.4d5fd5739bc8977b.json"
        ),
    ),
    (
        "state/secrets/actors/g_python0435/.rust-actor-secrets-migrated-v1",
        include_str!(
            "fixtures/python-0.4.35/f1-identity-integrations/state/secrets/actors/g_python0435/.rust-actor-secrets-migrated-v1"
        ),
    ),
    (
        "state/secrets/actors/g_python0435/peer1.698750a09b934337.json",
        include_str!(
            "fixtures/python-0.4.35/f1-identity-integrations/state/secrets/actors/g_python0435/peer1.698750a09b934337.json"
        ),
    ),
    (
        "state/nomcp_sessions/nomcp_fixture0001.json",
        include_str!(
            "fixtures/python-0.4.35/f1-identity-integrations/state/nomcp_sessions/nomcp_fixture0001.json"
        ),
    ),
    (
        "web_model_connectors.yaml",
        include_str!("fixtures/python-0.4.35/f1-identity-integrations/web_model_connectors.yaml"),
    ),
    (
        "group_bridge_pairing.yaml",
        include_str!("fixtures/python-0.4.35/f1-identity-integrations/group_bridge_pairing.yaml"),
    ),
    (
        "group_bridge_registrations.yaml",
        include_str!(
            "fixtures/python-0.4.35/f1-identity-integrations/group_bridge_registrations.yaml"
        ),
    ),
    (
        "group_bridge_credentials.yaml",
        include_str!(
            "fixtures/python-0.4.35/f1-identity-integrations/group_bridge_credentials.yaml"
        ),
    ),
    (
        "group_bridge_receipts.yaml",
        include_str!("fixtures/python-0.4.35/f1-identity-integrations/group_bridge_receipts.yaml"),
    ),
];

const F2_FILES: &[(&str, &str)] = &[
    (
        "groups/g_python0435/group.yaml",
        include_str!("fixtures/python-0.4.35/f2-product-state/groups/g_python0435/group.yaml"),
    ),
    (
        "groups/g_python0435/state/.rust-space-migrated-v1",
        include_str!(
            "fixtures/python-0.4.35/f2-product-state/groups/g_python0435/state/.rust-space-migrated-v1"
        ),
    ),
    (
        "groups/g_python0435/state/assistants.json",
        include_str!(
            "fixtures/python-0.4.35/f2-product-state/groups/g_python0435/state/assistants.json"
        ),
    ),
    (
        "groups/g_python0435/state/automation.json",
        include_str!(
            "fixtures/python-0.4.35/f2-product-state/groups/g_python0435/state/automation.json"
        ),
    ),
    (
        "groups/g_python0435/state/presentation.json",
        include_str!(
            "fixtures/python-0.4.35/f2-product-state/groups/g_python0435/state/presentation.json"
        ),
    ),
    (
        "state/capabilities/.rust-capabilities-migrated-v1",
        include_str!(
            "fixtures/python-0.4.35/f2-product-state/state/capabilities/.rust-capabilities-migrated-v1"
        ),
    ),
    (
        "state/capabilities/catalog.json",
        include_str!("fixtures/python-0.4.35/f2-product-state/state/capabilities/catalog.json"),
    ),
    (
        "state/capabilities/state.json",
        include_str!("fixtures/python-0.4.35/f2-product-state/state/capabilities/state.json"),
    ),
    (
        "state/secrets/space_providers/notebooklm.3a74eb58101dbfe3.json",
        include_str!(
            "fixtures/python-0.4.35/f2-product-state/state/secrets/space_providers/notebooklm.3a74eb58101dbfe3.json"
        ),
    ),
    (
        "state/space/bindings.json",
        include_str!("fixtures/python-0.4.35/f2-product-state/state/space/bindings.json"),
    ),
    (
        "state/space/job_payloads/spj_fixture.d9d2431952d929c6fdb43e048d93b223.json",
        include_str!(
            "fixtures/python-0.4.35/f2-product-state/state/space/job_payloads/spj_fixture.d9d2431952d929c6fdb43e048d93b223.json"
        ),
    ),
    (
        "state/space/jobs.json",
        include_str!("fixtures/python-0.4.35/f2-product-state/state/space/jobs.json"),
    ),
    (
        "state/space/providers.json",
        include_str!("fixtures/python-0.4.35/f2-product-state/state/space/providers.json"),
    ),
    (
        "state/voice_secretary_recording_lease.json",
        include_str!(
            "fixtures/python-0.4.35/f2-product-state/state/voice_secretary_recording_lease.json"
        ),
    ),
    (
        "voice-secretary/g_python0435/documents/index.json",
        include_str!(
            "fixtures/python-0.4.35/f2-product-state/voice-secretary/g_python0435/documents/index.json"
        ),
    ),
    (
        "voice-secretary/g_python0435/documents/voice-doc-fixture/transcript.jsonl",
        include_str!(
            "fixtures/python-0.4.35/f2-product-state/voice-secretary/g_python0435/documents/voice-doc-fixture/transcript.jsonl"
        ),
    ),
    (
        "voice-secretary/g_python0435/fixture-session/transcripts/segments.jsonl",
        include_str!(
            "fixtures/python-0.4.35/f2-product-state/voice-secretary/g_python0435/fixture-session/transcripts/segments.jsonl"
        ),
    ),
    (
        "workspace/docs/voice-secretary/fixture-voice-notes.md",
        include_str!(
            "fixtures/python-0.4.35/f2-product-state/workspace/docs/voice-secretary/fixture-voice-notes.md"
        ),
    ),
];

const F3_FILES: &[(&str, &str)] = &[
    (
        "settings.yaml",
        include_str!("fixtures/python-0.4.35/f3-retired-shadows/settings.yaml"),
    ),
    (
        "groups/g_python0435/group.yaml",
        include_str!("fixtures/python-0.4.35/f3-retired-shadows/groups/g_python0435/group.yaml"),
    ),
    (
        "groups/g_python0435/state/im_authorized_chats.json",
        include_str!(
            "fixtures/python-0.4.35/f3-retired-shadows/groups/g_python0435/state/im_authorized_chats.json"
        ),
    ),
    (
        "groups/g_python0435/state/im_pending_keys.json",
        include_str!(
            "fixtures/python-0.4.35/f3-retired-shadows/groups/g_python0435/state/im_pending_keys.json"
        ),
    ),
    (
        "groups/g_python0435/state/im_subscribers.json",
        include_str!(
            "fixtures/python-0.4.35/f3-retired-shadows/groups/g_python0435/state/im_subscribers.json"
        ),
    ),
    (
        "groups/g_python0435/state/actor-secrets.json",
        include_str!(
            "fixtures/python-0.4.35/f3-retired-shadows/groups/g_python0435/state/actor-secrets.json"
        ),
    ),
    (
        "groups/g_python0435/state/.rust-space-migrated-v1",
        include_str!(
            "fixtures/python-0.4.35/f3-retired-shadows/groups/g_python0435/state/.rust-space-migrated-v1"
        ),
    ),
    (
        "state/web_model_browser/g_python0435/web1/state.json",
        include_str!(
            "fixtures/python-0.4.35/f3-retired-shadows/state/web_model_browser/g_python0435/web1/state.json"
        ),
    ),
    (
        "state/actor_profiles/.rust-profiles-migrated-v1",
        include_str!(
            "fixtures/python-0.4.35/f3-retired-shadows/state/actor_profiles/.rust-profiles-migrated-v1"
        ),
    ),
    (
        "state/actor_profiles/profiles.json",
        include_str!(
            "fixtures/python-0.4.35/f3-retired-shadows/state/actor_profiles/profiles.json"
        ),
    ),
    (
        "profiles.json",
        include_str!("fixtures/python-0.4.35/f3-retired-shadows/profiles.json"),
    ),
    (
        "profile-secrets.json",
        include_str!("fixtures/python-0.4.35/f3-retired-shadows/profile-secrets.json"),
    ),
    (
        "state/secrets/actors/g_python0435/.rust-actor-secrets-migrated-v1",
        include_str!(
            "fixtures/python-0.4.35/f3-retired-shadows/state/secrets/actors/g_python0435/.rust-actor-secrets-migrated-v1"
        ),
    ),
    (
        "web_model_connectors.yaml",
        include_str!("fixtures/python-0.4.35/f3-retired-shadows/web_model_connectors.yaml"),
    ),
    (
        "group_bridge_pairing.yaml",
        include_str!("fixtures/python-0.4.35/f3-retired-shadows/group_bridge_pairing.yaml"),
    ),
    (
        "group_bridge_registrations.yaml",
        include_str!("fixtures/python-0.4.35/f3-retired-shadows/group_bridge_registrations.yaml"),
    ),
    (
        "group_bridge_credentials.yaml",
        include_str!("fixtures/python-0.4.35/f3-retired-shadows/group_bridge_credentials.yaml"),
    ),
    (
        "group_bridge_receipts.yaml",
        include_str!("fixtures/python-0.4.35/f3-retired-shadows/group_bridge_receipts.yaml"),
    ),
    (
        "state/nomcp_sessions/nomcp_fixture0001.json",
        include_str!(
            "fixtures/python-0.4.35/f3-retired-shadows/state/nomcp_sessions/nomcp_fixture0001.json"
        ),
    ),
    (
        "state/secrets/space_providers/.rust-credentials-migrated-v1",
        include_str!(
            "fixtures/python-0.4.35/f3-retired-shadows/state/secrets/space_providers/.rust-credentials-migrated-v1"
        ),
    ),
    (
        "space-credentials.json",
        include_str!("fixtures/python-0.4.35/f3-retired-shadows/space-credentials.json"),
    ),
    (
        "state/space/bindings.json",
        include_str!("fixtures/python-0.4.35/f3-retired-shadows/state/space/bindings.json"),
    ),
    (
        "state/capabilities/.rust-capabilities-migrated-v1",
        include_str!(
            "fixtures/python-0.4.35/f3-retired-shadows/state/capabilities/.rust-capabilities-migrated-v1"
        ),
    ),
    (
        "state/capabilities/state.json",
        include_str!("fixtures/python-0.4.35/f3-retired-shadows/state/capabilities/state.json"),
    ),
];

fn materialize(root: &Path, files: &[(&str, &str)]) {
    for (relative, contents) in files {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().expect("fixture file parent"))
            .expect("create fixture parent");
        let contents = contents.replace(
            "__FIXTURE_HOME__",
            root.to_str().expect("fixture home is UTF-8"),
        );
        std::fs::write(path, contents).expect("write frozen fixture file");
    }
}

fn snapshot(root: &Path, files: &[(&'static str, &'static str)]) -> Vec<(&'static str, Vec<u8>)> {
    files
        .iter()
        .map(|(relative, _)| {
            (
                *relative,
                std::fs::read(root.join(relative)).expect("read fixture snapshot"),
            )
        })
        .collect()
}

fn request(home: &HomeLayout, op: &str, args: Value) -> Value {
    let response = cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_else(Map::new),
        },
    );
    assert!(response.ok, "{op}: {:?}", response.error);
    Value::Object(response.result)
}

#[test]
fn rust_reads_the_frozen_python_0435_core_home_without_python() {
    let manifest: Value = serde_json::from_str(include_str!(
        "fixtures/python-0.4.35/_fixture_manifest.json"
    ))
    .expect("fixture manifest");
    assert_eq!(manifest["product_version"], "0.4.35");
    assert_eq!(
        manifest["source_commit"],
        "4a5c5efa973a6a2b91471466edb5cce7834a284f"
    );
    assert_eq!(manifest["sanitized"], true);

    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("python-home");
    materialize(&root, F0_FILES);
    let before = snapshot(&root, F0_FILES);

    let home = HomeLayout::from_path(&root).expect("Python home layout");
    home.initialize().expect("Rust adopts Python home");
    assert!(root.join(".cccc-rust-v1").is_file());

    let registry = Registry::load(&home).expect("Rust reads Python registry");
    assert_eq!(registry.groups.len(), 1);
    assert_eq!(registry.defaults["s_python0435"], "g_python0435");
    assert_eq!(
        active::get(&home)
            .expect("Rust reads active group")
            .as_deref(),
        Some("g_python0435")
    );

    let groups = GroupStore::new(home.clone()).expect("group store");
    let group = groups
        .load("g_python0435")
        .expect("Rust reads Python group");
    assert_eq!(group.title, "Python 0.4.35 upgrade fixture");
    assert_eq!(group.active_scope_key, "s_python0435");
    assert_eq!(group.scopes.len(), 1);
    assert_eq!(group.actors.len(), 1);
    assert_eq!(group.actors[0].id, "peer1");

    let events = ledger::read_all(&groups.ledger_path(&group.group_id).expect("ledger path"))
        .expect("Rust reads Python ledger");
    assert_eq!(events.len(), 5);
    assert_eq!(events[3].data["client_id"], "fixture-mail-1");
    assert_eq!(events[4].data["client_id"], "fixture-send-1");
    let unread =
        inbox::list_unread(&home, &group, "peer1", 20).expect("Rust projects Python Mail inbox");
    assert_eq!(unread.len(), 1);
    assert_eq!(unread[0].data["message_mode"], "mail");
    assert_eq!(unread[0].data["client_id"], "fixture-mail-1");

    let global = settings::load(&home).expect("Rust reads Python settings");
    assert_eq!(global.branding["name"], "Python 0.4.35 Fixture");
    assert_eq!(global.remote_access["provider"], "off");
    let token = AccessTokenStore::new(home.clone())
        .expect("token store")
        .lookup("acc_python_0435_fixture")
        .expect("token lookup")
        .expect("Python token");
    assert_eq!(token.user_id, "fixture-admin");
    assert!(token.is_admin);
    let account = membership::load(&home).expect("Rust reads Python membership");
    assert!(account.logged_in);
    assert_eq!(account.device_id.as_deref(), Some("device-python-0435"));
    assert_eq!(
        account.hostname.as_deref(),
        Some("python-0435-fixture.example.test")
    );

    for (relative, expected) in before {
        assert_eq!(
            std::fs::read(root.join(relative)).expect("read fixture after adoption"),
            expected,
            "Rust read unexpectedly rewrote {relative} from {FIXTURE_ROOT}"
        );
    }
}

#[test]
fn rust_reads_the_frozen_python_0435_identity_and_integration_state_without_python() {
    let manifest: Value = serde_json::from_str(include_str!(
        "fixtures/python-0.4.35/f1-identity-integrations/_fixture_manifest.json"
    ))
    .expect("F1 fixture manifest");
    assert_eq!(manifest["wave"], "F1");
    assert_eq!(manifest["product_version"], "0.4.35");
    assert_eq!(manifest["sanitized"], true);

    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("python-home");
    materialize(&root, F0_FILES);
    materialize(&root, F1_FILES);
    let mut before = snapshot(&root, F0_FILES);
    before.extend(snapshot(&root, F1_FILES));
    let home = HomeLayout::from_path(&root).expect("Python home layout");

    let profiles = ProfileStore::new(home.clone()).expect("profile store");
    let profile = profiles
        .get_ref("fixture-profile", "global", "")
        .expect("profile lookup")
        .expect("Python profile");
    assert_eq!(profile["runtime"], "codex");
    assert_eq!(
        profiles
            .secret_values_ref("fixture-profile", "global", "")
            .expect("profile secrets")["PRIVATE_KEY"],
        "fixture-private"
    );
    let private_keys = request(
        &home,
        "actor_env_private_keys",
        json!({"group_id":"g_python0435","actor_id":"peer1","by":"user"}),
    );
    assert_eq!(private_keys["keys"], json!(["ACTOR_TOKEN"]));

    let groups = GroupStore::new(home.clone()).expect("group store");
    let group = groups.load("g_python0435").expect("Python F1 group");
    assert_eq!(
        group.extra["web_model_browser_targets"]["web1"]["kind"],
        "existing_chat"
    );
    let connectors = web_model_connectors::load(&home).expect("Python connector store");
    let connector = connectors
        .iter()
        .find(|item| item["connector_id"] == "wmc_fixture")
        .expect("fixture connector");
    assert!(!connector["revoked"].as_bool().unwrap_or(true));
    assert!(web_model_connectors::secret_matches(
        connector,
        "wmcs_fixture_secret"
    ));

    let bridge = group_bridge_legacy::load(&home).expect("Python Group Bridge state");
    assert_eq!(bridge["trusts"][0]["status"], "active");
    assert_eq!(bridge["deliveries"][0]["source_event_id"], "fixture-source");
    let im = im_state::load(&groups, "g_python0435").expect("Python IM state");
    assert_eq!(im["config"]["platform"], "slack");
    assert!(im["config"].get("skip_pending_on_start").is_none());
    assert_eq!(im["authorized"][0]["thread_id"], "1710000000.100");
    assert_eq!(im["subscribers"][0]["subscribed"], true);

    let nomcp = nomcp::Store::new(home.clone()).expect("No-MCP store");
    let session = nomcp
        .authorize("nomcp_fixture0001", "nomcps_fixture_secret")
        .expect("Python No-MCP session");
    assert_eq!(session.group_id, "g_python0435");
    assert!(session.sent_message_ids.contains("fixture-advisory"));

    for (relative, expected) in before {
        assert_eq!(
            std::fs::read(root.join(relative)).expect("read F1 fixture after adoption"),
            expected,
            "Rust read unexpectedly rewrote Python F1 state: {relative}"
        );
    }
}

#[test]
fn rust_reads_the_frozen_python_0435_product_state_without_python() {
    let manifest: Value = serde_json::from_str(include_str!(
        "fixtures/python-0.4.35/f2-product-state/_fixture_manifest.json"
    ))
    .expect("F2 fixture manifest");
    assert_eq!(manifest["wave"], "F2");
    assert_eq!(manifest["product_version"], "0.4.35");
    assert_eq!(manifest["sanitized"], true);

    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("python-home");
    materialize(&root, F0_FILES);
    materialize(&root, F2_FILES);
    let mut before = snapshot(&root, F0_FILES);
    before.extend(snapshot(&root, F2_FILES));
    let home = HomeLayout::from_path(&root).expect("Python home layout");
    let groups = GroupStore::new(home.clone()).expect("group store");

    let assistant =
        assistant_state::load(&home, "g_python0435").expect("Python Voice Secretary state");
    assert_eq!(assistant["assistant"]["lifecycle"], "working");
    assert_eq!(assistant["sessions"][0]["session_id"], "fixture-session");
    assert_eq!(assistant["sessions"][0]["diarization_ready"], true);
    let lease = voice_recording_lease::current(&home).expect("Python recording lease");
    assert_eq!(lease["group_id"], "g_python0435");
    assert_eq!(lease["dispatch_target"], "document");

    let deck = presentation::load(&groups, "g_python0435").expect("Python presentation snapshot");
    assert_eq!(deck.highlight_slot_id, "slot-2");
    assert_eq!(
        deck.slots[1]
            .card
            .as_ref()
            .expect("fixture card")
            .content
            .url,
        Some("https://example.test/fixture".into())
    );
    let automation = request(
        &home,
        "group_automation_state",
        json!({"group_id":"g_python0435","by":"user"}),
    );
    assert_eq!(automation["version"], 2);
    assert_eq!(automation["ruleset"]["rules"][0]["id"], "fixture-reminder");

    let capability = request(
        &home,
        "capability_state",
        json!({"group_id":"g_python0435","actor_id":"peer1","by":"peer1"}),
    );
    assert!(
        capability["enabled_capabilities"]
            .as_array()
            .expect("enabled capabilities")
            .contains(&json!("skill:fixture:enabled"))
    );
    let overview = request(
        &home,
        "capability_overview",
        json!({"group_id":"g_python0435","actor_id":"peer1","by":"peer1"}),
    );
    assert!(
        overview["blocked_capabilities"]
            .as_array()
            .expect("blocked capabilities")
            .iter()
            .any(|item| item["capability_id"] == "skill:fixture:blocked")
    );

    let credential =
        space_credentials::status(&home, "notebooklm").expect("Python NotebookLM credential");
    assert_eq!(credential["source"], "store");
    let space = request(
        &home,
        "group_space_status",
        json!({"group_id":"g_python0435","provider":"notebooklm"}),
    );
    assert_eq!(
        space["bindings"]["work"]["remote_space_id"],
        "nb-fixture-work"
    );
    assert_eq!(
        space["bindings"]["memory"]["remote_space_id"],
        "nb-fixture-memory"
    );
    let jobs = request(
        &home,
        "group_space_jobs",
        json!({"group_id":"g_python0435","provider":"notebooklm","action":"list"}),
    );
    assert_eq!(jobs["jobs"][0]["idempotency_key"], "fixture-job");
    assert_eq!(jobs["jobs"][0]["payload"]["title"], "Fixture sync");
    let deduped = request(
        &home,
        "group_space_ingest",
        json!({
            "group_id":"g_python0435",
            "provider":"notebooklm",
            "lane":"work",
            "kind":"context_sync",
            "payload":{"title":"must not replace the frozen job"},
            "idempotency_key":"fixture-job",
            "by":"user"
        }),
    );
    assert_eq!(deduped["deduped"], true);
    assert_eq!(deduped["accepted"], true);
    assert_eq!(deduped["completed"], false);
    assert_eq!(deduped["job"]["state"], "pending");
    assert!(
        root.join("workspace/docs/voice-secretary/fixture-voice-notes.md")
            .is_file()
    );

    for (relative, expected) in before {
        assert_eq!(
            std::fs::read(root.join(relative)).expect("read F2 fixture after adoption"),
            expected,
            "Rust read unexpectedly rewrote Python F2 state: {relative}"
        );
    }
}

#[test]
fn rust_preserves_python_0435_terminal_state_and_retires_legacy_shadows() {
    let manifest: Value = serde_json::from_str(include_str!(
        "fixtures/python-0.4.35/f3-retired-shadows/_fixture_manifest.json"
    ))
    .expect("F3 fixture manifest");
    assert_eq!(manifest["wave"], "F3");
    assert_eq!(manifest["product_version"], "0.4.35");
    assert_eq!(manifest["sanitized"], true);

    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("python-home");
    materialize(&root, F0_FILES);
    materialize(&root, F3_FILES);
    let before = snapshot(&root, F3_FILES);
    let home = HomeLayout::from_path(&root).expect("Python home layout");
    let groups = GroupStore::new(home.clone()).expect("group store");

    let profiles = ProfileStore::new(home.clone()).expect("profile store");
    assert_eq!(
        profiles.get("retired-profile").expect("profile lookup"),
        None
    );
    let private_keys = request(
        &home,
        "actor_env_private_keys",
        json!({"group_id":"g_python0435","actor_id":"peer1","by":"user"}),
    );
    assert_eq!(private_keys["keys"], json!([]));

    let group = groups.load("g_python0435").expect("terminal group");
    assert!(
        group.extra["web_model_browser_targets"]
            .as_object()
            .is_some_and(Map::is_empty)
    );
    assert!(group.extra.get("group_space").is_none());
    assert!(group.extra.get("im").is_none());
    let im = im_state::load(&groups, "g_python0435").expect("retired IM state");
    assert!(im.get("config").is_none());
    assert_eq!(im["authorized"], json!([]));
    assert_eq!(im["pending"], json!([]));
    assert_eq!(im["subscribers"], json!([]));

    let connectors = web_model_connectors::load(&home).expect("retired connector store");
    assert_eq!(connectors.len(), 1);
    assert_eq!(connectors[0]["connector_id"], "wmc_fixture");
    assert_eq!(connectors[0]["revoked"], true);
    let bridge = group_bridge_legacy::load(&home).expect("retired Group Bridge state");
    assert_eq!(bridge["trusts"][0]["status"], "revoked");
    assert!(
        bridge["registrations"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );
    assert!(
        !std::fs::read_to_string(root.join("group_bridge_credentials.yaml"))
            .expect("credential store")
            .contains("stale-legacy-token")
    );
    let global = settings::load(&home).expect("settings after legacy retirement");
    assert!(!global.extra.contains_key("web_model_connectors"));
    assert!(global.extra.get("group_bridge").is_none_or(Value::is_null));

    let nomcp = nomcp::Store::new(home.clone()).expect("No-MCP store");
    assert!(
        !nomcp
            .get("nomcp_fixture0001")
            .expect("retired session")
            .expect("session record")
            .revoked_at
            .is_empty()
    );
    assert!(
        nomcp
            .authorize("nomcp_fixture0001", "nomcps_fixture_secret")
            .expect_err("revoked session must reject its former secret")
            .to_string()
            .contains("revoked")
    );

    let credential =
        space_credentials::status(&home, "notebooklm").expect("cleared NotebookLM credential");
    assert_eq!(credential["store_configured"], false);
    assert!(!root.join("space-credentials.json").exists());
    assert!(
        !root
            .join("state/secrets/space_providers/notebooklm.3a74eb58101dbfe3.json")
            .exists()
    );
    let space = request(
        &home,
        "group_space_status",
        json!({"group_id":"g_python0435","provider":"notebooklm"}),
    );
    assert_eq!(space["bindings"]["work"]["remote_space_id"], "");
    assert_eq!(space["bindings"]["memory"]["remote_space_id"], "");

    let capability = request(
        &home,
        "capability_state",
        json!({"group_id":"g_python0435","actor_id":"peer1","by":"peer1"}),
    );
    assert!(
        !capability["enabled_capabilities"]
            .as_array()
            .expect("enabled capabilities")
            .contains(&json!("skill:cccc:self-evolution"))
    );

    for (relative, expected) in before {
        if matches!(
            relative,
            "settings.yaml"
                | "web_model_connectors.yaml"
                | "group_bridge_pairing.yaml"
                | "group_bridge_registrations.yaml"
                | "group_bridge_credentials.yaml"
                | "group_bridge_receipts.yaml"
                | "space-credentials.json"
        ) {
            continue;
        }
        assert_eq!(
            std::fs::read(root.join(relative)).expect("read F3 fixture after adoption"),
            expected,
            "Rust terminal-state read unexpectedly rewrote Python F3 state: {relative}"
        );
    }
}
