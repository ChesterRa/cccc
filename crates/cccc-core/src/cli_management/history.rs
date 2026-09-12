//! 沿用原生文件锁和已提交 JSON 写入；归档不删除历史或日志。
use super::{Job, State, invalid, load, load_unlocked, root, validate_id};
use crate::{HomeLayout, fs};
use std::io;
use std::path::PathBuf;

const RECENT_COMPLETED: usize = 10_000;

fn archive_path(home: &HomeLayout, id: &str) -> io::Result<PathBuf> {
    validate_id(id)?;
    // 编码避免 Windows 上大小写不同的请求编号映射到同一文件。
    let name: String = id.bytes().map(|byte| format!("{byte:02x}")).collect();
    Ok(root(home).join("history").join(format!("{name}.json")))
}

pub(super) fn archived_job(home: &HomeLayout, id: &str) -> io::Result<Option<Job>> {
    let path = archive_path(home, id)?;
    match fs::read_json::<Job>(&path) {
        Ok(job) if job.id == id && !job.status.active() => Ok(Some(job)),
        Ok(_) => Err(invalid("cli_job_archive_invalid")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub fn find_job(home: &HomeLayout, id: &str) -> io::Result<Option<Job>> {
    validate_id(id)?;
    match load(home)?.jobs.remove(id) {
        Some(job) => Ok(Some(job)),
        None => archived_job(home, id),
    }
}

/// 显式查看全部或查询安装归属时读取；日常启动、排程和轮询不展开归档。
pub fn load_with_history(home: &HomeLayout) -> io::Result<State> {
    fs::with_exclusive_lock(&root(home).join("state.lock"), || {
        let mut state = load_unlocked(home)?;
        let entries = match std::fs::read_dir(root(home).join("history")) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(state),
            Err(error) => return Err(error),
        };
        for entry in entries {
            let entry = entry?;
            if entry.path().extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let job: Job = fs::read_json(&entry.path())?;
            if job.status.active() || archive_path(home, &job.id)? != entry.path() {
                return Err(invalid("cli_job_archive_invalid"));
            }
            if let Some(current) = state.jobs.get(&job.id) {
                if current != &job {
                    return Err(invalid("cli_job_archive_conflict"));
                }
            } else {
                state.jobs.insert(job.id.clone(), job);
            }
        }
        Ok(state)
    })
}

pub(super) fn compact(home: &HomeLayout, previous: &State, state: &mut State) -> io::Result<()> {
    let mut completed = state
        .jobs
        .values()
        .filter(|job| !job.status.active())
        .collect::<Vec<_>>();
    completed.sort_by(|a, b| (&b.created_at, &b.id).cmp(&(&a.created_at, &a.id)));
    let archive = completed
        .into_iter()
        .skip(RECENT_COMPLETED)
        // 只归档此前已提交的终态，不能提前提交当前操作的结果。
        .filter(|job| previous.jobs.get(&job.id) == Some(*job))
        .cloned()
        .collect::<Vec<_>>();
    for job in archive {
        if let Some(existing) = archived_job(home, &job.id)? {
            if existing != job {
                return Err(invalid("cli_job_archive_conflict"));
            }
        } else {
            fs::write_json_committed(&archive_path(home, &job.id)?, &job)?;
        }
        state.jobs.remove(&job.id);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli_management::{JobStatus, Operation, submit, update};
    use chrono::Utc;

    fn completed(id: &str) -> Job {
        Job {
            id: id.into(),
            runtime: "codex".into(),
            operation: Operation::Install,
            status: JobStatus::Failed,
            created_at: "2026-01-01T00:00:00Z".into(),
            started_at: None,
            finished_at: Some("2026-01-01T00:00:01Z".into()),
            source_rule: None,
            error: Some("fixture".into()),
        }
    }

    #[test]
    fn full_history_preserves_retries_logs_and_new_operations() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let mut state = State::default();
        for index in 0..=RECENT_COMPLETED {
            let job = completed(&format!("job-{index:05}"));
            state.jobs.insert(job.id.clone(), job);
        }
        fs::write_json(&root(&home).join("state.json"), &state).expect("legacy state");
        submit(&home, "claude", Operation::Install, "new", Utc::now())
            .expect("cap no longer blocks");
        assert_eq!(
            load(&home).expect("recent").jobs.len(),
            RECENT_COMPLETED + 1
        );
        let old = &state.jobs["job-00000"];
        assert_eq!(
            find_job(&home, &old.id).expect("archived lookup"),
            Some(old.clone())
        );
        assert_eq!(
            submit(&home, "codex", Operation::Install, &old.id, Utc::now()).expect("retry"),
            *old
        );
        assert!(submit(&home, "claude", Operation::Install, &old.id, Utc::now()).is_err());
        assert_eq!(
            load_with_history(&home).expect("all").jobs.len(),
            RECENT_COMPLETED + 2
        );
        assert!(super::super::read_log(&home, &old.id, 0).is_ok());
        // 模拟归档已写而 state 尚未替换：重试可去重，不能丢失记录。
        fs::write_json(&root(&home).join("state.json"), &state).expect("pre-commit state");
        assert_eq!(
            load_with_history(&home).expect("interrupted archive").jobs,
            state.jobs
        );
        update(&home, |_| Ok(())).expect("resume compact");
        assert_eq!(load_with_history(&home).expect("resumed").jobs, state.jobs);
    }

    #[test]
    fn archive_failure_preserves_recent_state_and_case_sensitive_ids() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        assert_ne!(
            archive_path(&home, "A").expect("upper"),
            archive_path(&home, "a").expect("lower")
        );
        let mut state = State::default();
        for index in 0..=RECENT_COMPLETED {
            let job = completed(&format!("job-{index:05}"));
            state.jobs.insert(job.id.clone(), job);
        }
        fs::write_json(&root(&home).join("state.json"), &state).expect("state");
        std::fs::write(root(&home).join("history"), "blocked").expect("fault");
        assert!(update(&home, |_| Ok(())).is_err());
        assert_eq!(load(&home).expect("unchanged"), state);
    }

    #[test]
    fn interruption_preserves_installation_and_is_not_failure() {
        use crate::cli_management::{Installation, claim_next, finish, interrupt};
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let now = Utc::now();
        submit(&home, "codex", Operation::Install, "install", now).expect("submit");
        claim_next(&home, now).expect("claim");
        let selected = Installation {
            version: "1".into(),
            executable: temp.path().join("codex"),
            bin_paths: vec![],
            installed_at: now.to_rfc3339(),
        };
        finish(&home, "install", Ok(selected.clone()), now).expect("finish");
        for (id, operation) in [
            ("update", Operation::Update),
            ("uninstall", Operation::Uninstall),
            ("install-again", Operation::Install),
        ] {
            submit(&home, "codex", operation, id, now).expect("submit");
            claim_next(&home, now).expect("claim");
            let job = interrupt(&home, id, "stopped".into(), now).expect("interrupt");
            assert_eq!(job.status, JobStatus::Interrupted);
            assert!(job.finished_at.is_some());
            assert_eq!(load(&home).expect("state").installations["codex"], selected);
        }
    }
}
