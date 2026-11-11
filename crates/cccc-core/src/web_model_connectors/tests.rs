use super::*;

fn fixture() -> (tempfile::TempDir, HomeLayout, Value) {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let configured = configure(&home).expect("configure");
    (temp, home, configured)
}
fn bound(home: &HomeLayout, id: &str, actor: &str, session: &str) -> Value {
    let pair = begin_pairing(home, id, "g_fixture", actor, "generation-1", false).expect("begin");
    accept_pairing(home, id, pair["code"].as_str().expect("code"), session).expect("accept");
    confirm_pairing(
        home,
        id,
        "g_fixture",
        actor,
        "generation-1",
        pair["pairing_id"].as_str().expect("operation"),
        &format!("https://chatgpt.com/c/conversation-{actor}"),
    )
    .expect("confirm")
}
#[test]
fn old_actor_credentials_require_reconfiguration_without_mutating_history() {
    let (_temp, home, _) = fixture();
    let legacy =
        json!({"connectors":{"old":{"group_id":"g_old","actor_id":"a","secret":"old-secret"}}});
    fs::write_secret_yaml(&store_path(&home), &legacy).expect("old fixture");
    let bytes = std::fs::read(store_path(&home)).expect("read");
    assert!(load(&home).expect("boot").is_empty());
    assert!(requires_reconfiguration(&home).expect("status"));
    assert_eq!(std::fs::read(store_path(&home)).expect("unchanged"), bytes);
    let configured = configure(&home).expect("explicit reconfigure");
    assert!(!secret_matches(&configured["connector"], "old-secret"));
    assert!(!requires_reconfiguration(&home).expect("status"));
}
#[test]
fn rotation_preserves_binding_but_rejects_old_credential_and_stores_no_secret() {
    let (_temp, home, first) = fixture();
    let id = first["connector"]["connector_id"].as_str().expect("id");
    let before = bound(&home, id, "a", "host-a");
    let rotated = configure(&home).expect("rotate");
    assert_eq!(rotated["connector"]["connector_id"], id);
    assert!(!secret_matches(
        &rotated["connector"],
        first["secret"].as_str().expect("old secret")
    ));
    assert!(secret_matches(
        &rotated["connector"],
        rotated["secret"].as_str().expect("secret")
    ));
    assert_eq!(
        binding_for_session(&rotated["connector"], "host-a"),
        Some(before)
    );
    let disk = std::fs::read_to_string(store_path(&home)).expect("store");
    assert!(!disk.contains(rotated["secret"].as_str().expect("secret")));
    assert!(!disk.contains(first["secret"].as_str().expect("secret")));
}
#[test]
fn host_metadata_is_scoped_stable_and_strict() {
    let (_temp, home, c) = fixture();
    let c = &c["connector"];
    let meta = json!({"openai/session":"host-A","openai/subject":"same-user"});
    let a = session_key(c, &meta).expect("key");
    assert_eq!(session_key(c, &meta).expect("same"), a);
    assert_ne!(
        session_key(
            c,
            &json!({"openai/session":"host-B","openai/subject":"same-user"})
        )
        .expect("B"),
        a
    );
    assert_ne!(
        session_key(c, &json!({"openai/session":"host-A"})).expect("no subject"),
        a
    );
    for bad in [
        Value::Null,
        json!({}),
        json!({"openai/session":7}),
        json!({"openai/session":"\n"}),
        json!({"openai/session":"valid","openai/subject":null}),
    ] {
        assert!(session_key(c, &bad).is_err());
    }
    let rotated = configure(&home).expect("rotate");
    assert_eq!(
        session_key(&rotated["connector"], &meta).expect("rotation keeps identity"),
        a
    );
    assert!(!a.contains("host-A"));
}
#[test]
fn host_identity_diagnostics_are_scoped_component_hashes_not_routing_keys() {
    let (_temp, home, configured) = fixture();
    let connector = &configured["connector"];
    let metadata = json!({"openai/session":"synthetic-conversation",
        "openai/subject":"synthetic-user","openai/organization":"synthetic-org",
        "unrelated":"synthetic-private-metadata"});
    let routing = session_key(connector, &metadata).expect("routing key");
    assert_eq!(
        routing,
        hash(
            &json!([
                "cccc-web-model-session-v1",
                connector["connector_id"],
                connector["routing_salt"],
                metadata["openai/session"],
                metadata["openai/subject"],
                metadata["openai/organization"]
            ])
            .to_string()
        )
    );
    let fingerprints = host_identity_fingerprints(connector, &metadata).expect("fingerprints");
    assert_eq!(fingerprints.as_object().expect("object").len(), 3);
    for value in fingerprints.as_object().expect("object").values() {
        assert_eq!(value.as_str().expect("hash").len(), 64);
        assert_ne!(value, &json!(routing));
    }
    assert!(!fingerprints.to_string().contains("synthetic"));
    let mut changed = metadata.clone();
    changed["openai/organization"] = json!("synthetic-other-org");
    let other = host_identity_fingerprints(connector, &changed).expect("changed organization");
    assert_eq!(fingerprints["session"], other["session"]);
    assert_eq!(fingerprints["subject"], other["subject"]);
    assert_ne!(fingerprints["organization"], other["organization"]);
    changed
        .as_object_mut()
        .expect("metadata")
        .remove("openai/subject");
    let absent = host_identity_fingerprints(connector, &changed).expect("missing optional field");
    assert!(absent["subject"].is_null());
    assert_eq!(absent["session"], fingerprints["session"]);
    assert!(host_identity_fingerprints(connector, &json!({})).is_err());
    assert!(host_identity_fingerprints(connector, &json!({"openai/session":7})).is_err());
    let rotated = configure(&home).expect("credential rotation");
    assert_eq!(
        fingerprints,
        host_identity_fingerprints(&rotated["connector"], &metadata).expect("rotation")
    );
    let mut other_connector = connector.clone();
    other_connector["routing_salt"] = json!("synthetic-other-salt");
    let other = host_identity_fingerprints(&other_connector, &metadata).expect("other connector");
    for field in ["session", "subject", "organization"] {
        assert_ne!(fingerprints[field], other[field]);
    }
}

#[test]
fn pairing_is_one_session_and_needs_explicit_confirmation() {
    let (_temp, home, c) = fixture();
    let id = c["connector"]["connector_id"].as_str().expect("id");
    let p = begin_pairing(&home, id, "g_fixture", "a", "generation-1", false).expect("begin");
    let code = p["code"].as_str().expect("code");
    let first = accept_pairing(&home, id, code, "session-a").expect("accept");
    assert_eq!(
        accept_pairing(&home, id, code, "session-a").expect("retry"),
        first
    );
    assert!(accept_pairing(&home, id, code, "session-b").is_err());
    assert!(binding_for_session(&load(&home).expect("load")[0], "session-a").is_none());
    assert!(
        !std::fs::read_to_string(store_path(&home))
            .expect("store")
            .contains(code)
    );
    let pair_id = p["pairing_id"].as_str().expect("id");
    assert!(
        confirm_pairing(
            &home,
            id,
            "g_fixture",
            "a",
            "new-generation",
            pair_id,
            "https://chatgpt.com/c/a"
        )
        .is_err()
    );
    let b = confirm_pairing(
        &home,
        id,
        "g_fixture",
        "a",
        "generation-1",
        pair_id,
        "https://chatgpt.com/c/a",
    )
    .expect("confirm");
    assert_eq!(
        confirm_pairing(
            &home,
            id,
            "g_fixture",
            "a",
            "generation-1",
            pair_id,
            "https://chatgpt.com/c/a"
        )
        .expect("confirm retry"),
        b
    );
    assert!(
        confirm_pairing(
            &home,
            id,
            "g_fixture",
            "a",
            "generation-1",
            pair_id,
            "https://chatgpt.com/c/different"
        )
        .is_err()
    );
    assert!(
        binding_for_actor(
            &load(&home).expect("load")[0],
            "g_fixture",
            "a",
            "new-generation"
        )
        .is_none()
    );
}
#[test]
fn conflicting_conversations_expiration_and_revocation_fail_closed() {
    let (_temp, home, c) = fixture();
    let id = c["connector"]["connector_id"].as_str().expect("id");
    bound(&home, id, "a", "session-a");
    let p = begin_pairing(&home, id, "g_fixture", "b", "generation-1", false).expect("begin");
    let code = p["code"].as_str().expect("code");
    accept_pairing(&home, id, code, "session-a").expect("candidate");
    assert!(
        confirm_pairing(
            &home,
            id,
            "g_fixture",
            "b",
            "generation-1",
            p["pairing_id"].as_str().expect("id"),
            "https://chatgpt.com/c/b"
        )
        .is_err()
    );
    update_connector(&home, id, |c| {
        c["pairings"][route_key("g_fixture", "b")]["expires_at_ms"] = json!(0)
    })
    .expect("expire");
    assert!(accept_pairing(&home, id, code, "session-a").is_err());
    revoke(&home, id).expect("revoke");
    assert!(binding_for_session(&load(&home).expect("load")[0], "session-a").is_none());
    assert!(begin_pairing(&home, id, "g_fixture", "a", "generation-1", false).is_err());
}
#[test]
fn actor_retirement_and_rollback_preserve_other_routes_and_global_credentials() {
    let (_temp, home, c) = fixture();
    let id = c["connector"]["connector_id"].as_str().expect("id");
    let a = bound(&home, id, "a", "session-a");
    let b = bound(&home, id, "b", "session-b");
    let retired = retire_actor(&home, "g_fixture", "a").expect("retire");
    let connector = &load(&home).expect("load")[0];
    assert!(binding_for_session(connector, "session-a").is_none());
    assert_eq!(binding_for_session(connector, "session-b"), Some(b));
    assert!(secret_matches(
        connector,
        c["secret"].as_str().expect("secret")
    ));
    restore(&home, &retired).expect("rollback");
    assert_eq!(
        binding_for_session(&load(&home).expect("load")[0], "session-a"),
        Some(a)
    );
    retire_group(&home, "g_fixture").expect("retire Group");
    assert!(
        load(&home).expect("load")[0]["bindings"]
            .as_object()
            .expect("bindings")
            .is_empty()
    );
}
#[test]
fn conversation_targets_are_native_https_chatgpt_conversations() {
    assert_eq!(
        conversation_url("https://chatgpt.com/g/g-example/c/abc-123?x=1#y").expect("valid"),
        "https://chatgpt.com/g/g-example/c/abc-123"
    );
    for url in [
        "http://chatgpt.com/c/a",
        "https://elsewhere.example/c/a",
        "https://chatgpt.com/",
        "https://chatgpt.com/c/WEB:pending",
        "https://name\u{40}chatgpt.com/c/a",
    ] {
        assert!(conversation_url(url).is_err(), "{url}");
    }
}

#[test]
fn pairing_receipt_and_failure_are_scoped_to_one_attempt() {
    let (_temp, home, c) = fixture();
    let id = c["connector"]["connector_id"].as_str().expect("id");
    let first = begin_pairing(&home, id, "g_fixture", "a", "gen", false).expect("begin");
    let code = first["code"].as_str().expect("code");
    let accepted = accept_pairing(&home, id, code, "session-a").expect("accept");
    assert!(
        accepted["receipt"]
            .as_str()
            .expect("receipt")
            .starts_with("CCCC_PAIR_RECEIPT_")
    );
    assert_eq!(
        accept_pairing(&home, id, code, "session-a").expect("retry")["receipt"],
        accepted["receipt"]
    );
    assert!(accept_pairing(&home, id, code, "session-b").is_err());
    let old_id = first["pairing_id"].as_str().expect("id");
    fail_pairing(&home, id, "g_fixture", "a", old_id, "pairing_timeout").expect("fail");
    assert!(accept_pairing(&home, id, code, "session-a").is_err());
    assert!(
        confirm_pairing(
            &home,
            id,
            "g_fixture",
            "a",
            "gen",
            old_id,
            "https://chatgpt.com/c/a"
        )
        .is_err()
    );
    let second =
        begin_pairing(&home, id, "g_fixture", "a", "gen", false).expect("retry explicitly");
    assert!(cancel_pairing(&home, id, "g_fixture", "a", old_id).is_err());
    assert!(fail_pairing(&home, id, "g_fixture", "a", old_id, "pairing_interrupted").is_err());
    let new_id = second["pairing_id"].as_str().expect("id");
    accept_pairing(
        &home,
        id,
        second["code"].as_str().expect("code"),
        "session-a",
    )
    .expect("accept");
    confirm_pairing(
        &home,
        id,
        "g_fixture",
        "a",
        "gen",
        new_id,
        "https://chatgpt.com/c/a",
    )
    .expect("confirm");
    assert!(fail_pairing(&home, id, "g_fixture", "a", new_id, "pairing_timeout").is_err());
    cancel_pairing(&home, id, "g_fixture", "a", new_id).expect("late cancel does not unbind");
    assert!(binding_for_session(&load(&home).expect("store")[0], "session-a").is_some());
}

#[test]
fn automatic_attempt_fences_survive_expiry_cancellation_and_other_actor_setup() {
    let (_temp, home, configured) = fixture();
    let id = configured["connector"]["connector_id"]
        .as_str()
        .expect("id");
    let a = begin_pairing(&home, id, "g", "a", "gen", true).expect("A");
    update_connector(&home, id, |c| {
        c["pairings"][route_key("g", "a")]["expires_at_ms"] = json!(0)
    })
    .expect("expire A");
    let b = begin_pairing(&home, id, "g", "b", "gen", true).expect("B");
    let current = load(&home).expect("store").remove(0);
    assert_eq!(
        pairing_for_actor(&current, "g", "a").expect("expiry fence")["pairing_id"],
        a["pairing_id"]
    );
    assert!(accept_pairing(&home, id, a["code"].as_str().expect("code"), "host-a").is_err());
    cancel_pairing(&home, id, "g", "b", b["pairing_id"].as_str().expect("id")).expect("cancel");
    assert!(accept_pairing(&home, id, b["code"].as_str().expect("code"), "host-b").is_err());
    assert_eq!(
        pairing_for_actor(&load(&home).expect("store")[0], "g", "b").expect("cancel fence")["state"],
        "cancelled"
    );
    let c = begin_pairing(&home, id, "g", "c", "gen", true).expect("C");
    accept_pairing(&home, id, c["code"].as_str().expect("code"), "host-c").expect("accept C");
    let d = begin_pairing(&home, id, "other-group", "d", "gen", true).expect("D");
    let manual = begin_pairing(&home, id, "g", "manual", "gen", false).expect("manual");
    interrupt_automatic_pairings(&home, "g", None).expect("pause");
    assert!(
        confirm_pairing(
            &home,
            id,
            "g",
            "c",
            "gen",
            c["pairing_id"].as_str().expect("id"),
            "https://chatgpt.com/c/c"
        )
        .is_err()
    );
    let current = load(&home).expect("store").remove(0);
    assert_eq!(
        pairing_for_actor(&current, "g", "c").expect("C")["error_code"],
        "pairing_interrupted"
    );
    assert_eq!(
        pairing_for_actor(&current, "g", "b").expect("B")["state"],
        "cancelled"
    );
    assert!(accept_pairing(&home, id, d["code"].as_str().expect("code"), "host-d").is_ok());
    assert!(
        accept_pairing(
            &home,
            id,
            manual["code"].as_str().expect("code"),
            "host-manual"
        )
        .is_ok()
    );
}

#[test]
fn grok_bindings_are_native_and_isolate_provider_routes() {
    let (_temp, home, chatgpt) = fixture();
    let cid = chatgpt["connector"]["connector_id"]
        .as_str()
        .expect("connector");
    let chat_binding = bound(&home, cid, "chat", "host-chat");
    let configured = configure_provider(&home, "grok_web").expect("Grok connector");
    let id = configured["connector"]["connector_id"]
        .as_str()
        .expect("Grok ID");
    let url = "https://grok.com/bot/00000000-0000-4000-8000-000000000001";
    let original = bind_grok(&home, id, "g_fixture", "bot", "generation", url).expect("bind");
    assert_eq!(original["transport"], "grok_webhook");
    assert_eq!(
        bind_grok(&home, id, "g_fixture", "bot", "generation", url).expect("same binding"),
        original
    );
    assert!(bind_grok(&home, id, "g_fixture", "other", "generation", url).is_err());
    assert!(begin_pairing(&home, id, "g_fixture", "bot", "generation", false).is_err());
    let snapshot = retire_actor(&home, "g_fixture", "bot").expect("retire");
    restore(&home, &snapshot).expect("restore");
    let connectors = load(&home).expect("connectors");
    let chat = connectors
        .iter()
        .find(|c| c["connector_id"] == cid)
        .expect("ChatGPT");
    assert_eq!(binding_for_session(chat, "host-chat"), Some(chat_binding));
    let grok = connectors
        .iter()
        .find(|c| c["connector_id"] == id)
        .expect("Grok");
    assert_eq!(
        binding_for_actor(grok, "g_fixture", "bot", "generation"),
        Some(original.clone())
    );
    let replacement =
        bind_grok(&home, id, "g_fixture", "bot", "new-generation", url).expect("replace");
    assert_ne!(replacement["revision"], original["revision"]);
}

#[test]
fn v2_chatgpt_routes_and_credentials_survive_provider_store_upgrade() {
    let (_temp, home, c) = fixture();
    let id = c["connector"]["connector_id"]
        .as_str()
        .expect("valid test fixture");
    bound(&home, id, "a", "host-a");
    let original = load(&home).expect("valid test fixture").remove(0);
    fs::write_secret_yaml(
        &store_path(&home),
        &json!({"version":2,"connector":original}),
    )
    .expect("valid test fixture");
    configure_provider(&home, "grok_web").expect("valid test fixture");
    let upgraded = load(&home).expect("valid test fixture");
    assert_eq!(upgraded.len(), 2);
    assert_eq!(
        upgraded.iter().find(|c| c["connector_id"] == id),
        Some(&original)
    );
}

#[test]
fn grok_url_requires_exact_provider_and_bot() {
    let url = "https://grok.com/bot/1373170d-9cf2-408c-b597-e243e5884f4a";
    assert_eq!(
        grok_bot_url(&format!("{url}?x=1#other")).expect("valid test fixture"),
        url
    );
    for bad in [
        "http://grok.com/bot/1373170d-9cf2-408c-b597-e243e5884f4a",
        "https://grok.com/",
        "https://grok.com/bot/no",
        "https://grok.com/bot/1373170d-9cf2-408c-b597-e243e5884f4a/extra",
        "https://evil.grok.com/bot/1373170d-9cf2-408c-b597-e243e5884f4a",
        "https://user\u{40}grok.com/bot/1373170d-9cf2-408c-b597-e243e5884f4a",
    ] {
        assert!(grok_bot_url(bad).is_err(), "{bad}");
    }
}

#[test]
fn create_if_absent_never_rotates_an_active_connector() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let created = create_provider_if_absent(&home, "chatgpt_web")
        .expect("create")
        .expect("absent, so created");
    assert!(
        created["secret"]
            .as_str()
            .is_some_and(|secret| secret.starts_with("wmcs_"))
    );
    let before = load(&home).expect("load");
    // A second one-click create, e.g. from a stale page, changes nothing.
    assert!(
        create_provider_if_absent(&home, "chatgpt_web")
            .expect("check")
            .is_none()
    );
    assert_eq!(load(&home).expect("load"), before);
    // After revocation it may create a fresh connector again.
    let id = created["connector"]["connector_id"].as_str().expect("id");
    assert!(revoke(&home, id).expect("revoke"));
    assert!(
        create_provider_if_absent(&home, "chatgpt_web")
            .expect("create")
            .is_some()
    );
}

fn store_bytes(home: &HomeLayout) -> Vec<u8> {
    std::fs::read(store_path(home)).expect("store readable")
}
fn live(home: &HomeLayout) -> Value {
    load(home)
        .expect("load")
        .into_iter()
        .find(|c| c["provider"] == "chatgpt_web")
        .expect("chatgpt connector")
}
fn confirmation(home: &HomeLayout) -> String {
    rotation_preview(&live(home))["confirmation"]
        .as_str()
        .expect("confirmation")
        .to_owned()
}

#[test]
fn flagless_create_never_changes_secret_hash_or_generation() {
    let (_temp, home, first) = fixture();
    let secret = first["secret"].as_str().expect("secret");
    let before = store_bytes(&home);
    for _ in 0..3 {
        assert!(
            create_provider_if_absent(&home, "chatgpt_web")
                .expect("call")
                .is_none()
        );
    }
    assert_eq!(
        store_bytes(&home),
        before,
        "a non-rotating call writes nothing"
    );
    let c = live(&home);
    assert!(secret_matches(&c, secret));
    assert_eq!(c["credential_generation"], 1);
    assert!(c["secret_rotated_at"].is_null());
}

#[test]
fn rotation_with_live_confirmation_replaces_only_the_credential() {
    let (_temp, home, first) = fixture();
    let id = first["connector"]["connector_id"]
        .as_str()
        .expect("id")
        .to_owned();
    let old_secret = first["secret"].as_str().expect("secret").to_owned();
    let binding = bound(&home, &id, "actor-a", "session-a");
    let before = live(&home);

    let rotated =
        rotate_provider(&home, "chatgpt_web", &confirmation(&home), "admin:test").expect("rotate");
    let after = live(&home);
    let new_secret = rotated["secret"].as_str().expect("secret");
    assert!(secret_matches(&after, new_secret));
    assert!(!secret_matches(&after, &old_secret));
    assert_eq!(after["connector_id"], before["connector_id"]);
    assert_eq!(after["routing_salt"], before["routing_salt"]);
    assert_eq!(after["bindings"], before["bindings"]);
    assert_eq!(after["pairings"], before["pairings"]);
    assert_eq!(after["credential_generation"], 2);
    assert_eq!(rotated["previous_generation"], 1);
    assert_eq!(after["rotated_by"], "admin:test");
    assert!(after["secret_rotated_at"].is_string());
    assert!(binding_for_actor(&after, "g_fixture", "actor-a", "generation-1").is_some());
    assert_eq!(binding["state"], "bound");
    let audit = after["events_log"]
        .as_array()
        .expect("log")
        .last()
        .cloned()
        .expect("entry");
    assert_eq!(audit["reason"], "secret_rotated");
    assert_eq!(audit["recorded_binding_count"], 1);
    assert!(!String::from_utf8_lossy(&store_bytes(&home)).contains(new_secret));
}

#[test]
fn stale_confirmation_refuses_and_writes_nothing() {
    let (_temp, home, first) = fixture();
    let id = first["connector"]["connector_id"]
        .as_str()
        .expect("id")
        .to_owned();
    let stale = confirmation(&home);
    // A binding appears between preview and click.
    bound(&home, &id, "actor-b", "session-b");
    let before = store_bytes(&home);
    let err = rotate_provider(&home, "chatgpt_web", &stale, "admin:test").expect_err("stale");
    assert_eq!(err.to_string(), "rotation_confirmation_stale");
    assert_eq!(store_bytes(&home), before);
    assert!(secret_matches(
        &live(&home),
        first["secret"].as_str().expect("secret")
    ));
    // Garbage and empty fences are refused the same way.
    for bad in ["", "not-a-hash"] {
        assert_eq!(
            rotate_provider(&home, "chatgpt_web", bad, "admin:test")
                .expect_err("refused")
                .to_string(),
            "rotation_confirmation_stale"
        );
    }
    assert_eq!(store_bytes(&home), before);
}

#[test]
fn a_confirmation_cannot_be_replayed_and_generations_stay_distinct() {
    let (_temp, home, _) = fixture();
    let first = confirmation(&home);
    rotate_provider(&home, "chatgpt_web", &first, "admin:test").expect("first rotation");
    // Same confirmation again (a retried request) must not rotate twice.
    let err = rotate_provider(&home, "chatgpt_web", &first, "admin:test").expect_err("replay");
    assert_eq!(err.to_string(), "rotation_confirmation_stale");
    // Two rotations inside the same clock tick still differ by generation.
    let second = confirmation(&home);
    assert_ne!(first, second);
    rotate_provider(&home, "chatgpt_web", &second, "admin:test").expect("second rotation");
    assert_eq!(live(&home)["credential_generation"], 3);
}

#[test]
fn rotation_without_an_active_connector_creates_nothing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let err = rotate_provider(&home, "chatgpt_web", "anything", "admin:test").expect_err("none");
    assert_eq!(err.to_string(), "connector_unavailable");
    assert!(load(&home).expect("load").is_empty());
    // Same after revocation: rotate must not resurrect a revoked connector.
    let created = create_provider_if_absent(&home, "chatgpt_web")
        .expect("create")
        .expect("new");
    let id = created["connector"]["connector_id"].as_str().expect("id");
    assert!(revoke(&home, id).expect("revoke"));
    let err = rotate_provider(&home, "chatgpt_web", "anything", "admin:test").expect_err("revoked");
    assert_eq!(err.to_string(), "connector_unavailable");
    assert_eq!(
        rotate_provider(&home, "chatgpt_web", "x", " ")
            .expect_err("by")
            .to_string(),
        "rotated_by_required"
    );
}

#[test]
fn legacy_connector_reports_unrecorded_generation_until_it_rotates() {
    let (_temp, home, _) = fixture();
    let id = live(&home)["connector_id"].as_str().expect("id").to_owned();
    update_connector(&home, &id, |c| {
        c.as_object_mut()
            .expect("connector")
            .remove("credential_generation");
    })
    .expect("strip");
    let preview = rotation_preview(&live(&home));
    assert_eq!(preview["credential_generation"], UNRECORDED_GENERATION);
    assert!(preview["secret_rotated_at"].is_null());
    rotate_provider(
        &home,
        "chatgpt_web",
        preview["confirmation"].as_str().expect("c"),
        "admin:test",
    )
    .expect("rotate");
    assert_eq!(live(&home)["credential_generation"], 1);
}

#[test]
fn rejected_secret_is_recorded_bounded_and_without_request_material() {
    let (_temp, home, first) = fixture();
    let id = first["connector"]["connector_id"]
        .as_str()
        .expect("id")
        .to_owned();
    let record = record_rejected_secret(&home, &id, "post_path_token").expect("record");
    assert_eq!(record, RejectionRecord::Written { count: 1 });
    let entry = live(&home)["rejection_log"]
        .as_array()
        .expect("log")
        .last()
        .cloned()
        .expect("entry");
    assert_eq!(entry["reason"], "invalid_secret");
    assert_eq!(entry["outcome"], "denied");
    assert_eq!(entry["transport"], "post_path_token");
    assert_eq!(entry["count"], 1);
    assert_eq!(
        entry["credential_generation"],
        live(&home)["credential_generation"]
    );
    for forbidden in [
        "secret",
        "secret_hash",
        "url",
        "authorization",
        "body",
        "actor_id",
    ] {
        assert!(
            entry.get(forbidden).is_none(),
            "{forbidden} must not be stored"
        );
    }
    // Within the write interval further hits are deferred, not written.
    assert_eq!(
        record_rejected_secret(&home, &id, "get_probe").expect("record"),
        RejectionRecord::Deferred
    );
    // Unknown connector: no write, no throttle entry.
    let before = store_bytes(&home);
    assert_eq!(
        record_rejected_secret(&home, "wmc_does_not_exist", "get_probe").expect("record"),
        RejectionRecord::UnknownConnector
    );
    assert_eq!(store_bytes(&home), before);
}

#[test]
fn rejection_log_cannot_evict_rotation_and_prunes_revoked_or_removed_ids() {
    let (_temp, home, first) = fixture();
    let id = first["connector"]["connector_id"]
        .as_str()
        .expect("id")
        .to_owned();
    rotate_provider(&home, "chatgpt_web", &confirmation(&home), "local:user").expect("rotate");
    let rotation_log = live(&home)["events_log"].clone();
    // Controlled throttle resets, around entries produced by the real recorder.
    for _ in 0..15 {
        if let Some(m) = REJECTIONS.get() {
            m.lock()
                .expect("throttle")
                .entries
                .remove(&(home.root().to_path_buf(), id.clone()));
        }
        record_rejected_secret(&home, &id, "post_header").expect("record");
    }
    let c = live(&home);
    assert_eq!(c["events_log"], rotation_log);
    assert_eq!(c["events_log"][0]["method"], "secret_rotated");
    assert_eq!(c["events_log"][0]["ok"], true);
    assert_eq!(c["rejection_log"].as_array().expect("log").len(), 10);
    assert!(revoke(&home, &id).expect("revoke"));
    let key = (home.root().to_path_buf(), id);
    assert!(
        !REJECTIONS
            .get()
            .expect("throttle")
            .lock()
            .expect("lock")
            .entries
            .contains_key(&key)
    );
    let next = create_provider_if_absent(&home, "chatgpt_web")
        .expect("create")
        .expect("new");
    let next_id = next["connector"]["connector_id"]
        .as_str()
        .expect("id")
        .to_owned();
    record_rejected_secret(&home, &next_id, "get_probe").expect("record");
    update(&home, |root| {
        root["connectors"]
            .as_object_mut()
            .expect("map")
            .remove("chatgpt_web");
        Ok(())
    })
    .expect("remove");
    assert!(
        !REJECTIONS
            .get()
            .expect("throttle")
            .lock()
            .expect("lock")
            .entries
            .contains_key(&(home.root().to_path_buf(), next_id))
    );
}

#[test]
fn rejection_throttle_coalesces_and_isolates_homes() {
    let (_temp, home, first) = fixture();
    let id = first["connector"]["connector_id"].as_str().expect("id");
    let mut throttle = RejectionThrottle::default();
    let now = Instant::now();
    assert_eq!(throttle.admit(&home, id, now), Some(1));
    assert_eq!(
        throttle.admit(&home, id, now + Duration::from_secs(1)),
        None
    );
    assert_eq!(
        throttle.admit(&home, id, now + Duration::from_secs(2)),
        None
    );
    assert_eq!(
        throttle.admit(&home, id, now + Duration::from_secs(5)),
        Some(3)
    );
    let (_other_temp, other_home, _) = fixture();
    assert_eq!(throttle.admit(&other_home, id, now), Some(1));
}
