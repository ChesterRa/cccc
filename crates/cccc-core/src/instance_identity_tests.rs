use super::*;

#[test]
fn reading_a_missing_identity_never_creates_one() {
    let temp = tempfile::tempdir().expect("fixture directory");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    assert_eq!(
        InstanceIdentity::load(&home)
            .expect_err("missing key")
            .kind(),
        std::io::ErrorKind::NotFound
    );
    assert!(!home.root().join("group_bridge_identity_key.yaml").exists());
    assert!(!home.root().join("group_bridge_identity_key.lock").exists());
}

#[test]
fn corrupt_identity_is_preserved_and_never_silently_replaced() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    InstanceIdentity::load_or_create(&home).expect("identity");
    let path = home.root().join("group_bridge_identity_key.yaml");
    for invalid in [
        "private_key: invalid",
        "private_key: ''",
        "[broken yaml",
        "{}",
    ] {
        std::fs::write(&path, invalid).expect("fixture operation");
        assert!(InstanceIdentity::load_or_create(&home).is_err());
        assert_eq!(
            std::fs::read_to_string(&path).expect("fixture operation"),
            invalid
        );
    }
}

#[test]
fn identity_is_stable_and_signatures_bind_the_complete_material() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let first = InstanceIdentity::load_or_create(&home).expect("identity");
    let second = InstanceIdentity::load_or_create(&home).expect("identity");
    assert_eq!(first.peer_id, second.peer_id);
    assert_eq!(first.public_key_b64, second.public_key_b64);
    let material = b"exact issuer/device/endpoint";
    let signature = first.sign(material).expect("sign");
    assert!(verify_signature(
        &second.peer_id,
        &second.public_key_b64,
        &signature,
        material
    ));
    assert!(!verify_signature(
        &second.peer_id,
        &second.public_key_b64,
        &signature,
        b"changed"
    ));
    assert!(!verify_signature(
        "other-instance",
        &second.public_key_b64,
        &signature,
        material
    ));
}
