// Included by the crate-level integration test harness.
use cccc_core::HomeLayout;
use cccc_core::access_tokens::AccessTokenStore;

#[test]
fn creates_scoped_token_and_deletes_by_hash_id() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store =
        AccessTokenStore::new(HomeLayout::from_path(temp.path().join("rust-home")).expect("home"))
            .expect("store");
    let token = store
        .create("user-a", vec!["g1".into(), "g1".into()], false, None)
        .expect("create");
    assert_eq!(token.allowed_groups, ["g1"]);
    assert_eq!(
        store.lookup(&token.token).expect("lookup"),
        Some(token.clone())
    );
    assert_eq!(
        store.delete(&token.token_id()).expect("delete"),
        Some(token)
    );
}

#[test]
fn preserves_the_last_administrator() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store =
        AccessTokenStore::new(HomeLayout::from_path(temp.path().join("rust-home")).expect("home"))
            .expect("store");
    let admin = store
        .create("admin", Vec::new(), true, Some("acc_admin"))
        .expect("admin");
    let member = store
        .create("member", vec!["g1".into()], false, Some("acc_member"))
        .expect("member");

    let demote = store.update(&admin.token_id(), Some(vec!["g1".into()]), Some(false));
    assert!(
        demote
            .as_ref()
            .is_err_and(cccc_core::access_tokens::is_last_admin_required)
    );
    let delete = store.delete(&admin.token_id());
    assert!(
        delete
            .as_ref()
            .is_err_and(cccc_core::access_tokens::is_last_admin_required)
    );
    assert!(
        store
            .lookup(&admin.token)
            .expect("lookup admin")
            .is_some_and(|entry| entry.is_admin)
    );

    assert_eq!(
        store.delete(&member.token_id()).expect("delete member"),
        Some(member)
    );
    let delete = store.delete(&admin.token_id());
    assert!(
        delete
            .as_ref()
            .is_err_and(cccc_core::access_tokens::is_last_admin_required)
    );
    assert_eq!(store.list().expect("list"), [admin]);
}
