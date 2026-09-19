use super::*;
use cccc_contracts::Actor;
use cccc_core::{GroupStore, HomeLayout};

#[tokio::test]
async fn actor_cleanup_preserves_other_surface_owners_and_disabled_actors() {
    require_chrome!();
    let (url, server) = local_page("Isolated resource cleanup").await;
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home).expect("store");
    let group = store.create("browser owner", "").expect("group");
    let mut actor = Actor::new("reader");
    actor.enabled = false;
    store
        .mutate(&group.group_id, |group| {
            group.actors.push(actor);
            Ok(())
        })
        .expect("actor");
    let manager = BrowserSurfaces::default();
    let actor_key = format!("web-model::{}::reader", group.group_id);
    let presentation_key = format!("{}::presentation", group.group_id);
    let notebook_key = "space-provider::notebooklm";
    for (index, key) in [&actor_key, &presentation_key, notebook_key]
        .iter()
        .enumerate()
    {
        manager
            .open(
                key,
                &temp.path().join(format!("profile-{index}")),
                &url,
                800,
                600,
            )
            .await
            .expect("open fixture browser");
    }

    assert_eq!(
        manager.close_missing_actors(&store).await.expect("cleanup"),
        0
    );
    assert_eq!(manager.info(&actor_key).await["active"], true);
    store
        .mutate(&group.group_id, |group| {
            group.actors.clear();
            Ok(())
        })
        .expect("remove actor");
    assert_eq!(
        manager.close_missing_actors(&store).await.expect("cleanup"),
        1
    );
    assert_eq!(manager.info(&actor_key).await["active"], false);
    assert_eq!(manager.info(&presentation_key).await["active"], true);
    assert_eq!(manager.info(notebook_key).await["active"], true);
    assert_eq!(
        manager.close_missing_actors(&store).await.expect("repeat"),
        0
    );

    assert!(store.delete(&group.group_id).expect("delete group"));
    assert_eq!(
        manager
            .close_missing_groups(&HashSet::new())
            .await
            .expect("group cleanup"),
        1
    );
    assert_eq!(manager.info(notebook_key).await["active"], true);
    manager.shutdown_all().await.expect("shutdown");
    server.abort();
}

#[tokio::test]
async fn actor_cleanup_checks_surfaces_opened_after_an_empty_pass() {
    require_chrome!();
    let (url, server) = local_page("Isolated late browser registration").await;
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home).expect("store");
    let group = store.create("late owner", "").expect("group");
    let manager = BrowserSurfaces::default();
    assert_eq!(
        manager.close_missing_actors(&store).await.expect("empty"),
        0
    );

    let key = format!("web-model::{}::removed", group.group_id);
    let profile = temp.path().join("profile");
    manager
        .open(&key, &profile, &url, 800, 600)
        .await
        .expect("open");
    assert_eq!(
        manager
            .close_missing_actors(&store)
            .await
            .expect("removed actor"),
        1
    );

    // Preserve the existing cleanup policy for unreadable owner configuration.
    store
        .mutate(&group.group_id, |group| {
            group.actors.push(Actor::new("removed"));
            Ok(())
        })
        .expect("recreate actor");
    manager
        .open(&key, &profile, &url, 800, 600)
        .await
        .expect("reopen");
    assert_eq!(
        manager
            .close_missing_actors(&store)
            .await
            .expect("live owner"),
        0
    );
    std::fs::write(
        store
            .group_dir(&group.group_id)
            .expect("group path")
            .join("group.yaml"),
        "[invalid: yaml",
    )
    .expect("damage fixture configuration");
    assert_eq!(
        manager
            .close_missing_actors(&store)
            .await
            .expect("unreadable owner"),
        1
    );
    assert_eq!(manager.info(&key).await["active"], false);
    manager.shutdown_all().await.expect("shutdown");
    server.abort();
}
