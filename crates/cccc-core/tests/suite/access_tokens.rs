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
