use super::*;
use cccc_contracts::{
    Actor,
    connect::{ConnectDirectory, ConnectInstance, ConnectPeerResponse},
};
use cccc_core::{
    connect::{self, ConnectSnapshot},
    instance_identity::InstanceIdentity,
    membership,
};
use chrono::{Duration, Utc};

#[test]
fn bounded_group_catalog_excludes_other_groups_and_private_actor_configuration() {
    let temp = tempfile::tempdir().expect("fixture");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("Shared group", "").expect("group");
    let other = store.create("Not shared", "").expect("group");
    let mut actor = Actor::new("worker");
    actor
        .env
        .insert("PRIVATE".into(), "fixture-private-setting".into());
    actor.command = vec!["fixture-private-command".into()];
    actors::add(&mut group, actor).expect("actor");
    actors::add(&mut group, Actor::new("peer")).expect("peer");
    store.save(&group).expect("save");
    let local_request = |by: &str| DaemonRequest {
        v: 1,
        op: "connect_catalog".into(),
        args: json!({"group_id":group.group_id,"by":by})
            .as_object()
            .expect("args")
            .clone(),
    };
    assert!(authorize_local_source(&home, &local_request("peer")).is_ok());
    assert!(authorize_local_source(&home, &local_request("group_bridge:external")).is_err());
    let scope = PeerScope::GroupPair {
        source_group_id: "allowed-source".into(),
        target_group_id: group.group_id.clone(),
    };
    let result =
        catalog(&home, &scope, "allowed-source", Some(&group.group_id), None).expect("catalog");
    let text = serde_json::to_string(&result).expect("json");
    assert!(!text.contains("fixture-private"));
    assert!(!text.contains(&other.group_id));
    assert_eq!(result["groups"][0]["actors"][0]["id"], "worker");
    assert!(catalog(&home, &scope, "allowed-source", None, None).is_err());
    assert!(catalog(&home, &scope, "wrong-source", Some(&group.group_id), None).is_err());
    assert!(catalog(&home, &scope, "allowed-source", Some(&other.group_id), None).is_err());
    assert!(catalog(&home, &scope, "", Some(&group.group_id), None).is_err());
    assert_eq!(
        catalog(&home, &PeerScope::SameAccount, "", None, None).expect("account catalog")["groups"]
            .as_array()
            .expect("groups")
            .len(),
        2
    );
}

#[test]
fn actor_registration_changes_on_recreate_not_edit_and_cannot_be_overridden() {
    let temp = tempfile::tempdir().expect("fixture");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let mut group = GroupStore::new(home)
        .expect("store")
        .create("Test", "")
        .expect("group");
    let old = actors::add(&mut group, Actor::new("worker")).expect("add");
    let edited = actors::update(
        &mut group,
        "worker",
        json!({"title":"Renamed", "generation":"injected"})
            .as_object()
            .expect("patch"),
    )
    .expect("edit");
    assert_eq!(old.generation, edited.generation);
    assert!(uuid::Uuid::parse_str(&old.generation).is_ok());
    actors::remove(&mut group, "worker").expect("remove");
    let recreated = actors::add(&mut group, old.clone()).expect("recreate");
    assert_ne!(old.generation, recreated.generation);
}

#[test]
fn catalog_handler_requires_current_peer_proof_and_returns_a_bound_response() {
    let temp = tempfile::tempdir().expect("fixture");
    let homes = ["a", "b"].map(|name| HomeLayout::from_path(temp.path().join(name)).expect("home"));
    for home in &homes {
        home.initialize().expect("initialize");
    }
    let identities = homes
        .each_ref()
        .map(|home| InstanceIdentity::load_or_create(home).expect("key"));
    let now = Utc::now();
    let instances = identities
        .iter()
        .enumerate()
        .map(|(index, key)| ConnectInstance {
            instance_id: key.peer_id.clone(),
            public_key: key.public_key_b64.clone(),
            device_id: format!("device-{index}"),
            client_version: env!("CARGO_PKG_VERSION").into(),
            public_origin: None,
            display_name: index.to_string(),
            registered_at: now.to_rfc3339(),
        })
        .collect::<Vec<_>>();
    for (home, own) in homes.iter().zip(&instances) {
        membership::save(
            home,
            &membership::MembershipState {
                logged_in: true,
                account_origin: Some("https://account.test".into()),
                device_id: Some(own.device_id.clone()),
                device_token: Some("fixture-only".into()),
                ..Default::default()
            },
        )
        .expect("membership");
        connect::save(
            home,
            &ConnectSnapshot {
                account_origin: "https://account.test".into(),
                device_id: own.device_id.clone(),
                instance_id: own.instance_id.clone(),
                directory: Some(ConnectDirectory {
                    protocol_version: 1,
                    account_id: "same-account".into(),
                    device_id: own.device_id.clone(),
                    issued_at: now.to_rfc3339(),
                    expires_at: (now + Duration::seconds(120)).to_rfc3339(),
                    instances: instances.clone(),
                }),
                ..Default::default()
            },
        )
        .expect("snapshot");
    }
    GroupStore::new(homes[1].clone())
        .expect("store")
        .create("Peer group", "")
        .expect("group");
    let envelope = connect_peer::sign_request(
        &homes[0],
        &instances[1].instance_id,
        ConnectPeerOperation::Catalog {
            connection_id: None,
            source_group_id: String::new(),
            target_group_id: None,
            after: None,
        },
    )
    .expect("request");
    let request = DaemonRequest {
        v: 1,
        op: "connect_peer_receive".into(),
        args: json!({"envelope":envelope})
            .as_object()
            .expect("args")
            .clone(),
    };
    let result = receive(&homes[1], &request).expect("receive");
    let response: ConnectPeerResponse =
        serde_json::from_value(result["response"].clone()).expect("response");
    connect_peer::verify_response(&homes[0], &envelope, &response).expect("verified");
    assert_eq!(
        response.result["result"]["groups"][0]["title"],
        "Peer group"
    );
    membership::update(&homes[1], |state| {
        state.disabled = true;
        Ok(())
    })
    .expect("retire");
    assert_eq!(
        receive(&homes[1], &request).expect_err("retired").code,
        "connect_peer_denied"
    );
}

#[test]
fn scoped_discovery_error_lists_only_the_calling_groups_authorized_targets() {
    use cccc_contracts::direct::DirectListener;
    use cccc_core::direct;
    let temp = tempfile::tempdir().expect("temp");
    let a = HomeLayout::from_path(temp.path().join("a")).expect("home");
    let b = HomeLayout::from_path(temp.path().join("b")).expect("home");
    let sa = GroupStore::new(a.clone()).expect("store");
    let sb = GroupStore::new(b.clone()).expect("store");
    let source = sa.create("Source", "").expect("source");
    let private = sa.create("Private", "").expect("private");
    direct::configure(
        &a,
        Some(DirectListener {
            bind: "127.0.0.1:8847".into(),
            address: "127.0.0.1:8847".into(),
        }),
        None,
    )
    .expect("listener");
    let mut targets = Vec::new();
    for name in ["Remote one", "Remote two"] {
        let target = sb.create(name, "").expect("target");
        targets.push(target.group_id.clone());
        let invitation = direct::invite(&a, &source.group_id).expect("invite");
        let id = direct::join(&b, &target.group_id, &invitation).expect("join");
        let joining = direct::load(&b)
            .expect("load")
            .relations
            .into_iter()
            .find(|r| r.id == id)
            .expect("relation");
        direct::hello(
            &a,
            &id,
            &joining.invitation.expect("invitation").secret,
            &joining.local,
            &joining.local.public_key,
        )
        .expect("hello");
        direct::set_state(&a, &source.group_id, &id, true).expect("approve");
    }
    let peer = InstanceIdentity::load(&b).expect("peer").peer_id;
    let invoke = |group: &str, args: Value| {
        let mut request = args.as_object().expect("args").clone();
        request.insert("group_id".into(), json!(group));
        request.insert("by".into(), json!("user"));
        crate::handle_request(
            &a,
            &DaemonRequest {
                v: 1,
                op: "connect_catalog".into(),
                args: request,
            },
        )
    };
    let error = invoke(&source.group_id, json!({"instance_id":peer}))
        .error
        .expect("Group required");
    assert_eq!(error.code, "connect_target_group_required");
    let listed = error.details["external_groups"]
        .as_array()
        .expect("choices");
    assert_eq!(listed.len(), 2);
    assert!(
        targets
            .iter()
            .all(|id| listed.iter().any(|g| g["group_id"] == *id))
    );
    let denied = invoke(&private.group_id, json!({"instance_id":peer}))
        .error
        .expect("no grant");
    assert_eq!(denied.code, "connect_peer_unavailable");
    assert!(!denied.details.contains_key("external_groups"));
    assert!(
        invoke(
            &source.group_id,
            json!({"instance_id":peer,"target_group_id":targets[0]})
        )
        .ok
    );
    let missing = invoke(&source.group_id, json!({"target_group_id":targets[0]}))
        .error
        .expect("instance required");
    assert_eq!(missing.code, "connect_instance_required");
}
