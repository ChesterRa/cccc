// Included by the crate-level integration test harness.
use cccc_contracts::{DaemonRequest, DaemonResponse};
use cccc_core::HomeLayout;
use serde_json::{Map, Value, json};

#[test]
fn im_auth_operations_use_python_compatible_state_and_standard_results() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(&home, "group_create", json!({"title":"IM auth"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    let store = cccc_core::GroupStore::new(home.clone()).expect("store");
    store
        .mutate(group_id, |group| {
            group.extra.insert(
                "im".into(),
                json!({"platform":"telegram","bot_token_env":"TOKEN","enabled":false}),
            );
            Ok(())
        })
        .expect("config");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs_f64();
    std::fs::write(
        store
            .state_dir(group_id)
            .expect("state dir")
            .join("im_pending_keys.json"),
        serde_json::to_vec_pretty(&json!({
            "fresh-key":{
                "chat_id":"chat-1","thread_id":9,"platform":"telegram","created_at":now
            },
            "expired-key":{
                "chat_id":"old","thread_id":0,"platform":"telegram","created_at":now-1200.0
            }
        }))
        .expect("pending json"),
    )
    .expect("pending fixture");

    let pending = call(&home, "im_list_pending", json!({"group_id":group_id}));
    assert_eq!(
        pending.result["pending"].as_array().expect("pending").len(),
        1
    );
    assert_eq!(pending.result["pending"][0]["key"], "fresh-key");
    assert!(pending.result["pending"][0]["expires_in_seconds"].is_number());

    let bound = call(
        &home,
        "im_bind_chat",
        json!({"group_id":group_id,"key":"fresh-key"}),
    );
    assert_eq!(
        Value::Object(bound.result),
        json!({"chat_id":"chat-1","thread_id":9,"platform":"telegram"})
    );
    let state = cccc_core::im_state::load(&store, group_id).expect("state after bind");
    assert_eq!(state["pending"], json!([]));
    assert_eq!(state["authorized"][0]["chat_id"], "chat-1");
    assert_eq!(state["subscribers"][0]["subscribed"], true);

    let revoked = call(
        &home,
        "im_revoke_chat",
        json!({"group_id":group_id,"chat_id":"chat-1","thread_id":9}),
    );
    assert_eq!(
        Value::Object(revoked.result),
        json!({"revoked":true,"unsubscribed":true})
    );
    let state = cccc_core::im_state::load(&store, group_id).expect("state after revoke");
    assert_eq!(state["authorized"], json!([]));
    assert_eq!(state["subscribers"][0]["subscribed"], false);

    let invalid = raw_call(
        &home,
        "im_bind_chat",
        json!({"group_id":group_id,"key":"expired-key"}),
    );
    assert_eq!(invalid.error.expect("invalid key").code, "invalid_key");

    let missing_key = raw_call(&home, "im_bind_chat", json!({"group_id":group_id}));
    assert_eq!(missing_key.error.expect("missing key").code, "missing_key");
    let missing_group = raw_call(&home, "im_list_pending", json!({}));
    assert_eq!(
        missing_group.error.expect("missing group id").code,
        "missing_group_id"
    );
    let unknown_group = raw_call(&home, "im_list_authorized", json!({"group_id":"g_missing"}));
    assert_eq!(
        unknown_group.error.expect("unknown group").code,
        "group_not_found"
    );
    let missing_chat = raw_call(&home, "im_revoke_chat", json!({"group_id":group_id}));
    assert_eq!(
        missing_chat.error.expect("missing chat id").code,
        "missing_chat_id"
    );
}
#[test]
fn actor_remove_ledger_failure_restores_authority_before_runtime_cleanup() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(&home, "group_create", json!({"title":"remove rollback"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"web1",
            "runtime":"web_model",
            "env_private":{"TOKEN":"restore-me"},
            "by":"user"
        }),
    );
    let connector = json!({
        "connector_id":"wmc_remove_rollback",
        "group_id":group_id,
        "actor_id":"web1",
        "provider":"chatgpt",
        "secret":"wmcs_remove_rollback",
        "created_at":"2026-08-12T00:00:00Z",
        "updated_at":"2026-08-12T00:00:00Z",
        "revoked":false
    });
    cccc_core::web_model_connectors::replace_active(&home, &connector).expect("connector");

    let store = cccc_core::GroupStore::new(home.clone()).expect("store");
    let headless_state = store
        .state_dir(group_id)
        .expect("state dir")
        .join("runners/headless/web1.json");
    std::fs::create_dir_all(headless_state.parent().expect("headless parent"))
        .expect("headless directory");
    std::fs::write(&headless_state, b"{\"status\":\"working\"}\n").expect("headless fixture");

    let ledger = store.ledger_path(group_id).expect("ledger path");
    std::fs::remove_file(&ledger).expect("remove ledger");
    std::fs::create_dir(&ledger).expect("replace ledger with directory");
    let failed = raw_call(
        &home,
        "actor_remove",
        json!({"group_id":group_id,"actor_id":"web1","by":"user"}),
    );

    assert!(!failed.ok, "corrupt ledger unexpectedly accepted removal");
    assert!(
        store
            .load(group_id)
            .expect("restored group")
            .actors
            .iter()
            .any(|actor| actor.id == "web1")
    );
    assert!(
        headless_state.is_file(),
        "runtime state was cleaned before commit"
    );
    let connectors = cccc_core::web_model_connectors::load(&home).expect("connectors");
    assert_eq!(
        connectors
            .iter()
            .find(|entry| entry["connector_id"] == "wmc_remove_rollback")
            .expect("restored connector")["revoked"],
        json!(false)
    );
    let secret_keys = call(
        &home,
        "actor_env_private_keys",
        json!({"group_id":group_id,"actor_id":"web1","by":"user"}),
    );
    assert_eq!(secret_keys.result["keys"], json!(["TOKEN"]));
}

#[test]
fn actor_stop_ledger_failure_restores_enabled_state() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(&home, "group_create", json!({"title":"stop rollback"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"web1",
            "runtime":"web_model",
            "runner":"headless",
            "by":"user"
        }),
    );

    let store = cccc_core::GroupStore::new(home.clone()).expect("store");
    let before = store.load(group_id).expect("group before stop");
    assert!(before.actors[0].enabled);
    let ledger = store.ledger_path(group_id).expect("ledger path");
    std::fs::remove_file(&ledger).expect("remove ledger");
    std::fs::create_dir(&ledger).expect("replace ledger with directory");

    let failed = raw_call(
        &home,
        "actor_stop",
        json!({"group_id":group_id,"actor_id":"web1","by":"user"}),
    );

    assert!(!failed.ok, "corrupt ledger unexpectedly accepted stop");
    let restored = store.load(group_id).expect("restored group");
    assert!(restored.actors[0].enabled);
    assert_eq!(restored.running, before.running);
    assert_eq!(restored.state, before.state);
}

#[test]
fn actor_add_validates_private_env_before_persisting_actor() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(
        &home,
        "group_create",
        json!({"title":"private env validation"}),
    );
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");

    let invalid_key = raw_call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"invalid-key",
            "runtime":"codex",
            "runner":"headless",
            "env_private":{"BAD-KEY":"secret"},
            "by":"user"
        }),
    );
    assert_eq!(
        invalid_key.error.expect("invalid key rejected").code,
        "invalid_args"
    );

    let too_large = raw_call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"large-secret",
            "runtime":"codex",
            "runner":"headless",
            "env_private":{"TOKEN":"x".repeat(200_001)},
            "by":"user"
        }),
    );
    assert_eq!(
        too_large.error.expect("oversized value rejected").code,
        "invalid_args"
    );

    let group = cccc_core::GroupStore::new(home).expect("store");
    assert!(
        group
            .load(group_id)
            .expect("group")
            .actors
            .iter()
            .all(|actor| actor.id != "invalid-key" && actor.id != "large-secret")
    );
}

#[test]
fn actor_add_rejects_private_env_from_foreman() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(
        &home,
        "group_create",
        json!({"title":"private env authority"}),
    );
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"lead",
            "runtime":"codex",
            "runner":"headless",
            "by":"user"
        }),
    );

    let denied = raw_call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"peer1",
            "runtime":"codex",
            "runner":"headless",
            "env_private":{"TOKEN":"secret"},
            "by":"lead"
        }),
    );

    assert!(!denied.ok, "foreman unexpectedly persisted private env");
    assert_eq!(
        denied.error.expect("permission error").code,
        "permission_denied"
    );
    let group = cccc_core::GroupStore::new(home)
        .expect("store")
        .load(group_id)
        .expect("group");
    assert!(group.actors.iter().all(|actor| actor.id != "peer1"));
}
#[test]
fn actor_removal_retires_connectors_after_the_runtime_changes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(
        &home,
        "group_create",
        json!({"title":"connector retirement"}),
    );
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"former-web",
            "runtime":"custom",
            "runner":"pty",
            "command":["sh"],
            "enabled":false,
            "by":"user"
        }),
    );
    let connector = json!({
        "connector_id":"wmc_former_web",
        "group_id":group_id,
        "actor_id":"former-web",
        "provider":"chatgpt",
        "secret":"wmcs_former_web",
        "created_at":"2026-08-12T00:00:00Z",
        "updated_at":"2026-08-12T00:00:00Z",
        "revoked":false
    });
    cccc_core::web_model_connectors::replace_active(&home, &connector)
        .expect("historical connector");

    call(
        &home,
        "actor_remove",
        json!({"group_id":group_id,"actor_id":"former-web","by":"user"}),
    );

    let connectors = cccc_core::web_model_connectors::load(&home).expect("connectors");
    assert_eq!(
        connectors
            .iter()
            .find(|entry| entry["connector_id"] == "wmc_former_web")
            .expect("retired connector")["revoked"],
        json!(true)
    );
}

#[test]
fn actor_private_env_requires_user_and_an_existing_custom_actor() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(&home, "group_create", json!({"title":"actor secrets"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"peer1",
            "runtime":"custom",
            "env_private":{"TOKEN":"owner-secret"},
            "by":"user"
        }),
    );

    for response in [
        raw_call(
            &home,
            "actor_env_private_keys",
            json!({"group_id":group_id,"actor_id":"peer1","by":"peer1"}),
        ),
        raw_call(
            &home,
            "actor_env_private_update",
            json!({
                "group_id":group_id,
                "actor_id":"peer1",
                "by":"peer1",
                "set":{"PWNED":"peer-write"}
            }),
        ),
    ] {
        assert_eq!(
            response.error.expect("peer denial").code,
            "permission_denied"
        );
    }

    for response in [
        raw_call(
            &home,
            "actor_env_private_keys",
            json!({"group_id":group_id,"actor_id":"future","by":"user"}),
        ),
        raw_call(
            &home,
            "actor_env_private_update",
            json!({
                "group_id":group_id,
                "actor_id":"future",
                "by":"user",
                "set":{"INJECTED":"before-actor-exists"}
            }),
        ),
    ] {
        assert_eq!(
            response.error.expect("missing actor").code,
            "actor_not_found"
        );
    }
    let user_keys = call(
        &home,
        "actor_env_private_keys",
        json!({"group_id":group_id,"actor_id":"peer1","by":"user"}),
    );
    assert_eq!(user_keys.result["group_id"], group_id);
    assert_eq!(user_keys.result["keys"], json!(["TOKEN"]));

    call(
        &home,
        "actor_profile_upsert",
        json!({"profile_id":"linked-profile","name":"Linked","runtime":"codex"}),
    );
    call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"linked",
            "profile_id":"linked-profile",
            "by":"user"
        }),
    );
    for response in [
        raw_call(
            &home,
            "actor_env_private_keys",
            json!({"group_id":group_id,"actor_id":"linked","by":"user"}),
        ),
        raw_call(
            &home,
            "actor_env_private_update",
            json!({
                "group_id":group_id,
                "actor_id":"linked",
                "by":"user",
                "set":{"TOKEN":"denied"}
            }),
        ),
    ] {
        assert_eq!(
            response.error.expect("linked actor denial").code,
            "actor_profile_linked_readonly"
        );
    }

    call(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"future","runtime":"custom","by":"user"}),
    );
    let future = call(
        &home,
        "actor_env_private_keys",
        json!({"group_id":group_id,"actor_id":"future","by":"user"}),
    );
    assert_eq!(future.result["keys"], json!([]));

    call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"reused",
            "runtime":"custom",
            "env_private":{"OLD_TOKEN":"old-generation"},
            "by":"user"
        }),
    );
    let secret_dir = home.root().join("state/secrets/actors").join(group_id);
    let residual_path = std::fs::read_dir(&secret_dir)
        .expect("secret directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
                && std::fs::read_to_string(path).is_ok_and(|text| text.contains("OLD_TOKEN"))
        })
        .expect("reused actor secret");
    call(
        &home,
        "actor_remove",
        json!({"group_id":group_id,"actor_id":"reused","by":"user"}),
    );
    std::fs::write(&residual_path, b"{\"OLD_TOKEN\":\"old-generation\"}\n")
        .expect("residual secret fixture");
    call(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"reused","runtime":"custom","by":"user"}),
    );
    let reused = call(
        &home,
        "actor_env_private_keys",
        json!({"group_id":group_id,"actor_id":"reused","by":"user"}),
    );
    assert_eq!(reused.result["keys"], json!([]));
}

#[test]
fn actor_scope_paths_are_persisted_as_attached_scope_keys() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let project = temp.path().join("project");
    std::fs::create_dir(&project).expect("project");
    let created = call(&home, "group_create", json!({"title":"scoped actors"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    let attached = call(
        &home,
        "attach",
        json!({"group_id":group_id,"path":project,"by":"user"}),
    );
    let scope_key = attached.result["scope_key"].as_str().expect("scope key");

    let added = call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"peer",
            "default_scope_key":project,
            "by":"user"
        }),
    );
    assert_eq!(added.result["actor"]["default_scope_key"], scope_key);

    let updated = call(
        &home,
        "actor_update",
        json!({
            "group_id":group_id,
            "actor_id":"peer",
            "patch":{"default_scope_key":project},
            "by":"user"
        }),
    );
    assert_eq!(updated.result["actor"]["default_scope_key"], scope_key);
    assert_eq!(
        cccc_core::GroupStore::new(home.clone())
            .expect("store")
            .load(group_id)
            .expect("group")
            .actors[0]
            .default_scope_key,
        scope_key
    );

    let invalid = raw_call(
        &home,
        "actor_update",
        json!({
            "group_id":group_id,
            "actor_id":"peer",
            "patch":{"default_scope_key":temp.path().join("other")},
            "by":"user"
        }),
    );
    assert_eq!(
        invalid.error.expect("scope error").code,
        "scope_not_attached"
    );
}

#[test]
fn attach_selects_the_attached_group() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let project = temp.path().join("project");
    std::fs::create_dir(&project).expect("project");
    let target = call(&home, "group_create", json!({"title":"target"}));
    let target_id = target.result["group"]["group_id"]
        .as_str()
        .expect("target group id");
    let previous = call(&home, "group_create", json!({"title":"previous"}));
    let previous_id = previous.result["group"]["group_id"]
        .as_str()
        .expect("previous group id");
    assert_eq!(
        cccc_core::active::get(&home)
            .expect("active group")
            .as_deref(),
        Some(previous_id)
    );

    let attached = call(
        &home,
        "attach",
        json!({"group_id":target_id,"path":project,"by":"user"}),
    );
    assert_eq!(attached.result["group_id"], target_id);
    assert_eq!(
        cccc_core::active::get(&home)
            .expect("active group")
            .as_deref(),
        Some(target_id)
    );

    let fresh_home = HomeLayout::from_path(temp.path().join("fresh-home")).expect("fresh home");
    let fresh_project = temp.path().join("fresh-project");
    std::fs::create_dir(&fresh_project).expect("fresh project");
    let created = call(
        &fresh_home,
        "attach",
        json!({"path":fresh_project,"by":"user"}),
    );
    let created_id = created.result["group_id"]
        .as_str()
        .expect("created group id");
    assert_eq!(
        cccc_core::active::get(&fresh_home)
            .expect("fresh active group")
            .as_deref(),
        Some(created_id)
    );
}

#[test]
fn scope_lifecycle_uses_canonical_receipts_state_and_events() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let scope_a = temp.path().join("scope-a");
    let scope_b = temp.path().join("scope-b");
    let unattached = temp.path().join("unattached");
    for path in [&scope_a, &scope_b, &unattached] {
        std::fs::create_dir(path).expect("scope directory");
    }
    let created = call(&home, "group_create", json!({"title":"scope lifecycle"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    let attached_a = call(
        &home,
        "attach",
        json!({"group_id":group_id,"path":scope_a,"by":"user"}),
    );
    let scope_a_key = attached_a.result["scope_key"]
        .as_str()
        .expect("scope a key");
    assert_eq!(attached_a.result["group_id"], group_id);
    assert!(attached_a.result.get("group").is_none());
    let attached_b = call(
        &home,
        "attach",
        json!({"group_id":group_id,"path":scope_b,"by":"user"}),
    );
    let scope_b_key = attached_b.result["scope_key"]
        .as_str()
        .expect("scope b key");

    call(
        &home,
        "actor_add",
        json!({
            "group_id":group_id,
            "actor_id":"scope-peer",
            "default_scope_key":scope_a_key,
            "enabled":false,
            "by":"user"
        }),
    );
    let selected = call(
        &home,
        "group_use",
        json!({"group_id":group_id,"path":scope_a,"by":"user"}),
    );
    assert_eq!(selected.result["group_id"], group_id);
    assert_eq!(selected.result["active_scope_key"], scope_a_key);
    assert_eq!(selected.result["event"]["kind"], "group.set_active_scope");
    assert_eq!(selected.result["event"]["scope_key"], scope_a_key);

    let detached = call(
        &home,
        "group_detach_scope",
        json!({"group_id":group_id,"scope_key":scope_a_key,"by":"user"}),
    );
    assert_eq!(detached.result["group_id"], group_id);
    assert_eq!(detached.result["event"]["kind"], "group.detach_scope");
    assert_eq!(detached.result["event"]["scope_key"], scope_a_key);

    let store = cccc_core::GroupStore::new(home.clone()).expect("store");
    let group = store.load(group_id).expect("group");
    assert_eq!(group.active_scope_key, scope_b_key);
    assert_eq!(group.scopes.len(), 1);
    assert_eq!(group.scopes[0].scope_key, scope_b_key);
    assert_eq!(group.actors[0].default_scope_key, scope_b_key);
    let registry = cccc_core::Registry::load(&home).expect("registry");
    assert_eq!(registry.groups[group_id].default_scope_key, scope_b_key);
    assert!(!registry.defaults.contains_key(scope_a_key));
    assert_eq!(registry.defaults[scope_b_key], group_id);

    let events = cccc_core::ledger::read_all(&store.ledger_path(group_id).expect("ledger path"))
        .expect("ledger");
    let scope_kinds = events
        .iter()
        .filter(|event| {
            matches!(
                event.kind.as_str(),
                "group.attach" | "group.set_active_scope" | "group.detach_scope"
            )
        })
        .map(|event| event.kind.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        scope_kinds,
        [
            "group.attach",
            "group.attach",
            "group.set_active_scope",
            "group.detach_scope"
        ]
    );

    let before = group;
    let invalid = raw_call(
        &home,
        "group_use",
        json!({"group_id":group_id,"path":unattached,"by":"user"}),
    );
    assert_eq!(
        invalid.error.expect("unattached scope").code,
        "scope_not_attached"
    );
    assert_eq!(store.load(group_id).expect("unchanged group"), before);
}

#[test]
fn prompt_im_space_and_voice_operations_share_rust_state() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(&home, "group_create", json!({"title":"integrations"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    call(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"peer1","runtime":"codex","by":"user"}),
    );
    let prompt = call(
        &home,
        "actor_prompt",
        json!({"group_id":group_id,"actor_id":"peer1"}),
    );
    assert!(prompt.result["prompt"].as_str().is_some_and(|text| {
        text.contains("You are peer1")
            && text.contains("CCCC Protocol:")
            && text.contains("use MCP tool `cccc_bootstrap`")
    }));

    let invalid_im = raw_call(
        &home,
        "im_set",
        json!({"group_id":group_id,"platform":"telegram"}),
    );
    assert!(!invalid_im.ok);
    call(
        &home,
        "im_set",
        json!({"group_id":group_id,"platform":"telegram","token_env":"TELEGRAM_TOKEN"}),
    );
    let start = raw_call(&home, "im_start", json!({"group_id":group_id}));
    assert!(!start.ok);
    assert_eq!(
        start.error.as_ref().map(|error| error.code.as_str()),
        Some("adapter_unavailable")
    );
    let status = call(&home, "im_status", json!({"group_id":group_id}));
    assert_eq!(status.result["configured"], true);
    assert_eq!(status.result["running"], false);
    assert_eq!(status.result["adapter_available"], false);

    call(
        &home,
        "group_space_bind",
        json!({"group_id":group_id,"provider":"notebooklm","lane":"work","remote_space_id":"notebook-1"}),
    );
    let capabilities = call(
        &home,
        "group_space_capabilities",
        json!({"group_id":group_id,"provider":"notebooklm"}),
    );
    assert_eq!(
        capabilities.result["ingest"]["resource_ingest"]["source_types"],
        json!([
            "file",
            "pasted_text",
            "web_page",
            "youtube",
            "google_docs",
            "google_slides",
            "google_spreadsheet"
        ])
    );
    assert!(
        capabilities.result["unavailable_capabilities"]
            .as_array()
            .is_some_and(|items| !items.contains(&json!("resource_ingest.file")))
    );
    let unsupported_resource = raw_call(
        &home,
        "group_space_ingest",
        json!({
            "group_id":group_id,
            "lane":"work",
            "kind":"resource_ingest",
            "payload":{"source_type":"file","file_path":"notes.md"}
        }),
    );
    assert_eq!(
        unsupported_resource
            .error
            .expect("attached-scope preflight error")
            .code,
        "scope_required"
    );
    let invalid_url = raw_call(
        &home,
        "group_space_ingest",
        json!({
            "group_id":group_id,
            "lane":"work",
            "kind":"resource_ingest",
            "payload":{"source_type":"web_page","url":"file:///tmp/secret"}
        }),
    );
    assert_eq!(
        invalid_url.error.expect("URL preflight error").code,
        "invalid_args"
    );
    let unavailable = raw_call(
        &home,
        "group_space_ingest",
        json!({"group_id":group_id,"lane":"work","payload":{}}),
    );
    assert_eq!(
        unavailable.error.expect("provider error").code,
        "space_provider_not_configured"
    );
    let local = raw_call(
        &home,
        "group_space_status",
        json!({"group_id":group_id,"provider":"local"}),
    );
    assert_eq!(
        local.error.expect("unsupported provider").code,
        "provider_unavailable"
    );
    let status = call(
        &home,
        "group_space_status",
        json!({"group_id":group_id,"provider":"notebooklm"}),
    );
    assert_eq!(
        status.result["bindings"]["work"]["remote_space_id"],
        "notebook-1"
    );

    let invalid_document = raw_call(
        &home,
        "assistant_voice_document_save",
        json!({"group_id":group_id,"document_path":"../escape.md","content":"bad"}),
    );
    assert!(!invalid_document.ok);
    let document = call(
        &home,
        "assistant_voice_document_save",
        json!({"group_id":group_id,"document_path":"voice/notes.md","content":"safe"}),
    );
    assert_eq!(document.result["document"]["storage_kind"], "rust_home");

    let profile = call(
        &home,
        "actor_profile_upsert",
        json!({"profile_id":"profile1","name":"Default","runtime":"codex"}),
    );
    assert_eq!(profile.result["profile"]["revision"], 1);
    let legacy = call(
        &home,
        "actor_profile_upsert",
        json!({"profile_id":"legacy","name":"Legacy","env":{"LEGACY_TOKEN":"secret"}}),
    );
    assert_eq!(legacy.result["profile"]["env"], json!({}));
    let legacy_keys = call(
        &home,
        "actor_profile_secret_keys",
        json!({"profile_id":"legacy"}),
    );
    assert_eq!(legacy_keys.result["keys"], json!(["LEGACY_TOKEN"]));
    let linked = call(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"linked","profile_id":"legacy","by":"user"}),
    );
    assert_eq!(linked.result["actor"]["profile_id"], "legacy");
    assert_eq!(linked.result["actor"]["profile_revision_applied"], 1);
    let linked_private = raw_call(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"linked-private","profile_id":"legacy","env_private":{"TOKEN":"denied"},"by":"user"}),
    );
    assert_eq!(
        linked_private.error.expect("linked private error").code,
        "actor_profile_linked_readonly"
    );
    let custom = call(
        &home,
        "actor_add",
        json!({"group_id":group_id,"actor_id":"custom-private","env_private":{"TOKEN":"value"},"by":"user"}),
    );
    assert_eq!(custom.result["actor"]["id"], "custom-private");
    let custom_keys = call(
        &home,
        "actor_env_private_keys",
        json!({"group_id":group_id,"actor_id":"custom-private"}),
    );
    assert_eq!(custom_keys.result["keys"], json!(["TOKEN"]));
    let conflict = raw_call(
        &home,
        "actor_profile_upsert",
        json!({"profile_id":"profile1","name":"Changed","expected_revision":0}),
    );
    assert!(!conflict.ok);
    call(
        &home,
        "actor_profile_env_private_update",
        json!({"profile_id":"profile1","set":{"API_TOKEN":"secret-value"}}),
    );
    let keys = call(
        &home,
        "actor_profile_env_private_keys",
        json!({"profile_id":"profile1"}),
    );
    assert_eq!(keys.result["keys"], json!(["API_TOKEN"]));
    assert_eq!(keys.result["masked_values"]["API_TOKEN"], "********");
    assert!(
        !serde_json::to_string(&keys.result)
            .expect("serialize keys")
            .contains("secret-value")
    );

    let denied = raw_call(
        &home,
        "group_space_provider_credential_update",
        json!({"provider":"notebooklm","by":"peer1","auth_json":"{}"}),
    );
    assert_eq!(
        denied.error.expect("permission error").code,
        "permission_denied"
    );
    let credential = call(
        &home,
        "group_space_provider_credential_update",
        json!({"provider":"notebooklm","by":"user","auth_json":"{\"cookie\":\"secret\"}"}),
    );
    assert_eq!(
        credential.result["credential"]["masked_value"],
        "{\"******\"}"
    );
    assert!(
        !serde_json::to_string(&credential.result)
            .expect("credential response")
            .contains("secret")
    );
    let health = call(
        &home,
        "group_space_provider_health_check",
        json!({"provider":"notebooklm","by":"user"}),
    );
    assert_eq!(health.result["healthy"], false);
    assert_eq!(
        health.result["error"]["code"],
        "space_provider_auth_invalid"
    );
    let auth_status = call(
        &home,
        "group_space_provider_auth",
        json!({"provider":"notebooklm","by":"user","action":"status"}),
    );
    assert_eq!(auth_status.result["credential"]["configured"], true);
    assert_eq!(auth_status.result["provider_state"]["write_ready"], false);
    let provider_state_before_candidate = auth_status.result["provider_state"].clone();
    let candidate_health = call(
        &home,
        "group_space_provider_health_check",
        json!({"provider":"notebooklm","by":"user","auth_json":"{}"}),
    );
    assert_eq!(candidate_health.result["healthy"], false);
    let auth_status_after_candidate = call(
        &home,
        "group_space_provider_auth",
        json!({"provider":"notebooklm","by":"user","action":"status"}),
    );
    assert_eq!(
        auth_status_after_candidate.result["provider_state"],
        provider_state_before_candidate
    );
    let remote_status = call(
        &home,
        "group_space_status",
        json!({"group_id":group_id,"provider":"notebooklm"}),
    );
    assert_eq!(remote_status.result["provider"]["auth_configured"], true);
    assert_eq!(remote_status.result["provider"]["write_ready"], false);
    let invalid_query_option = raw_call(
        &home,
        "group_space_query",
        json!({"group_id":group_id,"provider":"notebooklm","lane":"work","query":"x","options":{"language":"zh"}}),
    );
    assert_eq!(
        invalid_query_option.error.expect("invalid option").code,
        "invalid_args"
    );

    let memory_layout = call(
        &home,
        "memory_reme_layout_get",
        json!({"group_id":group_id}),
    );
    assert!(memory_layout.result["memory_root"].is_string());
    assert!(memory_layout.result["today_daily_file"].is_string());
    assert_eq!(memory_layout.result["backend"]["name"], "local");

    let missing_date = raw_call(
        &home,
        "memory_reme_write",
        json!({"group_id":group_id,"target":"daily","content":"entry"}),
    );
    assert!(!missing_date.ok);
    let memory_write = call(
        &home,
        "memory_reme_write",
        json!({"group_id":group_id,"target":"memory","content":"durable fact","mode":"append","idempotency_key":"memory-1"}),
    );
    assert_eq!(memory_write.result["status"], "written");
    let memory_dedup = call(
        &home,
        "memory_reme_write",
        json!({"group_id":group_id,"target":"memory","content":"changed payload","idempotency_key":"memory-1"}),
    );
    assert_eq!(memory_dedup.result["reason"], "persistence_idempotency_key");
    let memory_replace = call(
        &home,
        "memory_reme_write",
        json!({"group_id":group_id,"target":"memory","content":"replacement","mode":"replace"}),
    );
    assert_eq!(memory_replace.result["status"], "written");
    let index = call(
        &home,
        "memory_reme_index_sync",
        json!({"group_id":group_id,"mode":"scan"}),
    );
    assert!(index.result["indexed_files"].as_u64().unwrap_or(0) >= 2);
    assert!(index.result["indexed_chunks"].as_u64().unwrap_or(0) >= 1);
    assert!(
        index.result["watched_paths"]
            .as_array()
            .is_some_and(|paths| paths
                .iter()
                .all(|path| path.as_str().is_some_and(|path| path.ends_with(".md"))))
    );
}

#[test]
fn rust_group_space_sync_status_reads_python_canonical_manifests() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let project = temp.path().join("project");
    let space = project.join("space");
    std::fs::create_dir_all(&space).expect("space root");
    let created = call(&home, "group_create", json!({"title":"space sync status"}));
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");
    call(
        &home,
        "attach",
        json!({"group_id":group_id,"path":project,"by":"user"}),
    );
    call(
        &home,
        "group_space_bind",
        json!({
            "group_id":group_id,"provider":"notebooklm","lane":"work",
            "remote_space_id":"nb-work","by":"user"
        }),
    );
    std::fs::write(
        space.join(".space-sync-state.json"),
        serde_json::to_vec_pretty(&json!({
            "v":1,"group_id":group_id,"provider":"notebooklm",
            "remote_space_id":"nb-work","last_run_at":"2026-08-11T00:00:00Z",
            "state":"ok","converged":true,"unsynced_count":0,"uploaded":1
        }))
        .expect("work sync state"),
    )
    .expect("write work sync state");

    let work = call(
        &home,
        "group_space_sync",
        json!({
            "group_id":group_id,"provider":"notebooklm","lane":"work","action":"status"
        }),
    );
    assert_eq!(work.result["sync"]["available"], true);
    assert_eq!(work.result["sync"]["remote_space_id"], "nb-work");
    assert_eq!(work.result["sync"]["converged"], true);

    call(
        &home,
        "group_space_bind",
        json!({
            "group_id":group_id,"provider":"notebooklm","lane":"memory",
            "remote_space_id":"nb-memory","by":"user"
        }),
    );
    let memory_root = cccc_core::GroupStore::new(home.clone())
        .expect("store")
        .state_dir(group_id)
        .expect("state dir")
        .join("memory");
    std::fs::create_dir_all(&memory_root).expect("memory root");
    std::fs::write(
        memory_root.join("notebooklm_sync.json"),
        serde_json::to_vec_pretty(&json!({
            "v":1,"provider":"notebooklm","lane":"memory","group_id":group_id,
            "group_label":"space-sync-status","remote_space_id":"nb-memory",
            "last_scan_at":"2026-08-11T00:00:00Z","last_success_at":"2026-08-11T00:01:00Z",
            "files":{"2026-08-10":{
                "date":"2026-08-10","content_hash":"abc","entry_count":1,
                "source_ids":["src-1"],"state":"succeeded"
            }}
        }))
        .expect("memory sync state"),
    )
    .expect("write memory sync state");

    let memory = call(
        &home,
        "group_space_sync",
        json!({
            "group_id":group_id,"provider":"notebooklm","lane":"memory","action":"status"
        }),
    );
    assert_eq!(memory.result["sync"]["remote_space_id"], "nb-memory");
    assert_eq!(
        memory.result["sync"]["files"]["2026-08-10"]["state"],
        "succeeded"
    );
    assert_eq!(memory.result["summary"]["eligible_daily_files"], 1);
    assert_eq!(memory.result["summary"]["synced_daily_files"], 1);

    let overview = call(
        &home,
        "group_space_status",
        json!({"group_id":group_id,"provider":"notebooklm"}),
    );
    assert_eq!(overview.result["sync"]["remote_space_id"], "nb-work");
    assert_eq!(overview.result["sync"]["converged"], true);
    assert_eq!(overview.result["memory_sync"]["eligible_daily_files"], 1);
    assert_eq!(overview.result["memory_sync"]["synced_daily_files"], 1);
}

#[test]
fn rust_group_space_sync_is_truthfully_unavailable() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let created = call(
        &home,
        "group_create",
        json!({"title":"space sync capability"}),
    );
    let group_id = created.result["group"]["group_id"]
        .as_str()
        .expect("group id");

    let capabilities = call(
        &home,
        "group_space_capabilities",
        json!({"group_id":group_id,"provider":"notebooklm"}),
    );
    let available = capabilities.result["capabilities"]
        .as_array()
        .expect("capabilities");
    assert!(!available.iter().any(|value| value == "sync"));
    let unavailable = capabilities.result["unavailable_capabilities"]
        .as_array()
        .expect("unavailable capabilities");
    assert!(unavailable.iter().any(|value| value == "sync.work"));
    assert!(unavailable.iter().any(|value| value == "sync.memory"));

    let response = raw_call(
        &home,
        "group_space_sync",
        json!({
            "group_id":group_id,
            "provider":"notebooklm",
            "lane":"work",
            "action":"run",
            "by":"user"
        }),
    );
    assert_eq!(
        response.error.expect("unsupported sync").code,
        "capability_unavailable"
    );
}

#[test]
fn rust_daemon_provider_auth_mutations_do_not_report_synthetic_lifecycle() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");

    let status = call(
        &home,
        "group_space_provider_auth",
        json!({"provider":"notebooklm","by":"user","action":"status"}),
    );
    assert_eq!(status.result["auth"]["state"], "idle");

    for action in ["start", "cancel", "disconnect"] {
        let response = raw_call(
            &home,
            "group_space_provider_auth",
            json!({"provider":"notebooklm","by":"user","action":action}),
        );
        assert_eq!(
            response.error.expect("Web-owned auth lifecycle").code,
            "capability_unavailable",
            "action={action}"
        );
    }

    let invalid = raw_call(
        &home,
        "group_space_provider_auth",
        json!({"provider":"notebooklm","by":"user","action":"unknown"}),
    );
    assert_eq!(invalid.error.expect("invalid action").code, "invalid_args");
}

fn call(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    let response = raw_call(home, op, args);
    assert!(response.ok, "{op}: {:?}", response.error);
    response
}

fn raw_call(home: &HomeLayout, op: &str, args: Value) -> DaemonResponse {
    cccc_daemon::handle_request(
        home,
        &DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().cloned().unwrap_or_else(Map::new),
        },
    )
}
