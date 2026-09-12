use super::*;
use cccc_contracts::{ActorRuntime, RunnerKind};
use cccc_core::{GroupStore, cli_management as management};
use std::path::Path;
use std::time::{Duration, Instant};

fn write_cli(path: &Path, version: &str) {
    cccc_core::fs::atomic_write(
        path,
        format!(
            "@echo off\r\necho {version}>\"%CCCC_HOME%\\%CCCC_ACTOR_ID%.marker\"\r\ncmd.exe /Q\r\n"
        )
        .as_bytes(),
    )
    .expect("batch fixture");
}

fn select_version(home: &HomeLayout, version: &str, operation: management::Operation) {
    let now = chrono::Utc::now();
    let id = version.replace('.', "-");
    management::submit(home, "grok", operation, &id, now).expect("submit");
    management::claim_next(home, now).expect("claim");
    let executable = management::root(home)
        .join("versions")
        .join(&id)
        .join("bin/grok.cmd");
    write_cli(&executable, version);
    management::finish(
        home,
        &id,
        Ok(management::Installation {
            version: version.into(),
            bin_paths: vec![executable.parent().expect("bin").into()],
            executable,
            installed_at: now.to_rfc3339(),
        }),
        now,
    )
    .expect("select");
}

fn assert_actor_version(home: &HomeLayout, id: &str, version: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let marker =
            std::fs::read_to_string(home.root().join(format!("{id}.marker"))).unwrap_or_default();
        if marker.trim() == version {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "Actor {id} 未实际运行 {version}，证据：{marker:?}"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn windows_managed_actor_update_uninstall_and_external_fallback() {
    let temp = tempfile::tempdir().expect("fixture");
    let root = temp.path().join("CLI 测试 space");
    let home = HomeLayout::from_path(root.join("home")).expect("home");
    home.initialize().expect("initialize");
    let external = root.join("external/grok.cmd");
    write_cli(&external, "external");
    let external_bytes = std::fs::read(&external).expect("external");
    let sign_in = root.join("external/sign-in.fixture");
    cccc_core::fs::atomic_write(&sign_in, b"synthetic-login-and-session").expect("fixture");
    let mut group = GroupStore::new(home.clone())
        .expect("store")
        .create("Windows CLI 验证", "")
        .expect("group");
    group.scopes.push(cccc_core::Scope {
        scope_key: "fixture".into(),
        url: root.to_string_lossy().into_owned(),
        label: "fixture".into(),
        git_remote: String::new(),
    });
    group.active_scope_key = "fixture".into();
    let path = std::env::join_paths(
        std::iter::once(external.parent().expect("external").to_path_buf()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
        ),
    )
    .expect("PATH");
    let actor = |id: &str| {
        let mut actor = Actor::new(id);
        actor.runtime = ActorRuntime::Grok;
        actor.runner = RunnerKind::Pty;
        actor
            .env
            .insert("PATH".into(), path.to_string_lossy().into_owned());
        actor
    };
    // 即使后续断言失败，也回收本测试拥有的 PTY；不触碰其他组。
    struct Cleanup(String);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            for id in ["old", "new", "explicit", "fallback"] {
                let _ = cccc_runtime::stop(&self.0, id);
            }
        }
    }
    let _cleanup = Cleanup(group.group_id.clone());
    select_version(&home, "1.0.0", management::Operation::Install);
    let old = super::super::start(&home, &group, &actor("old")).expect("managed start");
    assert_actor_version(&home, "old", "1.0.0");
    let mut explicit = actor("explicit");
    explicit.command = vec![external.to_string_lossy().into_owned()];
    super::super::start(&home, &group, &explicit).expect("explicit start");
    assert_actor_version(&home, "explicit", "external");
    select_version(&home, "2.0.0", management::Operation::Update);
    super::super::start(&home, &group, &actor("new")).expect("updated start");
    assert_actor_version(&home, "new", "2.0.0");
    let still_running = cccc_runtime::status(&group.group_id, "old").expect("old status");
    assert!(still_running.running && still_running.pid == old.pid);

    let remove = |id| {
        management::submit(
            &home,
            "grok",
            management::Operation::Uninstall,
            id,
            chrono::Utc::now(),
        )
        .expect("remove");
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("runtime")
            .block_on(async {
                let worker = crate::ops::cli_management::Worker::start(home.clone());
                let deadline = Instant::now() + Duration::from_secs(10);
                let result = loop {
                    let state = management::load(&home).expect("state");
                    if !state.jobs[id].status.active() {
                        break Some(state.jobs[id].clone());
                    }
                    if Instant::now() >= deadline {
                        break None;
                    }
                    tokio::time::sleep(Duration::from_millis(25)).await;
                };
                worker.finish().await;
                result.expect("uninstall timeout")
            })
    };
    assert_eq!(remove("busy").status, management::JobStatus::Failed);
    assert!(
        cccc_runtime::status(&group.group_id, "old")
            .expect("old status")
            .running
    );
    for id in ["old", "new", "explicit"] {
        cccc_runtime::stop(&group.group_id, id).expect("stop");
    }
    assert_eq!(remove("uninstall").status, management::JobStatus::Succeeded);
    assert!(
        management::load(&home)
            .expect("state")
            .installations
            .is_empty()
    );
    super::super::start(&home, &group, &actor("fallback")).expect("fallback start");
    assert_actor_version(&home, "fallback", "external");
    super::super::start(&home, &group, &explicit).expect("external restart");
    assert_actor_version(&home, "explicit", "external");
    assert_eq!(std::fs::read(&external).expect("external"), external_bytes);
    assert_eq!(
        std::fs::read(&sign_in).expect("sign-in"),
        b"synthetic-login-and-session"
    );
    assert!(
        cccc_runtime::status(&group.group_id, "explicit")
            .expect("explicit status")
            .running
    );
}
