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

#[tokio::test]
async fn closed_actor_windows_retire_with_their_generation() {
    require_chrome!();
    let (url, server) = local_page("Closed Actor lifecycle").await;
    let temp = tempfile::tempdir().expect("temp");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = GroupStore::new(home).expect("store");
    let mut group = store.create("closed owner", "").expect("group");
    let mut actor = Actor::new("a");
    actor.runtime = cccc_contracts::ActorRuntime::WebModel;
    cccc_core::actors::add(&mut group, actor).expect("actor");
    store.save(&group).expect("save");
    let manager = BrowserSurfaces::default();
    let key = format!("web-model::{}::a", group.group_id);
    for cleanup in ["generation", "actor", "prefix", "group"] {
        let generation = group.actors[0].generation.clone();
        manager
            .open_with(OpenRequest {
                key: &key,
                profile: &temp.path().join("profile"),
                url: &url,
                width: 800,
                height: 600,
                storage_state: None,
                reuse_existing: true,
                mode: BrowserMode::Headless,
                shared_browser: true,
                actor_generation: Some(&generation),
            })
            .await
            .expect("open actor");
        let page = manager
            .sessions
            .lock()
            .await
            .get(&key)
            .expect("actor session")
            .page
            .clone();
        page.close().await.expect("user closes window");
        // CDP acknowledges CloseTarget before the target necessarily disappears
        // from GetTargets. Wait for that lifecycle fact, not an arbitrary delay.
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while manager.info(&key).await["state"] != "closed" {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("closed target retired");
        manager
            .close_missing_actors(&store)
            .await
            .expect("unchanged actor");
        assert_eq!(
            manager.info(&key).await["state"],
            "closed",
            "respect manual close"
        );
        match cleanup {
            "generation" => {
                group.actors[0].generation = "replacement-generation".into();
                store.save(&group).expect("recreate same ID");
                manager
                    .close_missing_actors(&store)
                    .await
                    .expect("retire old generation");
            }
            "actor" => {
                let mut removed = group.clone();
                removed.actors.clear();
                store.save(&removed).expect("delete actor");
                manager
                    .close_missing_actors(&store)
                    .await
                    .expect("retire removed actor");
                store.save(&group).expect("restore fixture actor");
            }
            "prefix" => {
                manager
                    .close_prefixes(&[key.clone()])
                    .await
                    .expect("Web deletion cleanup");
            }
            "group" => {
                manager
                    .close_missing_groups(&HashSet::new())
                    .await
                    .expect("group cleanup");
            }
            _ => unreachable!(),
        }
        assert_eq!(
            manager.info(&key).await["state"],
            "idle",
            "{cleanup} must retire the close intent"
        );
    }
    manager.shutdown_all().await.expect("shutdown");
    server.abort();
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn legacy_actor_pairing_does_not_replace_or_reap_its_window() {
    require_chrome!();
    if !std::path::Path::new("/usr/bin/Xvfb").is_file() {
        return;
    }
    let (url, server) = local_page("<textarea id=prompt-textarea></textarea>").await;
    let temp = tempfile::tempdir().expect("temp");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("init");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("legacy actor browser", "").expect("group");
    group.running = true;
    group.state = cccc_contracts::GroupState::Active;
    for id in ["viewed", "reaped", "modern"] {
        let mut actor = Actor::new(id);
        actor.runtime = cccc_contracts::ActorRuntime::WebModel;
        actor.enabled = true;
        cccc_core::actors::add(&mut group, actor).expect("add actor");
    }
    for actor in &mut group.actors[..2] {
        actor.generation.clear();
    }
    store.save(&group).expect("legacy group");
    let connector = cccc_core::web_model_connectors::configure(&home).expect("connector");
    let manager = BrowserSurfaces::default();
    let profile = temp.path().join("shared-browser");
    let mut original_pages = Vec::new();
    for actor in &group.actors {
        let key = format!("web-model::{}::{}", group.group_id, actor.id);
        manager
            .ensure_open_shared_actor(
                &key,
                &profile,
                &url,
                (800, 600),
                &cccc_core::actors::generation_identity(actor),
            )
            .await
            .expect("open Actor window");
        let page = manager
            .sessions
            .lock()
            .await
            .get(&key)
            .expect("window")
            .page
            .target_id()
            .clone();
        original_pages.push((key, page));
    }
    for actor in &group.actors[..2] {
        let request = cccc_contracts::DaemonRequest {
            v: 1,
            op: "web_model_pairing_begin".into(),
            args: serde_json::json!({
                "by":"user", "group_id":group.group_id, "actor_id":actor.id,
                "connector_id":connector["connector"]["connector_id"], "automatic":true
            })
            .as_object()
            .expect("args")
            .clone(),
        };
        assert!(cccc_daemon::handle_request(&home, &request).ok);
    }
    let reaped = manager.close_missing_actors(&store).await.expect("cleanup");
    let loaded = store.load(&group.group_id).expect("reload group");
    manager
        .ensure_open_shared_actor(
            &original_pages[0].0,
            &profile,
            &url,
            (800, 600),
            &cccc_core::actors::generation_identity(&loaded.actors[0]),
        )
        .await
        .expect("open viewer");
    let mut retained = Vec::new();
    for (key, original) in &original_pages {
        retained.push(
            manager
                .sessions
                .lock()
                .await
                .get(key)
                .is_some_and(|session| session.page.target_id() == original),
        );
    }
    manager.shutdown_all().await.expect("stop fixture browser");
    server.abort();
    assert_eq!(reaped, 0, "pairing must not make legacy windows stale");
    assert_eq!(
        retained,
        [true, true, true],
        "viewer and other Actors keep their exact Pages"
    );
}
