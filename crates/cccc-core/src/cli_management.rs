//! 实例级 CLI 管理状态，不保存供应商凭据或修改工作组状态。

use crate::{HomeLayout, automation, fs};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Map;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Install,
    #[serde(alias = "upgrade")]
    Update,
    Uninstall,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Interrupted,
}

impl JobStatus {
    pub fn active(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Installation {
    pub version: String,
    pub executable: PathBuf,
    pub bin_paths: Vec<PathBuf>,
    pub installed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub id: String,
    pub enabled: bool,
    pub trigger: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuleState {
    #[serde(flatten)]
    pub rule: Rule,
    pub next_run_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Job {
    pub id: String,
    pub runtime: String,
    pub operation: Operation,
    pub status: JobStatus,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub source_rule: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct State {
    pub revision: u64,
    pub rules: Vec<RuleState>,
    pub installations: BTreeMap<String, Installation>,
    pub jobs: BTreeMap<String, Job>,
}

pub fn root(home: &HomeLayout) -> PathBuf {
    home.root().join("cli-management")
}

/// 以字节游标分页读取完整 JSONL；正在写入的末行留到下次读取，不误报损坏。
pub fn read_log(home: &HomeLayout, id: &str, offset: u64) -> io::Result<Value> {
    use std::io::{BufRead, Read, Seek, SeekFrom};
    validate_id(id)?;
    if !load(home)?.jobs.contains_key(id) {
        return Err(io::Error::new(io::ErrorKind::NotFound, "cli_job_not_found"));
    }
    let file = match std::fs::File::open(root(home).join("logs").join(format!("{id}.jsonl"))) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound && offset == 0 => {
            return Ok(serde_json::json!({"entries":[],"next_offset":0,"has_more":false}));
        }
        Err(error) => return Err(error),
    };
    if offset > file.metadata()?.len() {
        return Err(invalid("invalid_cli_log_offset"));
    }
    let mut reader = std::io::BufReader::new(file);
    reader.seek(SeekFrom::Start(offset))?;
    let mut entries = Vec::new();
    let mut next = offset;
    while entries.len() < 200 && next - offset < 1_048_576 {
        let mut line = Vec::new();
        (&mut reader).take(1_048_577).read_until(b'\n', &mut line)?;
        if line.len() > 1_048_576 {
            return Err(invalid("cli_log_line_too_large"));
        }
        if !line.ends_with(b"\n") {
            break;
        }
        entries.push(serde_json::from_slice::<Value>(&line)?);
        next += line.len() as u64;
        // 每页最多展开一份子进程原始日志，沿用诊断日志的有界尾部视图。
        if entries
            .last()
            .is_some_and(|entry| entry.get("output_file").is_some())
        {
            break;
        }
    }
    Ok(
        serde_json::json!({"entries":entries,"next_offset":next,"has_more":next < reader.get_ref().metadata()?.len()}),
    )
}

pub fn load(home: &HomeLayout) -> io::Result<State> {
    let path = root(home).join("state.json");
    match fs::read_json(&path) {
        Ok(state) => Ok(state),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(State::default()),
        Err(error) => Err(error),
    }
}

/// 仅替换默认程序；保留参数、显式程序及没有受管安装的 Runtime 原有行为。
pub fn apply_command(
    home: &HomeLayout,
    runtime: &str,
    default: &[String],
    command: &mut Vec<String>,
    env: &mut BTreeMap<String, String>,
) -> io::Result<()> {
    if default.is_empty() || (!command.is_empty() && command.first() != default.first()) {
        return Ok(());
    }
    if let Some(executable) = apply_environment(home, runtime, env)? {
        if command.is_empty() {
            command.extend_from_slice(default);
        }
        command[0] = executable.to_string_lossy().into_owned();
    }
    Ok(())
}

/// 为新的启动构造选中版本环境；不修改进程全局 PATH，也不影响已经运行的 Actor。
pub fn apply_environment(
    home: &HomeLayout,
    runtime: &str,
    env: &mut BTreeMap<String, String>,
) -> io::Result<Option<PathBuf>> {
    let Some(installation) = load(home)?.installations.get(runtime).cloned() else {
        return Ok(None);
    };
    let managed_root = root(home).canonicalize()?;
    let executable = installation.executable.clone();
    if !executable.canonicalize()?.starts_with(&managed_root)
        || crate::runtime_mcp::find_program(&executable.to_string_lossy(), None).is_none()
    {
        return Err(invalid("cli_selected_executable_unavailable"));
    }
    let mut paths = vec![
        executable
            .parent()
            .ok_or_else(|| invalid("cli_selected_path_invalid"))?
            .to_path_buf(),
    ];
    for path in installation.bin_paths {
        if !path.is_dir() || !path.canonicalize()?.starts_with(&managed_root) {
            return Err(invalid("cli_selected_dependency_unavailable"));
        }
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    let inherited = env
        .get("PATH")
        .cloned()
        .or_else(|| std::env::var("PATH").ok());
    if let Some(inherited) = inherited {
        for path in std::env::split_paths(&inherited) {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    let path = std::env::join_paths(paths).map_err(io::Error::other)?;
    if runtime == "deepseek" {
        let bin = executable
            .parent()
            .ok_or_else(|| invalid("cli_selected_path_invalid"))?;
        let modules = bin
            .parent()
            .ok_or_else(|| invalid("cli_selected_path_invalid"))?;
        let dsh_home = modules
            .parent()
            .ok_or_else(|| invalid("cli_selected_path_invalid"))?;
        if bin.file_name().and_then(|name| name.to_str()) != Some(".bin")
            || modules.file_name().and_then(|name| name.to_str()) != Some("node_modules")
            || !dsh_home
                .canonicalize()?
                .starts_with(managed_root.join("versions"))
        {
            return Err(invalid("cli_selected_deepseek_root_invalid"));
        }
        env.insert("DSH_HOME".into(), dsh_home.to_string_lossy().into_owned());
    }
    env.insert("PATH".into(), path.to_string_lossy().into_owned());
    Ok(Some(executable))
}

fn update<T>(home: &HomeLayout, change: impl FnOnce(&mut State) -> io::Result<T>) -> io::Result<T> {
    fs::with_exclusive_lock(&root(home).join("state.lock"), || {
        let mut state = load(home)?;
        let previous = state.clone();
        let result = change(&mut state)?;
        if state != previous {
            fs::write_json_committed(&root(home).join("state.json"), &state)?;
        }
        Ok(result)
    })
}

pub fn validate_id(id: &str) -> io::Result<()> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
    {
        return Err(invalid("invalid_cli_management_id"));
    }
    Ok(())
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

// 仅供 CLI 计划适配 Web 星期编号，不改变工作组自动化的原生解释。
fn cron_expression(raw: &str) -> Option<String> {
    let fields = raw.split_whitespace().collect::<Vec<_>>();
    if fields.len() != 5 {
        return Some(raw.to_owned());
    }
    let mut weekdays = Vec::new();
    for item in fields[4].split(',') {
        if item.bytes().any(|byte| byte.is_ascii_alphabetic()) || item == "?" {
            weekdays.push(item.to_owned());
            continue;
        }
        let (range, step) = match item.split_once('/') {
            Some((range, step)) => (range, step.parse::<usize>().ok()?),
            None => (item, 1),
        };
        if step == 0 || step > 7 {
            return None;
        }
        let (start, end) = if range == "*" {
            (0, 6)
        } else if let Some((start, end)) = range.split_once('-') {
            (start.parse::<u8>().ok()?, end.parse::<u8>().ok()?)
        } else {
            let start = range.parse::<u8>().ok()?;
            (start, if item.contains('/') { 7 } else { start })
        };
        if start > end || end > 7 {
            return None;
        }
        weekdays.extend(
            (start..=end)
                .step_by(step)
                .map(|day| (day % 7 + 1).to_string()),
        );
    }
    Some(format!(
        "0 {} {} {} {} {}",
        fields[0],
        fields[1],
        fields[2],
        fields[3],
        weekdays.join(",")
    ))
}

fn next_run_at(
    trigger: &Map<String, Value>,
    last: Option<i64>,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    if trigger.get("kind").and_then(Value::as_str) != Some("cron") {
        return automation::next_rule_fire_at(Some(trigger), last, now);
    }
    let expression = cron_expression(trigger.get("cron")?.as_str()?)?;
    let mut converted = trigger.clone();
    converted.insert("cron".into(), Value::String(expression));
    automation::next_rule_fire_at(Some(&converted), last, now)
}

/// 与现有调度计算共用 trigger 合同，拒绝不受控字段和无效时区。
pub fn validate_rule(rule: &Rule, now: DateTime<Utc>) -> io::Result<()> {
    validate_id(&rule.id)?;
    let trigger = &rule.trigger;
    let allowed: &[&str] = match trigger.get("kind").and_then(Value::as_str) {
        Some("interval") => {
            if !trigger
                .get("every_seconds")
                .and_then(Value::as_i64)
                .is_some_and(|value| (60..=31_536_000).contains(&value))
            {
                return Err(invalid("invalid_cli_schedule_interval"));
            }
            &["kind", "every_seconds"]
        }
        Some("cron") => {
            let cron = trigger.get("cron").and_then(Value::as_str).unwrap_or("");
            if cron.len() > 128
                || cron.split_whitespace().count() != 5
                || trigger.get("timezone").and_then(Value::as_str).is_none()
                || next_run_at(trigger, None, now).is_none()
            {
                return Err(invalid("invalid_cli_schedule_cron"));
            }
            &["kind", "cron", "timezone"]
        }
        Some("at") => {
            if trigger
                .get("at")
                .and_then(Value::as_str)
                .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
                .is_none()
            {
                return Err(invalid("invalid_cli_schedule_time"));
            }
            &["kind", "at"]
        }
        _ => return Err(invalid("invalid_cli_schedule_kind")),
    };
    if trigger.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(invalid("unknown_cli_schedule_field"));
    }
    Ok(())
}

pub fn save_rules(
    home: &HomeLayout,
    expected_revision: u64,
    rules: Vec<Rule>,
    now: DateTime<Utc>,
) -> io::Result<State> {
    if rules.len() > 64 {
        return Err(invalid("too_many_cli_schedules"));
    }
    let mut ids = BTreeSet::new();
    for rule in &rules {
        validate_rule(rule, now)?;
        if !ids.insert(rule.id.clone()) {
            return Err(invalid("duplicate_cli_schedule_id"));
        }
    }
    update(home, |state| {
        if state.revision != expected_revision {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "cli_schedule_revision_conflict",
            ));
        }
        let mut next = Vec::new();
        for rule in rules {
            if let Some(previous) = state.rules.iter().find(|old| old.rule == rule) {
                next.push(previous.clone());
                continue;
            }
            let next_run_at = if rule.enabled {
                next_run_at(&rule.trigger, Some(now.timestamp()), now).or_else(|| {
                    // 一次性计划以指定时间为准，不能把保存时间当成已执行时间。
                    (rule.trigger.get("kind").and_then(Value::as_str) == Some("at"))
                        .then(|| next_run_at(&rule.trigger, None, now))
                        .flatten()
                })
            } else {
                None
            };
            next.push(RuleState { rule, next_run_at });
        }
        state.rules = next;
        state.revision += 1;
        Ok(state.clone())
    })
}

fn enqueue(
    state: &mut State,
    runtime: &str,
    operation: Operation,
    id: &str,
    source_rule: Option<String>,
    now: DateTime<Utc>,
) -> io::Result<Job> {
    validate_id(runtime)?;
    validate_id(id)?;
    if let Some(existing) = state.jobs.get(id) {
        if existing.runtime == runtime && existing.operation == operation {
            return Ok(existing.clone());
        }
        return Err(invalid("cli_request_id_conflict"));
    }
    if state
        .jobs
        .values()
        .any(|job| job.runtime == runtime && job.status.active())
    {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "cli_operation_in_progress",
        ));
    }
    if operation != Operation::Install && !state.installations.contains_key(runtime) {
        return Err(invalid("cli_not_managed"));
    }
    if state.jobs.len() >= 10_000 {
        return Err(invalid("cli_job_history_full"));
    }
    let job = Job {
        id: id.to_owned(),
        runtime: runtime.to_owned(),
        operation,
        status: JobStatus::Queued,
        created_at: now.to_rfc3339(),
        started_at: None,
        finished_at: None,
        source_rule,
        error: None,
    };
    state.jobs.insert(id.to_owned(), job.clone());
    Ok(job)
}

pub fn submit(
    home: &HomeLayout,
    runtime: &str,
    operation: Operation,
    request_id: &str,
    now: DateTime<Utc>,
) -> io::Result<Job> {
    update(home, |state| {
        enqueue(state, runtime, operation, request_id, None, now)
    })
}

/// 到期标记与更新任务同时持久化；重启不重新领取同一次计划。
pub fn enqueue_due(home: &HomeLayout, now: DateTime<Utc>) -> io::Result<Vec<Job>> {
    update(home, |state| {
        let runtimes = state.installations.keys().cloned().collect::<Vec<_>>();
        let mut due = Vec::new();
        for entry in &mut state.rules {
            if !entry.rule.enabled || !entry.next_run_at.is_some_and(|at| at <= now) {
                continue;
            }
            due.push(entry.rule.id.clone());
            entry.next_run_at =
                if entry.rule.trigger.get("kind").and_then(Value::as_str) == Some("at") {
                    None
                } else {
                    next_run_at(&entry.rule.trigger, Some(now.timestamp()), now)
                };
        }
        let mut jobs = Vec::new();
        for rule_id in due {
            for runtime in &runtimes {
                let id = uuid::Uuid::new_v4().to_string();
                match enqueue(
                    state,
                    runtime,
                    Operation::Update,
                    &id,
                    Some(rule_id.clone()),
                    now,
                ) {
                    Ok(job) => jobs.push(job),
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                    Err(error) => return Err(error),
                }
            }
        }
        Ok(jobs)
    })
}

pub fn claim_next(home: &HomeLayout, now: DateTime<Utc>) -> io::Result<Option<Job>> {
    update(home, |state| {
        // 单个后台安装任务，避免不同包管理后端争用共享缓存；不阻塞 Actor。
        if state
            .jobs
            .values()
            .any(|job| job.status == JobStatus::Running)
        {
            return Ok(None);
        }
        let next = state
            .jobs
            .values()
            .filter(|job| job.status == JobStatus::Queued)
            .min_by_key(|job| (&job.created_at, &job.id))
            .map(|job| job.id.clone());
        let Some(id) = next else {
            return Ok(None);
        };
        let job = state.jobs.get_mut(&id).expect("selected queued job");
        job.status = JobStatus::Running;
        job.started_at = Some(now.to_rfc3339());
        Ok(Some(job.clone()))
    })
}

/// 安装验证通过后才记录选中版本；失败不会改变旧安装选择。
pub fn finish(
    home: &HomeLayout,
    id: &str,
    result: Result<Installation, String>,
    now: DateTime<Utc>,
) -> io::Result<Job> {
    finish_result(home, id, result.map(Some), now)
}

/// 卸载删除成功后才移除选择；失败/中断保留记录，禁止静默切换外部安装。
pub fn finish_uninstall(
    home: &HomeLayout,
    id: &str,
    result: Result<(), String>,
    now: DateTime<Utc>,
) -> io::Result<Job> {
    finish_result(home, id, result.map(|()| None), now)
}

fn finish_result(
    home: &HomeLayout,
    id: &str,
    result: Result<Option<Installation>, String>,
    now: DateTime<Utc>,
) -> io::Result<Job> {
    update(home, |state| {
        let job = state
            .jobs
            .get_mut(id)
            .ok_or_else(|| invalid("cli_job_not_found"))?;
        if job.status != JobStatus::Running {
            return Err(invalid("cli_job_not_running"));
        }
        match result {
            Ok(Some(installation)) if job.operation != Operation::Uninstall => {
                state
                    .installations
                    .insert(job.runtime.clone(), installation);
                job.status = JobStatus::Succeeded;
            }
            Ok(None) if job.operation == Operation::Uninstall => {
                state.installations.remove(&job.runtime);
                job.status = JobStatus::Succeeded;
            }
            Ok(_) => return Err(invalid("cli_operation_result_mismatch")),
            Err(error) => {
                job.status = JobStatus::Failed;
                job.error = Some(error);
            }
        }
        job.finished_at = Some(now.to_rfc3339());
        Ok(job.clone())
    })
}

/// 仅由持有实例后台工作锁的新执行者在启动时调用。
pub fn recover_interrupted(home: &HomeLayout, now: DateTime<Utc>) -> io::Result<usize> {
    update(home, |state| {
        let mut count = 0;
        for job in state
            .jobs
            .values_mut()
            .filter(|job| job.status == JobStatus::Running)
        {
            job.status = JobStatus::Interrupted;
            job.error = Some("cli_worker_interrupted".into());
            job.finished_at = Some(now.to_rfc3339());
            count += 1;
        }
        Ok(count)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeDelta;
    use serde_json::json;

    #[test]
    fn cli_cron_conversion_preserves_saved_rules_and_reschedules_after_reload() {
        for (day, expected) in [
            ("0", "2026-09-12T19:00:00Z"),
            ("7", "2026-09-12T19:00:00Z"),
            ("1", "2026-09-13T19:00:00Z"),
            ("2", "2026-09-07T19:00:00Z"),
            ("3", "2026-09-08T19:00:00Z"),
            ("4", "2026-09-09T19:00:00Z"),
            ("5", "2026-09-10T19:00:00Z"),
            ("6", "2026-09-11T19:00:00Z"),
            ("1-5", "2026-09-07T19:00:00Z"),
            ("1,5", "2026-09-10T19:00:00Z"),
            ("1-5/2", "2026-09-08T19:00:00Z"),
            ("*/2", "2026-09-07T19:00:00Z"),
            ("5/2", "2026-09-10T19:00:00Z"),
            ("MON", "2026-09-13T19:00:00Z"),
            ("MON-FRI", "2026-09-07T19:00:00Z"),
        ] {
            let (_temp, home, now) = fixture();
            installed(&home, now);
            let rule = rule(
                "weekly",
                json!({
                    "kind":"cron", "cron":format!("0 3 * * {day}"), "timezone":"Asia/Shanghai"
                }),
            );
            let expected: DateTime<Utc> = expected.parse().unwrap();
            save_rules(&home, 0, vec![rule.clone()], now).unwrap();
            let stored = load(&home).unwrap();
            assert_eq!(stored.rules[0].rule, rule);
            assert_eq!(stored.rules[0].next_run_at, Some(expected), "{day}");
            assert!(
                enqueue_due(&home, expected - TimeDelta::seconds(1))
                    .unwrap()
                    .is_empty()
            );
            let jobs = enqueue_due(&home, expected).unwrap();
            assert_eq!(jobs.len(), 1, "{day}");
            assert_eq!(jobs[0].operation, Operation::Update);
            assert_eq!(jobs[0].source_rule.as_deref(), Some("weekly"));
            let reloaded = load(&home).unwrap();
            assert_eq!(reloaded.rules[0].rule, rule);
            assert_eq!(
                reloaded.rules[0].next_run_at,
                next_run_at(&rule.trigger, Some(expected.timestamp()), expected)
            );
            assert!(reloaded.rules[0].next_run_at.unwrap() > expected);
            if day == "1" {
                assert_eq!(
                    reloaded.rules[0].next_run_at,
                    Some(expected + TimeDelta::days(7))
                );
            }
            assert!(enqueue_due(&home, expected).unwrap().is_empty());
        }
    }

    #[test]
    fn cli_cron_keeps_calendar_and_validation_boundaries() {
        let now = "2026-04-30T04:00:00Z".parse().unwrap();
        for (expression, expected) in [
            ("0 3 31 * *", "2026-05-31T03:00:00Z"),
            ("0 3 * * *", "2026-05-01T03:00:00Z"),
        ] {
            let rule = rule(
                "calendar",
                json!({"kind":"cron","cron":expression,"timezone":"UTC"}),
            );
            validate_rule(&rule, now).unwrap();
            assert_eq!(
                next_run_at(&rule.trigger, None, now),
                Some(expected.parse().unwrap())
            );
        }
        for expression in ["0 3 * * 8", "0 3 * * */0", "0 3 * * 5-1", "0 0 3 * * 1"] {
            let rule = rule(
                "invalid",
                json!({"kind":"cron","cron":expression,"timezone":"UTC"}),
            );
            assert!(validate_rule(&rule, now).is_err(), "{expression}");
        }
    }

    #[test]
    fn cli_conversion_does_not_change_group_automation_weekdays() {
        let now = "2026-09-07T00:00:00Z".parse().unwrap();
        let trigger = json!({"kind":"cron","cron":"0 3 * * 1","timezone":"UTC"});
        let trigger = trigger.as_object().unwrap();
        let sunday: DateTime<Utc> = "2026-09-13T03:00:00Z".parse().unwrap();
        let monday: DateTime<Utc> = "2026-09-07T03:00:00Z".parse().unwrap();
        assert_eq!(
            automation::next_rule_fire_at(Some(trigger), None, now),
            Some(sunday)
        );
        assert_eq!(next_run_at(trigger, None, now), Some(monday));
        // CLI 适配没有修改输入；工作组的下次时间及到期判断仍用原生库编号。
        assert_eq!(trigger["cron"], "0 3 * * 1");
        assert_eq!(
            automation::next_rule_fire_at(Some(trigger), None, now),
            Some(sunday)
        );
        assert!(crate::automation_schedule::is_due(
            Some(trigger),
            Some(sunday.timestamp() - 1),
            sunday
        ));
        assert!(!crate::automation_schedule::is_due(
            Some(trigger),
            Some(monday.timestamp() - 1),
            monday
        ));
        for expression in ["0 0 3 * * 1", "0 0 3 * * 1 *"] {
            let trigger = json!({"kind":"cron","cron":expression,"timezone":"UTC"});
            assert_eq!(
                automation::next_rule_fire_at(trigger.as_object(), None, now),
                Some(sunday)
            );
        }
    }

    #[test]
    fn update_accepts_legacy_operation_and_serializes_canonically() {
        for input in ["update", "upgrade"] {
            let operation: Operation = serde_json::from_value(json!(input)).unwrap();
            assert_eq!(operation, Operation::Update);
            assert_eq!(serde_json::to_value(operation).unwrap(), json!("update"));
        }
    }

    #[test]
    fn legacy_update_record_keeps_identity_and_deduplicates_after_reload() {
        let (_temp, home, now) = fixture();
        installed(&home, now);
        let job = submit(&home, "codex", Operation::Update, "legacy-request", now).unwrap();
        let mut stored = serde_json::to_value(load(&home).unwrap()).unwrap();
        stored["jobs"]["legacy-request"]["operation"] = json!("upgrade");
        fs::write_json_committed(&root(&home).join("state.json"), &stored).unwrap();
        assert_eq!(load(&home).unwrap().jobs["legacy-request"], job);
        assert_eq!(
            submit(&home, "codex", Operation::Update, "legacy-request", now).unwrap(),
            job
        );
        assert_eq!(load(&home).unwrap().jobs.len(), 2);
        assert_eq!(
            claim_next(&home, now).unwrap().unwrap().operation,
            Operation::Update
        );
        let stored: Value = fs::read_json(&root(&home).join("state.json")).unwrap();
        assert_eq!(stored["jobs"]["legacy-request"]["operation"], "update");
    }

    fn fixture() -> (tempfile::TempDir, HomeLayout, DateTime<Utc>) {
        let temp = tempfile::tempdir().unwrap();
        let home = HomeLayout::from_path(temp.path().join("home")).unwrap();
        home.initialize().unwrap();
        (temp, home, "2026-09-07T00:00:00Z".parse().unwrap())
    }

    fn rule(id: &str, trigger: Value) -> Rule {
        Rule {
            id: id.into(),
            enabled: true,
            trigger: trigger.as_object().unwrap().clone(),
        }
    }

    fn installed(home: &HomeLayout, now: DateTime<Utc>) {
        submit(home, "codex", Operation::Install, "install-1", now).unwrap();
        claim_next(home, now).unwrap().unwrap();
        finish(
            home,
            "install-1",
            Ok(Installation {
                version: "1.0".into(),
                executable: root(home).join("versions/1.0/codex"),
                bin_paths: vec![],
                installed_at: now.to_rfc3339(),
            }),
            now,
        )
        .unwrap();
    }

    #[test]
    fn uninstall_clears_only_managed_selection_and_remains_idempotent() {
        let (_temp, home, now) = fixture();
        assert!(submit(&home, "codex", Operation::Uninstall, "remove", now).is_err());
        installed(&home, now);
        save_rules(
            &home,
            load(&home).unwrap().revision,
            vec![rule(
                "daily",
                json!({"kind":"cron","cron":"0 3 * * *","timezone":"UTC"}),
            )],
            now,
        )
        .unwrap();
        let job = submit(&home, "codex", Operation::Uninstall, "remove", now).unwrap();
        assert_eq!(
            submit(&home, "codex", Operation::Uninstall, "remove", now).unwrap(),
            job
        );
        assert!(submit(&home, "codex", Operation::Update, "update", now).is_err());
        claim_next(&home, now).unwrap();
        finish_uninstall(&home, "remove", Err("in use".into()), now).unwrap();
        assert!(load(&home).unwrap().installations.contains_key("codex"));
        submit(&home, "codex", Operation::Uninstall, "retry", now).unwrap();
        claim_next(&home, now).unwrap();
        finish_uninstall(&home, "retry", Ok(()), now).unwrap();
        assert!(load(&home).unwrap().installations.is_empty());
        assert_eq!(
            submit(&home, "codex", Operation::Uninstall, "retry", now)
                .unwrap()
                .status,
            JobStatus::Succeeded
        );
        assert_eq!(load(&home).unwrap().jobs.len(), 3);
        assert_eq!(load(&home).unwrap().rules.len(), 1);
        assert!(
            enqueue_due(&home, now + TimeDelta::days(1))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn rejects_invalid_schedules_and_stale_edits_without_mutation() {
        let (_temp, home, now) = fixture();
        let weekly = rule(
            "weekly",
            json!({"kind":"cron","cron":"0 3 * * 1","timezone":"Asia/Shanghai"}),
        );
        let saved = save_rules(&home, 0, vec![weekly.clone()], now).unwrap();
        assert_eq!(
            saved.rules[0].next_run_at.unwrap().to_rfc3339(),
            "2026-09-13T19:00:00+00:00"
        );
        assert!(save_rules(&home, 0, vec![], now).is_err());
        let bad = rule(
            "bad",
            json!({"kind":"cron","cron":"0 3 * * 1","timezone":"not-a-zone"}),
        );
        assert!(save_rules(&home, 1, vec![bad], now).is_err());
        assert!(save_rules(&home, 1, vec![weekly.clone(), weekly], now).is_err());
        assert_eq!(load(&home).unwrap(), saved);
    }

    #[test]
    fn failure_preserves_selected_version_and_retry_is_idempotent() {
        let (_temp, home, now) = fixture();
        installed(&home, now);
        let previous = load(&home).unwrap().installations;
        let job = submit(&home, "codex", Operation::Update, "update-1", now).unwrap();
        assert_eq!(
            submit(&home, "codex", Operation::Update, "update-1", now).unwrap(),
            job
        );
        assert!(submit(&home, "codex", Operation::Update, "update-2", now).is_err());
        claim_next(&home, now).unwrap().unwrap();
        finish(&home, "update-1", Err("verification failed".into()), now).unwrap();
        assert_eq!(load(&home).unwrap().installations, previous);
        assert!(submit(&home, "codex", Operation::Install, "update-1", now).is_err());
    }

    #[test]
    fn due_rules_are_atomic_and_do_not_update_unmanaged_tools() {
        let (_temp, home, now) = fixture();
        installed(&home, now);
        save_rules(
            &home,
            0,
            vec![rule(
                "interval",
                json!({"kind":"interval","every_seconds":60}),
            )],
            now,
        )
        .unwrap();
        assert!(enqueue_due(&home, now).unwrap().is_empty());
        let due = now + TimeDelta::seconds(61);
        let jobs = enqueue_due(&home, due).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].runtime, "codex");
        assert_eq!(jobs[0].operation, Operation::Update);
        assert!(enqueue_due(&home, due).unwrap().is_empty());
        claim_next(&home, due).unwrap();
        assert_eq!(recover_interrupted(&home, due).unwrap(), 1);
        assert_eq!(recover_interrupted(&home, due).unwrap(), 0);
        assert!(claim_next(&home, due).unwrap().is_none());
        assert_eq!(load(&home).unwrap().installations["codex"].version, "1.0");
    }

    #[test]
    fn one_time_disabled_and_multiple_weekly_rules_keep_their_boundaries() {
        let (_temp, home, now) = fixture();
        installed(&home, now);
        let once = rule("once", json!({"kind":"at","at":now.to_rfc3339()}));
        let mut disabled = rule("disabled", json!({"kind":"interval","every_seconds":60}));
        disabled.enabled = false;
        let monday = rule(
            "monday",
            json!({"kind":"cron","cron":"0 3 * * 1","timezone":"UTC"}),
        );
        let friday = rule(
            "friday",
            json!({"kind":"cron","cron":"0 3 * * 5","timezone":"UTC"}),
        );
        let state = save_rules(&home, 0, vec![once, disabled, monday, friday], now).unwrap();
        assert!(state.rules[1].next_run_at.is_none());
        assert_ne!(state.rules[2].next_run_at, state.rules[3].next_run_at);
        assert_eq!(enqueue_due(&home, now).unwrap().len(), 1);
        assert!(enqueue_due(&home, now).unwrap().is_empty());
        assert!(load(&home).unwrap().rules[0].next_run_at.is_none());
    }

    #[test]
    fn rejects_corrupt_state_instead_of_resetting_user_data() {
        let (_temp, home, _now) = fixture();
        fs::atomic_write(&root(&home).join("state.json"), b"broken").unwrap();
        assert!(load(&home).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn selected_cli_environment_does_not_change_global_path_or_silently_fallback() {
        use std::os::unix::fs::PermissionsExt;
        let (_temp, home, now) = fixture();
        installed(&home, now);
        let executable = load(&home).unwrap().installations["codex"]
            .executable
            .clone();
        fs::atomic_write(&executable, b"#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let global = std::env::var_os("PATH");
        let mut env = BTreeMap::from([
            ("PATH".into(), "/usr/bin".into()),
            ("ACTOR_SETTING".into(), "retained".into()),
        ]);
        assert_eq!(
            apply_environment(&home, "codex", &mut env).unwrap(),
            Some(executable.clone())
        );
        assert_eq!(
            std::env::split_paths(&env["PATH"]).next().unwrap(),
            executable.parent().unwrap()
        );
        assert_eq!(env["ACTOR_SETTING"], "retained");
        assert_eq!(std::env::var_os("PATH"), global);
        std::fs::rename(&executable, executable.with_extension("missing")).unwrap();
        let before = env.clone();
        assert!(apply_environment(&home, "codex", &mut env).is_err());
        assert_eq!(env, before);
        assert!(
            apply_environment(&home, "custom", &mut env)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn log_cursor_paginates_and_retries_partial_records_without_losing_text() {
        let (_temp, home, now) = fixture();
        submit(&home, "codex", Operation::Install, "log-test", now).unwrap();
        let path = root(&home).join("logs/log-test.jsonl");
        let mut contents = (0..205)
            .map(|index| format!("{{\"text\":\"第{index}行\"}}\n"))
            .collect::<String>();
        let complete_length = contents.len() as u64;
        contents.push_str("{\"text\":\"尚未完成");
        fs::atomic_write(&path, contents.as_bytes()).unwrap();
        let first = read_log(&home, "log-test", 0).unwrap();
        assert_eq!(first["entries"].as_array().unwrap().len(), 200);
        let second = read_log(&home, "log-test", first["next_offset"].as_u64().unwrap()).unwrap();
        assert_eq!(second["entries"].as_array().unwrap().len(), 5);
        assert_eq!(second["next_offset"], complete_length);
        assert!(second["has_more"].as_bool().unwrap());
        assert!(
            read_log(&home, "log-test", complete_length).unwrap()["entries"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        contents.push_str("\"}\n");
        fs::atomic_write(&path, contents.as_bytes()).unwrap();
        let last = read_log(&home, "log-test", complete_length).unwrap();
        assert_eq!(last["entries"][0]["text"], "尚未完成");
        assert_eq!(last["has_more"], false);
        assert!(read_log(&home, "../log-test", 0).is_err());
        assert!(read_log(&home, "unknown", 0).is_err());
        assert!(read_log(&home, "log-test", contents.len() as u64 + 1).is_err());
    }
}
