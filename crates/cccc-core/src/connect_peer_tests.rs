use super::*;
use crate::{connect::ConnectSnapshot, membership};
use cccc_contracts::connect::{ConnectDirectory, ConnectInstance};
use serde_json::json;

pub(crate) fn fixture() -> (tempfile::TempDir, Vec<HomeLayout>, Vec<ConnectInstance>) {
    let temp = tempfile::tempdir().expect("fixture");
    let mut homes = Vec::new();
    let mut instances = Vec::new();
    for index in 0..3 {
        let home = HomeLayout::from_path(temp.path().join(format!("home-{index}"))).expect("home");
        home.initialize().expect("initialize fixture Home");
        let key = InstanceIdentity::load_or_create(&home).expect("key");
        membership::save(
            &home,
            &membership::MembershipState {
                logged_in: true,
                account_origin: Some("https://account.test".into()),
                device_id: Some(format!("device-{index}")),
                device_token: Some("fixture-only".into()),
                ..Default::default()
            },
        )
        .expect("membership");
        instances.push(ConnectInstance {
            instance_id: key.peer_id,
            device_id: format!("device-{index}"),
            public_key: key.public_key_b64,
            public_origin: Some(format!("https://device-{index}.test")),
            client_version: env!("CARGO_PKG_VERSION").into(),
            display_name: index.to_string(),
            registered_at: Utc::now().to_rfc3339(),
        });
        homes.push(home);
    }
    for (home, own) in homes.iter().zip(&instances) {
        let now = Utc::now();
        connect::save(
            home,
            &ConnectSnapshot {
                account_origin: "https://account.test".into(),
                device_id: own.device_id.clone(),
                instance_id: own.instance_id.clone(),
                directory: Some(ConnectDirectory {
                    protocol_version: 1,
                    account_id: "account".into(),
                    device_id: own.device_id.clone(),
                    issued_at: now.to_rfc3339(),
                    expires_at: (now + chrono::Duration::seconds(120)).to_rfc3339(),
                    instances: instances.clone(),
                }),
                ..Default::default()
            },
        )
        .expect("snapshot");
    }
    (temp, homes, instances)
}

fn catalog() -> ConnectPeerOperation {
    ConnectPeerOperation::Catalog {
        connection_id: None,
        source_group_id: "source-group".into(),
        target_group_id: None,
        after: None,
    }
}

#[test]
fn peer_requests_bind_account_devices_target_operation_and_response() {
    let (_temp, homes, instances) = fixture();
    let request = sign_request(&homes[0], &instances[1].instance_id, catalog()).expect("request");
    assert_eq!(
        authenticate(&homes[1], &request).expect("authenticated"),
        PeerScope::SameAccount
    );
    assert!(authenticate(&homes[2], &request).is_err());
    for field in [
        "source_device_id",
        "target_device_id",
        "account_origin",
        "account_id",
        "request_id",
    ] {
        let mut value = serde_json::to_value(&request).expect("json");
        value["proof"][field] = json!("changed");
        assert!(authenticate(&homes[1], &serde_json::from_value(value).expect("request")).is_err());
    }
    let mut modified = request.clone();
    modified.operation = ConnectPeerOperation::Catalog {
        connection_id: None,
        source_group_id: "other-source".into(),
        target_group_id: None,
        after: None,
    };
    assert!(authenticate(&homes[1], &modified).is_err());
    let response = sign_response(&homes[1], &request, json!({"groups":[]})).expect("response");
    verify_response(&homes[0], &request, &response).expect("response verified");
    assert!(verify_response(&homes[0], &modified, &response).is_err());
    let mut changed = response;
    changed.result = json!({"groups":["injected"]});
    assert!(verify_response(&homes[0], &request, &changed).is_err());
    assert!(
        serde_json::from_value::<ConnectPeerOperation>(
            json!({"op":"context_get", "group_id":"g1"})
        )
        .is_err()
    );
}

#[test]
fn old_proofs_and_results_do_not_survive_rebinding_revocation_or_expiry() {
    let (_temp, homes, instances) = fixture();
    let request = sign_request(&homes[0], &instances[1].instance_id, catalog()).expect("request");
    let response = sign_response(&homes[1], &request, json!({})).expect("response");
    let mut expired = request.clone();
    expired.proof.issued_at = (Utc::now() - chrono::Duration::seconds(120)).to_rfc3339();
    expired.proof.expires_at = (Utc::now() - chrono::Duration::seconds(60)).to_rfc3339();
    expired.proof.signature = InstanceIdentity::load(&homes[0])
        .expect("key")
        .sign(&expired.signing_material())
        .expect("signature");
    assert!(authenticate(&homes[1], &expired).is_err());
    membership::update(&homes[1], |state| {
        state.device_id = Some("replacement".into());
        Ok(())
    })
    .expect("rebind");
    assert!(authenticate(&homes[1], &request).is_err());
    assert!(sign_response(&homes[1], &request, json!({})).is_err());
    membership::update(&homes[0], |state| {
        state.disabled = true;
        Ok(())
    })
    .expect("retire");
    assert!(verify_response(&homes[0], &request, &response).is_err());
}

#[test]
fn identity_challenges_and_group_pair_scope_cannot_expand_authority() {
    let (_temp, homes, instances) = fixture();
    let nonce = uuid::Uuid::new_v4().to_string();
    let proof = identity_proof(&homes[1], &nonce).expect("proof");
    verify_identity(&instances[1], "https://device-1.test", &nonce, &proof).expect("peer");
    assert!(verify_identity(&instances[2], "https://device-2.test", &nonce, &proof).is_err());
    assert!(verify_identity(&instances[1], "https://device-1.test", "different", &proof).is_err());
    let scope = PeerScope::GroupPair {
        source_group_id: "source".into(),
        target_group_id: "target".into(),
    };
    assert!(scope.allows("source", Some("target")));
    assert!(!scope.allows("other-source", Some("target")));
    assert!(!scope.allows("source", Some("other-target")));
    assert!(!scope.allows("source", None));
    assert!(PeerScope::SameAccount.allows("", None));
    assert!(!scope.allows("", Some("target")));
}
