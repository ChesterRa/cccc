//! A page that a session owner of contract 1 keeps, as the next Web reads it.
//! The owner stores the page state as given, so its shape is this Web's
//! contract with the Web that kept the page, across releases.

use super::super::{KeptSurface, system_browser};
use serde_json::Value;

const SNAPSHOT: &str =
    include_str!("../../../../cccc-daemon/src/session_owner/fixtures/contract1/snapshot.json");

#[test]
fn a_page_kept_by_a_contract_1_owner_is_read_without_loss() {
    let snapshot: Value = serde_json::from_str(SNAPSHOT).expect("contract 1 snapshot");
    let page = &snapshot["browsers"][0]["resource"]["sessions"][0];
    assert_eq!(page["key"], "web-model-login");
    let state = page["state"].clone();
    let surface: KeptSurface = serde_json::from_value(state.clone()).expect("kept surface");
    assert_eq!(serde_json::to_value(&surface).expect("encode"), state);
    // Turning the desktop view off still hides this page's VNC viewer.
    assert_eq!(
        system_browser::web_viewer(&surface.viewer, false)["vnc"]["error"],
        "disabled"
    );
    assert_eq!(
        system_browser::web_viewer(&surface.viewer, true),
        surface.viewer
    );
}
