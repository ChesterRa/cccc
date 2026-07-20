use crate::HomeLayout;
use crate::capability_legacy::{catalog, scope};
use serde_json::json;

#[test]
fn reads_legacy_skill_catalog_and_group_actor_scope() {
    let (_temp, home) = test_home();
    write_json(
        &home,
        "catalog.json",
        json!({"records":[{
            "capability_id":"skill:test:review","kind":"skill","name":"review",
            "description_short":"Review code","capsule_text":"Skill: review",
            "source_id":"test"
        }]}),
    );
    write_json(
        &home,
        "state.json",
        json!({
            "group_enabled":{"g_test":["skill:test:review"]},
            "actor_enabled":{"g_test":{"user":["skill:test:actor"]}},
            "actor_hidden":{"g_test":{"user":["skill:test:hidden"]}}
        }),
    );

    let capabilities = catalog(&home).expect("catalog");
    let actor_scope = scope(&home, "g_test", "user").expect("scope");

    assert_eq!(capabilities[0].id, "skill:test:review");
    assert_eq!(capabilities[0].description, "Review code");
    assert!(actor_scope.enabled.contains("skill:test:review"));
    assert!(actor_scope.enabled.contains("skill:test:actor"));
    assert!(actor_scope.hidden.contains("skill:test:hidden"));
}

#[test]
fn reads_object_catalog_and_invalidates_cache_after_change() {
    let (_temp, home) = test_home();
    write_json(
        &home,
        "catalog.json",
        json!({"records":{
            "skill:test:review":{
                "capability_id":"skill:test:review","kind":"skill","name":"review",
                "source_id":"github_skills_curated","source_uri":"https://example.test/review"
            }
        }}),
    );

    let capabilities = catalog(&home).expect("catalog");
    assert_eq!(capabilities.len(), 1);
    assert_eq!(capabilities[0].kind, "skill");
    assert_eq!(capabilities[0].source, "github_skills_curated");
    assert_eq!(capabilities[0].source_uri, "https://example.test/review");

    write_json(
        &home,
        "catalog.json",
        json!({"records":{
            "skill:test:updated":{
                "capability_id":"skill:test:updated","kind":"skill","name":"updated"
            }
        },"revision":"longer-to-change-the-file-fingerprint"}),
    );
    let updated = catalog(&home).expect("updated catalog");
    assert_eq!(updated.len(), 1);
    assert_eq!(updated[0].id, "skill:test:updated");
}

fn test_home() -> (tempfile::TempDir, HomeLayout) {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    std::fs::create_dir_all(home.root().join("state/capabilities")).expect("state dir");
    (temp, home)
}

fn write_json(home: &HomeLayout, name: &str, value: serde_json::Value) {
    std::fs::write(
        home.root().join("state/capabilities").join(name),
        serde_json::to_vec(&value).expect("json"),
    )
    .expect("fixture");
}
