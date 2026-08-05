use cccc_notebooklm::Client;

#[test]
#[ignore = "requires a live CCCC_NOTEBOOKLM_AUTH_JSON credential"]
fn live_account_smoke() {
    let credential = std::env::var("CCCC_NOTEBOOKLM_AUTH_JSON")
        .expect("CCCC_NOTEBOOKLM_AUTH_JSON must contain Playwright storage-state JSON");
    let client = Client::from_storage_state(&credential).expect("authenticate");
    client.health_check().expect("list notebooks");
}
