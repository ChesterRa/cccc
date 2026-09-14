use super::*;
use crate::connect_peer::{self, PeerScope};
use cccc_contracts::connect::ConnectPeerOperation;

fn fixture() -> (tempfile::TempDir, Vec<HomeLayout>, ConnectGroupLink) {
    let (temp, homes, instances) = connect_peer::tests::fixture();
    let mut endpoints = Vec::new();
    for (index, (home, instance)) in homes.iter().zip(&instances).enumerate() {
        let mut snapshot = connect::load(home).expect("snapshot").expect("linked");
        let directory = snapshot.directory.as_mut().expect("directory");
        directory.account_id = format!("account-{index}");
        directory.instances = vec![instance.clone()];
        connect::save(home, &snapshot).expect("snapshot");
        let group = GroupStore::new(home.clone())
            .expect("store")
            .create(&format!("Group {index}"), "")
            .expect("group");
        endpoints.push(ConnectGroupEndpoint {
            account_id: format!("account-{index}"),
            instance: instance.clone(),
            group_generation: generation(&group),
            group_id: group.group_id,
            title: group.title,
        });
    }
    let link = ConnectGroupLink {
        id: uuid::Uuid::new_v4().to_string(),
        source: endpoints[0].clone(),
        target: endpoints[1].clone(),
    };
    for index in 0..2 {
        save(
            &homes[index],
            &ConnectGroupLinks {
                protocol_version: 1,
                account_origin: "https://account.test".into(),
                account_id: format!("account-{index}"),
                device_id: instances[index].device_id.clone(),
                issued_at: timestamp(Utc::now()),
                expires_at: timestamp(Utc::now() + chrono::Duration::seconds(119)),
                links: vec![link.clone()],
            },
        )
        .expect("grant");
    }
    (temp, homes, link)
}

fn catalog(link: &ConnectGroupLink) -> ConnectPeerOperation {
    ConnectPeerOperation::Catalog {
        connection_id: Some(link.id.clone()),
        source_group_id: link.source.group_id.clone(),
        target_group_id: Some(link.target.group_id.clone()),
        after: None,
    }
}

#[test]
fn group_tickets_bind_live_resource_generation_and_do_not_grant_peer_access() {
    let (_temp, homes, link) = fixture();
    let selected = ticket(&homes[0], &link.source.group_id).expect("select");
    let request = ConnectGroupCheck {
        ticket: selected.clone(),
        nonce: uuid::Uuid::new_v4().to_string(),
    };
    let result = check(&homes[0], &request).expect("live Group");
    assert!(verify_signature(
        &link.source.instance.instance_id,
        &link.source.instance.public_key,
        &result.signature,
        &result.signing_material()
    ));
    assert!(check(&homes[1], &request).is_err());
    let mut forged = request.clone();
    forged.ticket.group_id = link.target.group_id.clone();
    let unknown = check(&homes[0], &forged).expect_err("forged nonexistent Group");
    forged.ticket.group_id = link.source.group_id.clone();
    forged.ticket.title = "forged".into();
    assert_eq!(
        check(&homes[0], &forged).expect_err("forged existing Group"),
        unknown
    );
    let store = GroupStore::new(homes[0].clone()).expect("store");
    let mut group = store.load(&link.source.group_id).expect("Group");
    group.title = "Renamed".into();
    store.save(&group).expect("rename");
    assert_eq!(
        check(&homes[0], &request)
            .expect("rename preserves selection")
            .title,
        "Renamed"
    );
    store.delete(&group.group_id).expect("delete fixture Group");
    let imported = store
        .import(group.clone())
        .expect("import the same ID and history metadata");
    assert_eq!(imported.group_id, group.group_id);
    assert_eq!(imported.created_at, group.created_at);
    assert_ne!(generation(&imported), generation(&group));
    assert!(check(&homes[0], &request).is_err());
    assert!(retired(&homes[0], &link.id).expect("retired"));
}

#[test]
fn external_peers_have_only_the_exact_bidirectional_group_scope() {
    let (_temp, homes, link) = fixture();
    assert!(connect_peer::binding(&homes[0], &link.target.instance.instance_id).is_err());
    let request =
        connect_peer::sign_request(&homes[0], &link.target.instance.instance_id, catalog(&link))
            .expect("request");
    assert_eq!(
        connect_peer::authenticate(&homes[1], &request).expect("peer"),
        PeerScope::GroupPair {
            source_group_id: link.source.group_id.clone(),
            target_group_id: link.target.group_id.clone()
        }
    );
    let result = connect_peer::sign_response(&homes[1], &request, serde_json::json!({"ok":true}))
        .expect("response");
    connect_peer::verify_response(&homes[0], &request, &result).expect("verified");
    assert!(connect_peer::authenticate(&homes[2], &request).is_err());
    assert!(
        connect_peer::group_binding(
            &homes[0],
            &link.target.instance.instance_id,
            "unrelated-local-group",
            &link.target.group_id
        )
        .is_err()
    );
    let reverse = ConnectPeerOperation::Catalog {
        connection_id: Some(link.id.clone()),
        source_group_id: link.target.group_id.clone(),
        target_group_id: Some(link.source.group_id.clone()),
        after: None,
    };
    let reverse = connect_peer::sign_request(&homes[1], &link.source.instance.instance_id, reverse)
        .expect("reverse");
    connect_peer::authenticate(&homes[0], &reverse).expect("bidirectional");
    let mut malicious = request.clone();
    malicious.operation = ConnectPeerOperation::Catalog {
        connection_id: Some(link.id.clone()),
        source_group_id: link.source.group_id.clone(),
        target_group_id: None,
        after: None,
    };
    malicious.proof.operation_sha256 = connect_peer::operation_digest(&malicious.operation);
    malicious.proof.signature = InstanceIdentity::load(&homes[0])
        .expect("key")
        .sign(&malicious.proof.signing_material())
        .expect("signature");
    assert!(connect_peer::authenticate(&homes[1], &malicious).is_err());
}

#[test]
fn revoke_expiry_and_reconnect_never_reauthorize_old_requests_or_catalogues() {
    let (_temp, homes, mut link) = fixture();
    let request =
        connect_peer::sign_request(&homes[0], &link.target.instance.instance_id, catalog(&link))
            .expect("request");
    let response =
        connect_peer::sign_response(&homes[1], &request, serde_json::json!({})).expect("response");
    let old = link.id.clone();
    let mut grant = load(&homes[0]).expect("read").expect("grant");
    grant.links.clear();
    save(&homes[0], &grant).expect("revoke");
    assert!(retired(&homes[0], &old).expect("revoked"));
    assert!(connect_peer::verify_response(&homes[0], &request, &response).is_err());
    link.id = uuid::Uuid::new_v4().to_string();
    grant.links = vec![link.clone()];
    save(&homes[0], &grant).expect("new connection");
    assert!(
        connect_peer::scoped_binding(&homes[0], &link.target.instance.instance_id, Some(&old))
            .is_err()
    );
    connect_peer::group_binding(
        &homes[0],
        &link.target.instance.instance_id,
        &link.source.group_id,
        &link.target.group_id,
    )
    .expect("new connection is usable");
    grant.expires_at = timestamp(Utc::now() - chrono::Duration::seconds(1));
    assert!(save(&homes[0], &grant).is_err());
    fs::write_secret_json(&homes[0].root().join("secrets/connect_groups.json"), &grant)
        .expect("expired fixture");
    assert!(load(&homes[0]).expect("read").is_none());
    assert!(!retired(&homes[0], &link.id).expect("unavailable is not proof of revocation"));
    assert!(
        connect_peer::scoped_binding(&homes[0], &link.target.instance.instance_id, Some(&link.id))
            .is_err()
    );
}

#[test]
fn unreadable_resources_deny_access_without_proving_retirement_and_recover_in_place() {
    let (_temp, homes, link) = fixture();
    let selected = ticket(&homes[0], &link.source.group_id).expect("select");
    let request = ConnectGroupCheck {
        ticket: selected,
        nonce: uuid::Uuid::new_v4().to_string(),
    };
    let path = homes[0]
        .root()
        .join("groups")
        .join(&link.source.group_id)
        .join("group.yaml");
    let original = std::fs::read(&path).expect("fixture YAML");
    std::fs::write(&path, "v: [invalid YAML").expect("temporary parse error");
    assert!(resource_current(&homes[0], &link.source).is_err());
    assert!(
        retired(&homes[0], &link.id).is_err(),
        "unreadable is not a retirement decision"
    );
    assert!(
        connect_peer::scoped_binding(&homes[0], &link.target.instance.instance_id, Some(&link.id))
            .is_err()
    );
    assert!(matches!(
        check(&homes[0], &request),
        Err(SelectionError::Unavailable(_))
    ));
    let mut forged = request.clone();
    forged.ticket.title = "forged".into();
    assert_eq!(
        check(&homes[0], &forged).expect_err("authenticate before reading"),
        SelectionError::Invalid
    );
    std::fs::write(&path, original).expect("restore fixture YAML");
    assert!(resource_current(&homes[0], &link.source).expect("resource restored"));
    assert!(!retired(&homes[0], &link.id).expect("same resource"));
    check(&homes[0], &request).expect("same selection recovers");
    connect_peer::scoped_binding(&homes[0], &link.target.instance.instance_id, Some(&link.id))
        .expect("same grant recovers");
    let mut snapshot = connect::load(&homes[0]).expect("snapshot").expect("linked");
    let current = snapshot.clone();
    snapshot.directory.as_mut().expect("directory").expires_at = "2000-01-01T00:00:00Z".into();
    connect::save(&homes[0], &snapshot).expect("expired confirmation");
    assert!(matches!(
        check(&homes[0], &request),
        Err(SelectionError::Unavailable(_))
    ));
    connect::save(&homes[0], &current).expect("fresh confirmation");
    GroupStore::new(homes[0].clone())
        .expect("store")
        .delete(&link.source.group_id)
        .expect("delete");
    assert!(!resource_current(&homes[0], &link.source).expect("confirmed deletion"));
    assert!(retired(&homes[0], &link.id).expect("deleted"));
    assert_eq!(
        check(&homes[0], &request).expect_err("new selection needed"),
        SelectionError::Invalid
    );
}
