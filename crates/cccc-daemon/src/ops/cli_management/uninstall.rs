use super::{process::Log, usage};
use cccc_core::{HomeLayout, cli_management as management};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// 只从安装任务记录推导删除目标，不接受接口传入的路径或跟随目录链接。
fn directories(home: &HomeLayout, job: &management::Job) -> io::Result<Vec<PathBuf>> {
    let state = management::load_with_history(home)?;
    let installation = state
        .installations
        .get(&job.runtime)
        .ok_or_else(|| io::Error::other("CLI 没有受管安装，不删除外部软件"))?;
    let root = management::root(home);
    let versions = root.join("versions");
    for path in [&root, &versions] {
        match path.symlink_metadata() {
            Ok(meta) if !meta.is_dir() || meta.file_type().is_symlink() => {
                return Err(io::Error::other("受管安装根目录不是普通目录，拒绝卸载"));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    let mut directories = Vec::new();
    for installed_job in state.jobs.values().filter(|entry| {
        entry.runtime == job.runtime && entry.operation != management::Operation::Uninstall
    }) {
        management::validate_id(&installed_job.id)?;
        let path = versions.join(&installed_job.id);
        match path.symlink_metadata() {
            Ok(meta) if !meta.is_dir() || meta.file_type().is_symlink() => {
                return Err(io::Error::other("受管安装任务目录不是普通目录，拒绝卸载"));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        directories.push(path);
    }
    let owned = |path: &Path| {
        !path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
            && directories
                .iter()
                .any(|directory| path.starts_with(directory))
    };
    if !owned(&installation.executable) || installation.bin_paths.iter().any(|path| !owned(path)) {
        return Err(io::Error::other("无法从安装任务确认受管目录归属，拒绝卸载"));
    }
    // 保守拒绝被其他受管 Runtime 引用的目录。
    for (runtime, other) in &state.installations {
        if runtime != &job.runtime
            && (owned(&other.executable) || other.bin_paths.iter().any(|path| owned(path)))
        {
            return Err(io::Error::other("安装目录被其他 CLI 引用，拒绝卸载"));
        }
    }
    Ok(directories)
}

pub(super) fn run(
    home: &HomeLayout,
    job: &management::Job,
    log: &Log,
    stop: &AtomicBool,
) -> io::Result<()> {
    let _usage = usage::exclusive(home, &job.runtime)?;
    let directories = directories(home, job)?;
    log.write(
        "stage",
        "仅卸载本 CLI 的受管软件目录；保留外部安装、登录、会话、工作组和日志",
    )?;
    log.sync()?;
    for directory in directories {
        if stop.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "卸载中断，保留受管记录；请重试清理或重新安装修复",
            ));
        }
        log.write("stage", &format!("删除受管目录 {}", directory.display()))?;
        match std::fs::remove_dir_all(&directory) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    log.write(
        "stage",
        "受管软件删除完成；后续默认启动恢复原生探测，显式 Actor 命令不变",
    )?;
    log.sync()?;
    // 在释放使用锁之前提交移除选择；失败不静默回落。
    management::finish_uninstall(home, &job.id, Ok(()), chrono::Utc::now())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_contracts::{Actor, ActorRuntime, RunnerKind};
    use cccc_core::{GroupStore, Scope};
    use chrono::Utc;

    fn installed(home: &HomeLayout, id: &str) -> PathBuf {
        home.initialize().expect("installed");
        let now = Utc::now();
        management::submit(home, "codex", management::Operation::Install, id, now)
            .expect("installed");
        management::claim_next(home, now).expect("installed");
        let executable = management::root(home)
            .join("versions")
            .join(id)
            .join("bin/codex");
        cccc_core::fs::atomic_write(&executable, b"#!/bin/sh\nexec sleep 60\n").expect("installed");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
                .expect("installed");
        }
        management::finish(
            home,
            id,
            Ok(management::Installation {
                version: "1.0.0".into(),
                executable: executable.clone(),
                bin_paths: vec![executable.parent().expect("installed").to_path_buf()],
                installed_at: now.to_rfc3339(),
            }),
            now,
        )
        .expect("installed");
        executable
    }

    fn remove(home: &HomeLayout, id: &str) -> management::Job {
        management::submit(
            home,
            "codex",
            management::Operation::Uninstall,
            id,
            Utc::now(),
        )
        .expect("remove");
        let job = management::claim_next(home, Utc::now())
            .expect("remove")
            .expect("remove");
        super::super::execute_job(home, &job, &AtomicBool::new(false)).expect("remove");
        management::load(home).expect("remove").jobs[id].clone()
    }

    #[test]
    fn archived_installation_keeps_usage_protection_and_interrupted_uninstall_record() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let executable = installed(&home, "archived-install");
        let mut state = management::load(&home).expect("state");
        let original = state.jobs["archived-install"].clone();
        for index in 0..10_000 {
            let mut job = original.clone();
            job.id = format!("recent-{index}");
            job.runtime = "claude".into();
            job.created_at = "2099-01-01T00:00:00Z".into();
            state.jobs.insert(job.id.clone(), job);
        }
        cccc_core::fs::write_json(&management::root(&home).join("state.json"), &state)
            .expect("legacy state");
        let now = Utc::now();
        management::submit(
            &home,
            "codex",
            management::Operation::Uninstall,
            "interrupted",
            now,
        )
        .expect("submit");
        assert!(
            !management::load(&home)
                .expect("recent")
                .jobs
                .contains_key("archived-install")
        );
        assert_eq!(
            management::find_job(&home, "archived-install").expect("archive"),
            Some(original)
        );
        let lease = usage::acquire(
            &home,
            ActorRuntime::Cursor,
            &[executable.to_string_lossy().into_owned()],
        )
        .expect("cross-runtime archived command");
        assert!(usage::exclusive(&home, "codex").is_err());
        drop(lease);
        let job = management::claim_next(&home, now)
            .expect("claim")
            .expect("job");
        super::super::execute_job(&home, &job, &AtomicBool::new(true))
            .expect("interruption persisted");
        let state = management::load(&home).expect("state");
        assert_eq!(
            state.jobs["interrupted"].status,
            management::JobStatus::Interrupted
        );
        assert!(state.jobs["interrupted"].finished_at.is_some());
        assert_eq!(state.installations["codex"].executable, executable);
        assert!(executable.exists());
        assert_eq!(
            remove(&home, "retry-archive").status,
            management::JobStatus::Succeeded
        );
        assert!(!executable.exists());
        assert!(
            management::root(&home)
                .join("logs/interrupted.jsonl")
                .is_file()
        );
    }

    #[test]
    fn removes_all_owned_versions_preserves_external_data_and_rejects_active_use() {
        let temp = tempfile::tempdir().expect("removes all owned");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("removes all owned");
        let old = installed(&home, "old");
        let selected = installed(&home, "new");
        let external = temp.path().join("external/codex");
        let login = home.root().join("login-fixture.json");
        let session = home.root().join("groups/fixture/session.txt");
        for path in [&external, &login, &session] {
            cccc_core::fs::atomic_write(path, b"preserve").expect(
                "removes_all_owned_versions_preserves_external_data_and_rejects_active_use",
            );
        }
        let in_use = usage::acquire(&home, ActorRuntime::Codex, &[]).expect("removes all owned");
        let failure = remove(&home, "busy");
        assert_eq!(failure.status, management::JobStatus::Failed);
        assert!(failure.error.expect("removes all owned").contains("使用"));
        assert!(old.exists() && selected.exists());
        drop(in_use);
        let result = remove(&home, "retry");
        assert_eq!(result.status, management::JobStatus::Succeeded);
        assert!(!old.exists() && !selected.exists());
        assert!(
            management::load(&home)
                .expect("removes all owned")
                .installations
                .is_empty()
        );
        assert!(
            management::apply_environment(&home, "codex", &mut Default::default())
                .expect("removes all owned")
                .is_none()
        );
        for path in [&external, &login, &session] {
            assert_eq!(
                std::fs::read(path).expect(
                    "removes_all_owned_versions_preserves_external_data_and_rejects_active_use"
                ),
                b"preserve"
            );
        }
        assert!(management::root(&home).join("logs/retry.jsonl").is_file());
        assert_eq!(
            management::submit(
                &home,
                "codex",
                management::Operation::Uninstall,
                "retry",
                Utc::now()
            )
            .expect("removes all owned"),
            result
        );
    }

    #[cfg(unix)]
    #[test]
    fn real_pty_actor_blocks_removal_even_when_its_configuration_changes() {
        let temp = tempfile::tempdir().expect("real pty actor");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("real pty actor");
        let executable = installed(&home, "pty-install");
        let store = GroupStore::new(home.clone()).expect("real pty actor");
        let mut group = store.create("受控卸载测试", "").expect("real pty actor");
        group.scopes.push(Scope {
            scope_key: "test".into(),
            url: temp.path().to_string_lossy().into_owned(),
            label: "test".into(),
            git_remote: String::new(),
        });
        group.active_scope_key = "test".into();
        let mut actor = Actor::new("fixture");
        actor.runtime = ActorRuntime::Custom;
        actor.runner = RunnerKind::Pty;
        actor.command = vec![executable.to_string_lossy().into_owned()];
        group.actors.push(actor);
        crate::ops::actor_runtime::apply(&home, &group, "fixture", "actor.start")
            .expect("real pty actor");
        let pid = cccc_runtime::status(&group.group_id, "fixture")
            .expect("real pty actor")
            .pid;
        group.actors[0].command = vec!["sh".into()];
        let failed = remove(&home, "in-use");
        // 无论断言结果如何，先结束本测试拥有的子进程。
        let still_running =
            cccc_runtime::status(&group.group_id, "fixture").expect("real pty actor");
        cccc_runtime::stop(&group.group_id, "fixture").expect("real pty actor");
        assert_eq!(failed.status, management::JobStatus::Failed);
        assert!(still_running.running);
        assert_eq!(still_running.pid, pid);
        assert_eq!(
            remove(&home, "after-stop").status,
            management::JobStatus::Succeeded
        );
        assert!(!executable.exists());
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlinked_job_directories_and_allows_retry_after_missing_files() {
        let temp = tempfile::tempdir().expect("refuses symlinked job");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("refuses symlinked job");
        installed(&home, "owned");
        let path = management::root(&home).join("versions/owned");
        let moved = temp.path().join("external");
        std::fs::rename(&path, &moved).expect("refuses symlinked job");
        std::os::unix::fs::symlink(&moved, &path).expect("refuses symlinked job");
        assert_eq!(
            remove(&home, "symlink").status,
            management::JobStatus::Failed
        );
        assert!(moved.join("bin/codex").exists());
        std::fs::remove_file(&path).expect("refuses symlinked job");
        assert_eq!(
            remove(&home, "missing").status,
            management::JobStatus::Succeeded
        );
        assert!(moved.join("bin/codex").exists());
    }

    #[test]
    fn uninstall_lock_rejects_new_starts_and_releases_after_failure() {
        let temp = tempfile::tempdir().expect("uninstall lock rejects");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("uninstall lock rejects");
        installed(&home, "guarded");
        let guard = usage::exclusive(&home, "codex").expect("uninstall lock rejects");
        assert!(usage::acquire(&home, ActorRuntime::Codex, &[]).is_err());
        assert!(usage::acquire(&home, ActorRuntime::Claude, &[]).is_ok());
        drop(guard);
        assert!(usage::acquire(&home, ActorRuntime::Codex, &[]).is_ok());
    }
}
