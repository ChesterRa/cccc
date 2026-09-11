use super::*;
use cccc_core::{GroupStore, cli_management as management};
use std::os::unix::fs::PermissionsExt;

#[test]
fn managed_cli_scheduled_update_preserves_live_actor_and_survives_worker_restart() {
    use std::{
        path::Path,
        process::Command,
        time::{Duration, Instant},
    };
    const CHILD_ROOT: &str = "CCCC_CLI_WORKER_FIXTURE";
    if let Ok(root) = std::env::var(CHILD_ROOT) {
        let root = Path::new(&root);
        assert_eq!(
            std::fs::read_to_string(root.join("fixture-marker")).expect(
                "managed_cli_scheduled_update_preserves_live_actor_and_survives_worker_restart"
            ),
            "controlled-cli-only"
        );
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("managed cli scheduled")
            .block_on(scheduled_fixture(root));
        return;
    }
    // 子测试进程拥有独立 PATH；不修改并行测试或真实 CCCC 的进程环境。
    let temp = tempfile::tempdir().expect("managed cli scheduled");
    let root = temp.path();
    cccc_core::fs::atomic_write(&root.join("fixture-marker"), b"controlled-cli-only")
        .expect("managed cli scheduled");
    cccc_core::fs::atomic_write(&root.join("target-version"), b"1.0.0")
        .expect("managed cli scheduled");
    for version in ["0.9.0", "1.0.0", "2.0.0", "3.0.0"] {
        let code = format!(
            r#"#!/bin/sh
if [ "${{1:-}}" = --version ]; then
  printf 'cursor-agent {version}\n'
  [ '{version}' != '3.0.0' ]
  exit $?
fi
printf '{version}\n' >> "$CCCC_HOME/$CCCC_ACTOR_ID.marker"
while IFS= read -r fixture_line; do
  printf '{version}\n' >> "$CCCC_HOME/$CCCC_ACTOR_ID.marker"
done
"#
        );
        let path = root.join(if version == "0.9.0" {
            "cursor-agent"
        } else {
            version
        });
        cccc_core::fs::atomic_write(&path, code.as_bytes()).expect(
            "managed_cli_scheduled_update_preserves_live_actor_and_survives_worker_restart",
        );
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).expect(
            "managed_cli_scheduled_update_preserves_live_actor_and_survives_worker_restart",
        );
    }
    cccc_core::fs::atomic_write(
        &root.join("mise"),
        br#"#!/bin/sh
set -eu
fixture_dir=$(dirname "$0")
case "$1" in
  latest) cat "$fixture_dir/target-version" ;;
  install)
fixture_version=$(cat "$fixture_dir/target-version")
mkdir -p "$MISE_DATA_DIR/bin"
cp "$fixture_dir/$fixture_version" "$MISE_DATA_DIR/bin/cursor-agent"
;;
  env) printf '{"PATH":"%s/bin"}\n' "$MISE_DATA_DIR" ;;
  *) exit 9 ;;
esac
"#,
    )
    .expect("managed cli scheduled");
    std::fs::set_permissions(root.join("mise"), std::fs::Permissions::from_mode(0o700))
        .expect("managed cli scheduled");
    let path = std::env::join_paths([root, Path::new("/usr/bin"), Path::new("/bin")])
        .expect("managed cli scheduled");
    let mut command =
        Command::new(std::env::current_exe().expect(
            "managed_cli_scheduled_update_preserves_live_actor_and_survives_worker_restart",
        ));
    let test_name = format!(
        "{}::managed_cli_scheduled_update_preserves_live_actor_and_survives_worker_restart",
        module_path!()
            .split_once("::")
            .expect("managed cli scheduled")
            .1
    );
    command
        .args(["--exact", &test_name, "--nocapture"])
        .env_clear()
        .env("PATH", path)
        .env(CHILD_ROOT, root);
    let (mut child, owner) =
        cccc_runtime::OwnedProcessTree::spawn(&mut command).expect("managed cli scheduled");
    let deadline = Instant::now() + Duration::from_secs(40);
    let result = loop {
        if let Some(status) = child.try_wait().expect("managed cli scheduled") {
            break Some(status);
        }
        if Instant::now() >= deadline {
            break None;
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    owner.terminate().expect("managed cli scheduled");
    child.wait().expect("managed cli scheduled");
    assert!(
        result.is_some_and(|status| status.success()),
        "独立后台/Actor 验证失败或超时"
    );
    let home = HomeLayout::from_path(root.join("home")).expect("managed cli scheduled");
    assert_eq!(
        management::load(&home)
            .expect("managed cli scheduled")
            .jobs
            .len(),
        4,
        "必须实际执行子测试，不能接受零测试通过"
    );
}

async fn scheduled_fixture(root: &std::path::Path) {
    use crate::ops::cli_management::Worker;
    use cccc_contracts::{ActorRuntime, RunnerKind};
    use std::time::{Duration, Instant};
    let home = HomeLayout::from_path(root.join("home")).expect("scheduled fixture");
    home.initialize().expect("scheduled fixture");
    let external_path = root.join("cursor-agent");
    let external_bytes = std::fs::read(&external_path).expect("scheduled fixture");
    let external_mode = std::fs::metadata(&external_path)
        .expect("scheduled fixture")
        .permissions()
        .mode();
    let store = GroupStore::new(home.clone()).expect("scheduled fixture");
    let mut group = store
        .create("CLI 生命周期验证", "")
        .expect("scheduled fixture");
    group.scopes.push(cccc_core::Scope {
        scope_key: "fixture".into(),
        url: root.to_string_lossy().into_owned(),
        label: "fixture".into(),
        git_remote: String::new(),
    });
    group.active_scope_key = "fixture".into();
    let worker = Worker::start(home.clone());
    management::submit(
        &home,
        "cursor",
        management::Operation::Install,
        "initial",
        chrono::Utc::now(),
    )
    .expect("scheduled fixture");
    let job_home = &home;
    let wait_jobs = |count| async move {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let state = management::load(job_home).expect("scheduled fixture");
            if state.jobs.len() == count && state.jobs.values().all(|job| !job.status.active()) {
                return state;
            }
            assert!(Instant::now() < deadline, "后台任务未按期完成");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    let initial = wait_jobs(1).await;
    assert_eq!(initial.installations["cursor"].version, "1.0.0");
    let actor = |id| {
        let mut actor = Actor::new(id);
        actor.runtime = ActorRuntime::Cursor;
        actor.runner = RunnerKind::Pty;
        actor
    };
    let old = super::super::start(&home, &group, &actor("old")).expect("scheduled fixture");
    assert!(old.running);
    let schedule = |id: &str| {
        let at = chrono::Utc::now() + chrono::TimeDelta::seconds(1);
        management::Rule {
            id: id.into(),
            enabled: true,
            trigger: serde_json::json!({"kind":"at","at":at.to_rfc3339()})
                .as_object()
                .expect("scheduled fixture")
                .clone(),
        }
    };
    cccc_core::fs::atomic_write(&root.join("target-version"), b"2.0.0").expect("scheduled fixture");
    management::save_rules(&home, 0, vec![schedule("update")], chrono::Utc::now())
        .expect("scheduled fixture");
    let updated = wait_jobs(2).await;
    assert_eq!(updated.installations["cursor"].version, "2.0.0");
    assert!(
        updated
            .jobs
            .values()
            .any(|job| job.source_rule.as_deref() == Some("update")
                && job.status == management::JobStatus::Succeeded)
    );
    let current = cccc_runtime::status(&group.group_id, "old").expect("scheduled fixture");
    assert!(current.running && current.pid == old.pid);
    super::super::start(&home, &group, &actor("new")).expect("scheduled fixture");
    cccc_runtime::write(&group.group_id, "old", b"still working\r").expect("scheduled fixture");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let old_text = std::fs::read_to_string(home.root().join("old.marker")).unwrap_or_default();
        let new_text = std::fs::read_to_string(home.root().join("new.marker")).unwrap_or_default();
        if old_text.lines().count() >= 2 && !new_text.is_empty() {
            assert!(old_text.lines().all(|line| line == "1.0.0"));
            assert!(new_text.lines().all(|line| line == "2.0.0"));
            break;
        }
        assert!(Instant::now() < deadline, "Actor 未输出版本证据");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    cccc_core::fs::atomic_write(&root.join("target-version"), b"3.0.0").expect("scheduled fixture");
    management::save_rules(
        &home,
        1,
        vec![schedule("failed-update")],
        chrono::Utc::now(),
    )
    .expect("scheduled fixture");
    let failed = wait_jobs(3).await;
    assert_eq!(failed.installations, updated.installations);
    assert!(
        failed
            .jobs
            .values()
            .any(|job| job.source_rule.as_deref() == Some("failed-update")
                && job.status == management::JobStatus::Failed)
    );
    worker.finish().await;
    let restarted = Worker::start(home.clone());
    tokio::time::sleep(Duration::from_millis(1200)).await;
    restarted.finish().await;
    assert_eq!(
        management::load(&home).expect("scheduled fixture").jobs,
        failed.jobs
    );
    assert_eq!(
        cccc_runtime::status(&group.group_id, "old")
            .expect("scheduled fixture")
            .pid,
        old.pid
    );
    for id in ["old", "new"] {
        cccc_runtime::stop(&group.group_id, id).expect("scheduled fixture");
    }
    management::submit(
        &home,
        "cursor",
        management::Operation::Uninstall,
        "remove",
        chrono::Utc::now(),
    )
    .expect("scheduled fixture");
    let worker = Worker::start(home.clone());
    let removed = wait_jobs(4).await;
    assert_eq!(
        removed.jobs["remove"].status,
        management::JobStatus::Succeeded
    );
    assert!(removed.installations.is_empty());
    assert_eq!(
        std::fs::read(&external_path).expect("scheduled fixture"),
        external_bytes
    );
    assert_eq!(
        std::fs::metadata(&external_path)
            .expect("scheduled fixture")
            .permissions()
            .mode(),
        external_mode
    );
    super::super::start(&home, &group, &actor("fallback")).expect("scheduled fixture");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if std::fs::read_to_string(home.root().join("fallback.marker"))
            .unwrap_or_default()
            .contains("0.9.0")
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "卸载后 Actor 没有实际执行外部版本"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    cccc_runtime::stop(&group.group_id, "fallback").expect("scheduled fixture");
    worker.finish().await;
    println!(
        "后台计划 → 更新 → 新 Actor 使用 2.0.0；旧 Actor 保持 1.0.0；失败保留版本；Worker 重启不重放，验证通过"
    );
}

#[test]
fn managed_cli_is_used_for_default_launch_but_explicit_program_is_preserved() {
    let temp = tempfile::tempdir().expect("managed cli is");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("managed cli is");
    home.initialize().expect("managed cli is");
    let group = GroupStore::new(home.clone())
        .expect("managed cli is")
        .create("test", "")
        .expect("managed cli is");
    let now = chrono::Utc::now();
    management::submit(
        &home,
        "codex",
        management::Operation::Install,
        "launch-test",
        now,
    )
    .expect("managed cli is");
    management::claim_next(&home, now).expect("managed cli is");
    let executable = management::root(&home).join("versions/launch-test/bin/codex");
    cccc_core::fs::atomic_write(&executable, b"#!/bin/sh\nexit 0\n").expect("managed cli is");
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
        .expect("managed cli is");
    management::finish(
        &home,
        "launch-test",
        Ok(management::Installation {
            version: "test".into(),
            executable: executable.clone(),
            bin_paths: vec![
                executable
                    .parent()
                    .expect(
                        "managed_cli_is_used_for_default_launch_but_explicit_program_is_preserved",
                    )
                    .to_path_buf(),
            ],
            installed_at: now.to_rfc3339(),
        }),
        now,
    )
    .expect("managed cli is");
    let mut actor = Actor::new("codex");
    let resolved = resolve_launch_actor(&home, &group, &actor).expect("managed cli is");
    assert_eq!(resolved.command[0], executable.to_string_lossy());
    assert!(resolved.command.iter().any(|arg| arg == "--search"));
    assert!(actor.command.is_empty(), "启动解析不能改变保存的 Actor");
    actor.command = vec!["codex".into(), "--custom-flag".into()];
    let resolved = resolve_launch_actor(&home, &group, &actor).expect("managed cli is");
    assert_eq!(
        resolved.command,
        [
            executable.to_string_lossy().into_owned(),
            "--custom-flag".into()
        ]
    );
    actor.command[0] = "/operator/custom/codex".into();
    let resolved = resolve_launch_actor(&home, &group, &actor).expect("managed cli is");
    assert_eq!(resolved.command, actor.command);
    assert_eq!(resolved.env, actor.env);
}

#[test]
fn managed_cli_launch_keeps_resolved_profile_snapshot() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path()).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("store")
        .create("snapshot", "")
        .expect("group");
    let profiles = cccc_core::profiles::ProfileStore::new(home.clone()).expect("profiles");
    let profile = |runtime, command| {
        serde_json::json!({"id":"ap_snapshot", "name":"snapshot", "runtime":runtime, "command":[command]}).as_object().expect("object").clone()
    };
    profiles
        .upsert(profile("cursor", "/old/cursor-agent"), None)
        .expect("original");
    let actor =
        crate::ops::actor_profile_runtime::link(&home, &Actor::new("snapshot"), "ap_snapshot")
            .expect("link");
    let snapshot = crate::ops::actor_profile_runtime::resolve(&home, &actor).expect("resolve once");
    profiles
        .upsert(profile("codex", "/new/codex"), None)
        .expect("changed during launch");
    let launch = resolve_launch_actor(&home, &group, &snapshot).expect("prepare resolved launch");
    assert_eq!(launch.runtime, snapshot.runtime);
    assert_eq!(launch.command, snapshot.command);
    assert_ne!(
        launch.command,
        crate::ops::actor_profile_runtime::resolve(&home, &actor)
            .expect("new profile")
            .command
    );
}
