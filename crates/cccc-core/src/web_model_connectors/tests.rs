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
        "https://name@chatgpt.com/c/a",
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
fn grok_credentials_scope_routes_and_survive_restart_but_not_rebinding() {
    let (_temp, home, chatgpt) = fixture();
    let cid = chatgpt["connector"]["connector_id"]
        .as_str()
        .expect("valid test fixture");
    let chat_binding = bound(&home, cid, "chat", "host-chat");
    let g = configure_provider(&home, "grok_web").expect("valid test fixture");
    let id = g["connector"]["connector_id"]
        .as_str()
        .expect("valid test fixture");
    let url_a = "https://grok.com/bot/1373170d-9cf2-408c-b597-e243e5884f4a";
    let url_b = "https://grok.com/bot/958f2446-013f-4225-8dd3-f146295992d7";
    let a = bind_grok(&home, id, "g_fixture", "a", "gen-a", url_a).expect("valid test fixture");
    let b = bind_grok(&home, id, "g_fixture", "b", "gen-b", url_b).expect("valid test fixture");
    let reload = || {
        load(&home)
            .expect("valid test fixture")
            .into_iter()
            .find(|c| c["connector_id"] == id)
            .expect("valid test fixture")
    };
    let token_a = grok_token(&reload(), &a).expect("valid test fixture");
    let token_b = grok_token(&reload(), &b).expect("valid test fixture");
    assert_ne!(token_a, token_b);
    assert_eq!(binding_for_token(&reload(), &token_a), Some(a.clone()));
    assert_eq!(binding_for_token(&reload(), &token_b), Some(b.clone()));
    assert!(binding_for_token(&chatgpt["connector"], &token_a).is_none());
    assert!(binding_for_token(&reload(), "bad").is_none());
    assert!(session_key(&reload(), &json!({"openai/session":"spoof"})).is_err());
    assert!(begin_pairing(&home, id, "g_fixture", "a", "gen-a", false).is_err());
    assert!(bind_grok(&home, id, "g_fixture", "c", "gen-c", url_a).is_err());
    assert_eq!(
        bind_grok(&home, id, "g_fixture", "a", "gen-a", url_a).expect("valid test fixture"),
        a
    );
    let rotated = configure_provider(&home, "grok_web").expect("valid test fixture");
    assert_eq!(
        grok_token(&rotated["connector"], &a).expect("valid test fixture"),
        token_a
    );
    let snapshots = retire_actor(&home, "g_fixture", "a").expect("valid test fixture");
    assert!(binding_for_token(&reload(), &token_a).is_none());
    assert!(
        !serde_json::to_string(&snapshots)
            .expect("valid test fixture")
            .contains(&token_a)
    );
    restore(&home, &snapshots).expect("valid test fixture");
    assert_eq!(binding_for_token(&reload(), &token_a), Some(a));
    let replacement =
        bind_grok(&home, id, "g_fixture", "a", "gen-new", url_a).expect("valid test fixture");
    assert_ne!(
        grok_token(&reload(), &replacement).expect("valid test fixture"),
        token_a
    );
    assert!(binding_for_token(&reload(), &token_a).is_none());
    let chat = load(&home)
        .expect("valid test fixture")
        .into_iter()
        .find(|c| c["connector_id"] == cid)
        .expect("valid test fixture");
    assert_eq!(binding_for_session(&chat, "host-chat"), Some(chat_binding));
    assert!(secret_matches(
        &chat,
        chatgpt["secret"].as_str().expect("valid test fixture")
    ));
    revoke(&home, id).expect("valid test fixture");
    assert!(binding_for_token(&reload(), &token_b).is_none());
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
        "https://user@grok.com/bot/1373170d-9cf2-408c-b597-e243e5884f4a",
    ] {
        assert!(grok_bot_url(bad).is_err(), "{bad}");
    }
}
