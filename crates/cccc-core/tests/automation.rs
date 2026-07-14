use cccc_core::{GroupStore, HomeLayout, automation};
use serde_json::json;

#[test]
fn canonical_interval_rule_emits_once_per_interval() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("automation", "").expect("group");
    store
        .mutate(&group.group_id, |group| {
            group.automation = json!({
                "version":1,
                "rules":[{
                    "id":"reminder","enabled":true,"to":["@all"],
                    "trigger":{"kind":"interval","every_seconds":3600},
                    "action":{"kind":"notify","message":"check in"}
                }]
            })
            .as_object()
            .cloned()
            .expect("object");
            Ok(())
        })
        .expect("automation config");

    let first = automation::tick(&home).expect("first tick");
    assert_eq!(first.notifications.len(), 1);
    assert_eq!(first.notifications[0].data["text"], "check in");
    let second = automation::tick(&home).expect("second tick");
    assert!(second.notifications.is_empty());
}
