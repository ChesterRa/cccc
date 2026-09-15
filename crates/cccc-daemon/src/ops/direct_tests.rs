use super::*;
#[test]
fn direct_group_delete_and_reset_retire_grants_and_release_quota() {
    for operation in ["group_delete", "group_reset"] {
        let temp = tempfile::tempdir().expect("home");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let store = cccc_core::GroupStore::new(home.clone()).expect("store");
        let old = store.create("Old", "").expect("old group");
        let retained = store.create("Retained", "").expect("retained group");
        direct::configure(
            &home,
            Some(DirectListener {
                bind: "127.0.0.1:8847".into(),
                address: "127.0.0.1:8847".into(),
            }),
            None,
        )
        .expect("listener");
        direct::invite(&home, &retained.group_id).expect("retained invitation");
        for _ in 0..63 {
            direct::invite(&home, &old.group_id).expect("old invitation");
        }
        assert!(
            direct::invite(&home, &retained.group_id).is_err(),
            "fixture fills the quota"
        );
        let old_ids = direct::load(&home)
            .expect("store")
            .relations
            .into_iter()
            .filter(|r| r.local.group_id == old.group_id)
            .map(|r| r.id)
            .collect::<Vec<_>>();
        let cache = home
            .root()
            .join("state/connect/catalog")
            .join(format!("{}.json", old_ids[0]));
        cccc_core::fs::write_json(&cache, &json!({"groups":[]})).expect("disposable catalog");
        let result = crate::dispatch::dispatch(
            &home,
            &request(
                operation,
                json!({"group_id":old.group_id,"confirm":old.group_id,"by":"user"}),
            ),
        );
        assert!(result.ok, "Group lifecycle operation succeeds");
        let direct = direct::load(&home).expect("remaining records");
        assert_eq!(
            direct.relations.len(),
            1,
            "deleted Group must not consume the quota"
        );
        assert!(old_ids.iter().all(|id| direct.retired.contains(id)));
        assert!(!cache.exists());
        direct::invite(&home, &retained.group_id).expect("quota released");
    }
}

#[test]
fn direct_admin_can_clean_preexisting_orphans_but_cannot_create_or_approve_them() {
    let temp = tempfile::tempdir().expect("home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let store = cccc_core::GroupStore::new(home.clone()).expect("store");
    let old = store.create("Old", "").expect("old group");
    let other = store.create("Other", "").expect("other group");
    direct::configure(
        &home,
        Some(DirectListener {
            bind: "127.0.0.1:8847".into(),
            address: "127.0.0.1:8847".into(),
        }),
        None,
    )
    .expect("listener");
    direct::invite(&home, &old.group_id).expect("invite");
    let id = direct::load(&home).expect("store").relations[0].id.clone();
    // Simulate a crash after the Group disappeared but before grant retirement.
    std::fs::remove_dir_all(store.group_dir(&old.group_id).expect("dir")).expect("missing Group");
    let invoke = |op, group: &str, by| {
        crate::dispatch::dispatch(
            &home,
            &request(op, json!({"group_id":group,"id":id,"by":by})),
        )
    };
    assert!(!invoke("connect_direct_status", &old.group_id, "worker").ok);
    let status = invoke("connect_direct_status", &old.group_id, "user");
    assert!(status.ok);
    assert_eq!(status.result["relations"][0]["current"], false);
    for op in [
        "connect_direct_invite",
        "connect_direct_join",
        "connect_direct_approve",
    ] {
        assert!(!invoke(op, &old.group_id, "user").ok);
    }
    assert!(!invoke("connect_direct_revoke", &other.group_id, "user").ok);
    assert!(invoke("connect_direct_revoke", &old.group_id, "user").ok);
    assert!(invoke("connect_direct_remove", &old.group_id, "user").ok);
    let store = direct::load(&home).expect("retired store");
    assert!(store.relations.is_empty());
    assert!(store.retired.contains(&id));
}
fn request(op: &str, args: Value) -> DaemonRequest {
    DaemonRequest {
        v: 1,
        op: op.into(),
        args: args.as_object().expect("args").clone(),
    }
}
#[test]
fn direct_management_is_user_only_and_polling_does_not_initialize_runtime() {
    let temp = tempfile::tempdir().expect("home");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let group = cccc_core::GroupStore::new(home.clone())
        .expect("store")
        .create("Local", "")
        .expect("group");
    let get = request(
        "connect_direct_status",
        json!({"group_id":group.group_id,"by":"user"}),
    );
    let status = crate::dispatch::dispatch(&home, &get);
    assert!(status.ok);
    assert!(status.result["listener"].is_null());
    let addresses = status.result["addresses"]
        .as_array()
        .expect("local address suggestions");
    for address in addresses {
        let advertised: std::net::SocketAddr = address["address"]
            .as_str()
            .expect("address string")
            .parse()
            .expect("socket address");
        let bind: std::net::SocketAddr = address["bind"]
            .as_str()
            .expect("bind string")
            .parse()
            .expect("socket address");
        assert!(!advertised.ip().is_unspecified());
        assert!(!advertised.ip().is_loopback());
        assert_eq!(advertised.is_ipv4(), bind.is_ipv4());
        assert_eq!(advertised.port(), bind.port());
        assert!(address["interface"].is_string());
    }
    assert!(!home.root().join("direct_connections.json").exists());
    assert!(!home.root().join("group_bridge_identity_key.yaml").exists());
    for op in [
        "connect_direct_status",
        "connect_direct_configure",
        "connect_direct_invite",
        "connect_direct_join",
        "connect_direct_approve",
        "connect_direct_revoke",
        "connect_direct_remove",
    ] {
        let response = crate::dispatch::dispatch(
            &home,
            &request(op, json!({"group_id":group.group_id,"by":"worker"})),
        );
        assert!(!response.ok);
        assert_eq!(response.error.expect("denied").code, "permission_denied");
    }
    direct::configure(
        &home,
        Some(DirectListener {
            bind: "127.0.0.1:8847".into(),
            address: "127.0.0.1:8847".into(),
        }),
        Some("Office"),
    )
    .expect("configure");
    direct::invite(&home, &group.group_id).expect("invite");
    cccc_core::fs::write_json(
        &home.root().join("state/connect/direct_status.json"),
        &json!({
            "bind":"127.0.0.1:9911", "listener":true, "error":"Stale bind error",
            "checked_at":cccc_contracts::utc_now(),
        }),
    )
    .expect("stale runtime status");
    let status = crate::dispatch::dispatch(&home, &get);
    assert_eq!(status.result["runtime"]["listener"], false);
    assert!(status.result["runtime"]["error"].is_null());
    let json = serde_json::to_string(&status).expect("json");
    assert!(!json.contains("secret"));
    assert!(!json.contains("cccc-direct:"));
    assert_eq!(status.result["relations"][0]["online"], false);
    assert!(
        status.result["relations"][0]["expires_at"]
            .as_str()
            .is_some_and(direct::unexpired)
    );
    direct::configure(&home, None, None).expect("stop");
    assert_eq!(direct::load(&home).expect("store").display_name, "Office");
}
