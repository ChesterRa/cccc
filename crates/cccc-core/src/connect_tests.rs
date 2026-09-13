use super::*;
use crate::instance_identity::InstanceIdentity;
use cccc_contracts::connect::{ConnectDirectory, ConnectInstance};

fn fixture() -> (tempfile::TempDir, HomeLayout, ConnectSnapshot) {
    let temp = tempfile::tempdir().expect("fixture operation");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("fixture operation");
    let identity = InstanceIdentity::load_or_create(&home).expect("fixture operation");
    membership::save(
        &home,
        &membership::MembershipState {
            logged_in: true,
            account_origin: Some("https://account.test".into()),
            device_id: Some("device-a".into()),
            device_token: Some("fixture-only-token".into()),
            ..Default::default()
        },
    )
    .expect("fixture operation");
    let now = Utc::now();
    let snapshot = ConnectSnapshot {
        account_origin: "https://account.test".into(),
        device_id: "device-a".into(),
        instance_id: identity.peer_id.clone(),
        checked_at: now.to_rfc3339(),
        directory: Some(ConnectDirectory {
            protocol_version: 1,
            account_id: "account-a".into(),
            device_id: "device-a".into(),
            issued_at: now.to_rfc3339(),
            expires_at: (now + chrono::Duration::seconds(120)).to_rfc3339(),
            instances: vec![ConnectInstance {
                instance_id: identity.peer_id,
                public_key: identity.public_key_b64,
                device_id: "device-a".into(),
                public_origin: Some("https://a.test".into()),
                client_version: "0.4.39".into(),
                display_name: "A".into(),
                registered_at: now.to_rfc3339(),
            }],
        }),
        ..Default::default()
    };
    (temp, home, snapshot)
}

#[test]
fn saved_directory_survives_restart_but_not_expiry_or_local_binding_changes() {
    let (_temp, home, mut snapshot) = fixture();
    save(&home, &snapshot).expect("fixture operation");
    assert!(
        load(&home)
            .expect("fixture operation")
            .expect("fixture operation")
            .directory
            .is_some()
    );
    snapshot
        .directory
        .as_mut()
        .expect("fixture operation")
        .expires_at = (Utc::now() - chrono::Duration::seconds(1)).to_rfc3339();
    save(&home, &snapshot).expect("fixture operation");
    assert!(
        load(&home)
            .expect("fixture operation")
            .expect("fixture operation")
            .directory
            .is_none()
    );
    membership::update(&home, |state| {
        state.device_id = Some("rebound".into());
        Ok(())
    })
    .expect("fixture operation");
    assert!(load(&home).expect("fixture operation").is_none());
    membership::update(&home, |state| {
        state.device_id = Some("device-a".into());
        state.account_origin = Some("https://different.test".into());
        Ok(())
    })
    .expect("fixture operation");
    assert!(load(&home).expect("fixture operation").is_none());
    membership::update(&home, |state| {
        state.account_origin = Some("https://account.test".into());
        state.disabled = true;
        Ok(())
    })
    .expect("fixture operation");
    assert!(load(&home).expect("fixture operation").is_none());
}

#[test]
fn directory_rejects_wrong_identity_duplicate_bindings_and_unbounded_lifetimes() {
    let (_temp, _home, snapshot) = fixture();
    let directory = snapshot.directory.expect("fixture operation");
    let validate = |value: &ConnectDirectory| {
        validate_directory(value, "device-a", &snapshot.instance_id, Utc::now())
    };
    assert!(validate(&directory).is_ok());
    let mut changed = directory.clone();
    changed.instances.push(changed.instances[0].clone());
    assert!(validate(&changed).is_err());
    let mut changed = directory.clone();
    changed.instances[0].public_key = "invalid".into();
    assert!(validate(&changed).is_err());
    let mut changed = directory.clone();
    changed.expires_at = (Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
    assert!(validate(&changed).is_err());
    let mut changed = directory.clone();
    changed.instances.clear();
    assert!(validate(&changed).is_err());
    let mut changed = directory;
    changed.instances[0].public_origin = Some("https://secret@other.test/path".into());
    assert!(validate(&changed).is_err());
}

#[test]
fn reading_status_does_not_create_an_identity_or_cached_state() {
    let temp = tempfile::tempdir().expect("fixture operation");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("fixture operation");
    assert!(load(&home).expect("fixture operation").is_none());
    assert!(!home.root().join("group_bridge_identity_key.yaml").exists());
    assert!(!home.root().join("secrets/connect.json").exists());
}
