use cccc_contracts::{ActorRuntime, DaemonRequest};
use cccc_core::{HomeLayout, cli_management as management};
use chrono::Utc;
use serde::Serialize;
use serde_json::{Value, json};
use std::io;

use super::operation::{Operation, Policy};
use crate::dispatch::{OpError, OpResult, object, required_arg};

mod distribution;
mod hermes;
mod install;
mod process;
mod uninstall;
pub(crate) mod usage;

// 存储故障不能依赖同一块磁盘记录自身；沿用状态接口的错误反馈，不另建健康 API。
fn worker_failures() -> std::sync::MutexGuard<'static, std::collections::HashSet<std::path::PathBuf>>
{
    static FAILURES: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashSet<std::path::PathBuf>>,
    > = std::sync::OnceLock::new();
    FAILURES
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

pub(crate) struct Worker {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    task: tokio::task::JoinHandle<()>,
}

impl Worker {
    pub(crate) fn start(home: HomeLayout) -> Self {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let task = tokio::spawn(async move {
            let mut recovered = false;
            while !worker_stop.load(Ordering::Acquire) {
                let tick_home = home.clone();
                let tick_stop = Arc::clone(&worker_stop);
                let tick = tokio::task::spawn_blocking(move || {
                    let home = tick_home;
                    let worker_stop = tick_stop;
                    let result = (|| -> io::Result<()> {
                        if worker_stop.load(Ordering::Acquire) {
                            return Ok(());
                        }
                        let state = management::load(&home)?;
                        if !recovered {
                            if state
                                .jobs
                                .values()
                                .any(|job| job.status == management::JobStatus::Running)
                            {
                                management::recover_interrupted(&home, Utc::now())?;
                            }
                            recovered = true;
                        }
                        if state.rules.iter().any(|rule| {
                            rule.rule.enabled && rule.next_run_at.is_some_and(|at| at <= Utc::now())
                        }) {
                            management::enqueue_due(&home, Utc::now())?;
                        }
                        if !worker_stop.load(Ordering::Acquire)
                            && management::load(&home)?
                                .jobs
                                .values()
                                .any(|job| job.status == management::JobStatus::Queued)
                            && let Some(job) = management::claim_next(&home, Utc::now())?
                        {
                            execute_job(&home, &job, &worker_stop)?;
                        }
                        Ok(())
                    })();
                    (recovered, result)
                })
                .await;
                let result = match tick {
                    Ok((did_recover, result)) => {
                        recovered = did_recover;
                        result
                    }
                    Err(error) => Err(io::Error::other(error.to_string())),
                };
                if let Err(error) = result {
                    // 持久化失败后不能永久留下 Running。恢复读写后重新核对旧任务，
                    // 标记为中断而不是重复安装；已经保存成功的任务不会被改写。
                    recovered = false;
                    worker_failures().insert(home.root().to_path_buf());
                    tracing::error!(%error, "CLI management worker failed");
                } else {
                    worker_failures().remove(home.root());
                }
                for _ in 0..4 {
                    if worker_stop.load(Ordering::Acquire) {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                }
            }
            worker_failures().remove(home.root());
        });
        Self { stop, task }
    }

    pub(crate) async fn finish(mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        // 比原生轻量服务多留出进程树回收的 5 秒预算；不无上限等待。
        match tokio::time::timeout(std::time::Duration::from_secs(6), &mut self.task).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => tracing::error!(%error, "CLI management worker join failed"),
            Err(_) => {
                self.task.abort();
                let _ = (&mut self.task).await;
                tracing::warn!(
                    "CLI management shutdown timed out; stop requested, blocking cleanup may still be pending"
                );
            }
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
    }
}

fn execute_job(
    home: &HomeLayout,
    job: &management::Job,
    stop: &std::sync::atomic::AtomicBool,
) -> io::Result<()> {
    if job.operation == management::Operation::Uninstall {
        let result =
            process::Log::open(home, &job.id).and_then(|log| uninstall::run(home, job, &log, stop));
        if let Err(error) = result {
            let log = process::Log::open(home, &job.id)?;
            let message = log.redact(&error.to_string());
            log.write("error", &message)?;
            log.sync()?;
            management::finish_uninstall(home, &job.id, Err(message), Utc::now())?;
        }
        return Ok(());
    }
    let result = (|| -> io::Result<management::Installation> {
        let log = process::Log::open(home, &job.id)?;
        log.write(
            "stage",
            &format!("开始 {:?}：{}，任务 {}", job.operation, job.runtime, job.id),
        )?;
        let result = install::install(home, job, &log, stop);
        match result {
            Ok(installation) => {
                log.write(
                    "stage",
                    "验证完成；选中版本仅供后续 Actor 启动使用，现有 Actor 不受影响",
                )?;
                log.sync()?;
                Ok(installation)
            }
            Err(error) => {
                let message = log.redact(&error.to_string());
                log.write("error", &message)?;
                log.sync()?;
                Err(io::Error::new(error.kind(), message))
            }
        }
    })();
    management::finish(
        home,
        &job.id,
        result.map_err(|error| error.to_string()),
        Utc::now(),
    )?;
    Ok(())
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum Source {
    Mise { tool: &'static str, node: bool },
    Official { distribution: &'static str },
    Deepseek,
    NotApplicable { reason: &'static str },
}

// 穷尽 Runtime 枚举；上游增加 Runtime 时必须重新核对安装来源，不能悄悄漏项。
pub(super) fn source(runtime: ActorRuntime) -> Source {
    let (tool, node) = match runtime {
        ActorRuntime::Amp => ("amp", true),
        ActorRuntime::Antigravity => ("antigravity-cli", false),
        ActorRuntime::Auggie => ("npm:@augmentcode/auggie", true),
        ActorRuntime::Claude => ("claude", false),
        ActorRuntime::Cline => ("npm:cline", true),
        ActorRuntime::Codex => ("codex", false),
        ActorRuntime::Copilot => ("copilot", false),
        ActorRuntime::Cursor => ("cursor-agent", false),
        ActorRuntime::Kilo => ("npm:@kilocode/cli", true),
        ActorRuntime::Grok => ("grok", false),
        ActorRuntime::Kimi => ("npm:@moonshot-ai/kimi-code", true),
        ActorRuntime::Opencode => ("opencode", false),
        ActorRuntime::Deepseek => return Source::Deepseek,
        ActorRuntime::Devin => {
            return Source::Official {
                distribution: "devin",
            };
        }
        ActorRuntime::Kiro => {
            return Source::Official {
                distribution: "kiro",
            };
        }
        ActorRuntime::Droid => {
            return Source::Official {
                distribution: "droid",
            };
        }
        ActorRuntime::Hermes => {
            return Source::Official {
                distribution: "hermes",
            };
        }
        ActorRuntime::WebModel => {
            return Source::NotApplicable {
                reason: "runtime_is_not_cli",
            };
        }
        ActorRuntime::Custom => {
            return Source::NotApplicable {
                reason: "custom_install_source_unknown",
            };
        }
    };
    Source::Mise { tool, node }
}

pub(super) fn resolve_operation(request: &DaemonRequest) -> Option<Operation> {
    let policy = match request.op.as_str() {
        "cli_management_get" | "cli_management_log" => Policy::Read,
        "cli_management_submit"
        | "cli_management_schedules_update"
        | "cli_management_uninstall" => Policy::GlobalWrite,
        _ => return None,
    };
    Some(Operation::new(policy, handle))
}

fn handle(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    authorize(request).and_then(|()| match request.op.as_str() {
        "cli_management_get" => status(home, request),
        "cli_management_log" => read_log(home, request),
        "cli_management_schedules_update" => save_schedules(home, request),
        "cli_management_submit" => submit(home, request),
        _ => {
            fields(request, &["runtime", "request_id"])?;
            let mut request = request.clone();
            request.args.insert("operation".into(), json!("uninstall"));
            submit(home, &request)
        }
    })
}

fn authorize(request: &DaemonRequest) -> Result<(), OpError> {
    if request.args.get("by").and_then(Value::as_str) != Some("user") {
        return Err(OpError::new(
            "permission_denied",
            "CLI 管理仅允许用户控制操作",
        ));
    }
    Ok(())
}

fn fields(request: &DaemonRequest, allowed: &[&str]) -> Result<(), OpError> {
    if request
        .args
        .keys()
        .any(|key| key != "by" && !allowed.contains(&key.as_str()))
    {
        return Err(OpError::new("invalid_args", "包含不支持的 CLI 管理字段"));
    }
    Ok(())
}

fn state_error(error: io::Error) -> OpError {
    let code = match error.kind() {
        io::ErrorKind::AlreadyExists => "cli_schedule_revision_conflict",
        io::ErrorKind::WouldBlock => "cli_operation_busy",
        io::ErrorKind::InvalidInput => "invalid_args",
        io::ErrorKind::NotFound => "cli_job_not_found",
        _ => "cli_management_state_error",
    };
    OpError::new(code, error.to_string())
}

fn read_log(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    fields(request, &["job_id", "offset"])?;
    let id = required_arg(request, "job_id")?;
    let offset = request
        .args
        .get("offset")
        .and_then(Value::as_u64)
        .ok_or_else(|| OpError::new("invalid_args", "offset 必须是非负整数"))?;
    object(process::read_log(home, &id, offset).map_err(state_error)?)
}

fn status(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    fields(request, &[])?;
    if worker_failures().contains(home.root()) {
        return Err(OpError::new(
            "cli_worker_unavailable",
            "CLI 管理后台发生错误；请检查 CCCC 守护进程日志和存储状态，恢复后刷新重试",
        ));
    }
    let state = management::load(home).map_err(state_error)?;
    let runtimes = cccc_runtime::detect_runtimes()
        .into_iter()
        .map(|probe| {
            let runtime: ActorRuntime =
                serde_json::from_value(json!(probe.name)).map_err(OpError::invalid)?;
            let managed_error = if state.installations.contains_key(&probe.name) {
                management::apply_environment(home, &probe.name, &mut Default::default())
                    .err().map(|error| error.to_string())
            } else { None };
            Ok(json!({
                "name":probe.name, "display_name":probe.display_name, "command":probe.command,
                "external_available":probe.available, "external_path":probe.path,
                "source":source(runtime), "installation":state.installations.get(&probe.name),
                "managed_error":managed_error,
                "uninstall_available":state.installations.contains_key(&probe.name),
                "uninstall_reason": if state.installations.contains_key(&probe.name) { Value::Null } else { json!("cli_not_managed") },
            }))
        })
        .collect::<Result<Vec<_>, OpError>>()?;
    object(json!({"runtimes":runtimes,"state":state}))
}

fn submit(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    fields(request, &["runtime", "operation", "request_id"])?;
    let runtime_name = required_arg(request, "runtime")?;
    let runtime: ActorRuntime =
        serde_json::from_value(json!(runtime_name)).map_err(OpError::invalid)?;
    if let Source::NotApplicable { reason } = source(runtime) {
        return Err(OpError::new(reason, "此 Runtime 不适用自动 CLI 安装与更新"));
    }
    let operation = serde_json::from_value(
        request
            .args
            .get("operation")
            .cloned()
            .unwrap_or(Value::Null),
    )
    .map_err(OpError::invalid)?;
    let request_id = required_arg(request, "request_id")?;
    let job = management::submit(home, &runtime_name, operation, &request_id, Utc::now())
        .map_err(state_error)?;
    object(json!({"job":job}))
}

fn save_schedules(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    fields(request, &["revision", "rules"])?;
    let revision = request
        .args
        .get("revision")
        .and_then(Value::as_u64)
        .ok_or_else(|| OpError::new("invalid_args", "revision 必须是非负整数"))?;
    let rules = serde_json::from_value(request.args.get("rules").cloned().unwrap_or(Value::Null))
        .map_err(OpError::invalid)?;
    let state = management::save_rules(home, revision, rules, Utc::now()).map_err(state_error)?;
    object(json!({"state":state}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatch::dispatch;

    fn request(op: &str, args: Value) -> DaemonRequest {
        DaemonRequest {
            v: 1,
            op: op.into(),
            args: args.as_object().unwrap().clone(),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_bounds_a_stalled_async_task() {
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let task = tokio::spawn(std::future::pending::<()>());
        let observer = task.abort_handle();
        let worker = Worker {
            stop: stop.clone(),
            task,
        };
        tokio::time::timeout(std::time::Duration::from_secs(7), worker.finish())
            .await
            .unwrap();
        assert!(stop.load(std::sync::atomic::Ordering::Acquire));
        assert!(observer.is_finished());
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_waits_for_owned_process_cleanup_without_stopping_others() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        use std::time::Duration;
        let temp = tempfile::tempdir().unwrap();
        let home = HomeLayout::from_path(temp.path().join("home")).unwrap();
        let marker = temp.path().join("started");
        let late = temp.path().join("late");
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let command_marker = marker.clone();
        let late_marker = late.clone();
        let cleaned = Arc::new(AtomicBool::new(false));
        let worker_cleaned = cleaned.clone();
        let mut other = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let task = tokio::spawn(async move {
            tokio::task::spawn_blocking(move || {
                let log = process::Log::open(&home, "shutdown").unwrap();
                let mut command = std::process::Command::new("/bin/sh");
                command
                    .args([
                        "-c",
                        "touch \"$CLI_STARTED\"; (sleep 1; touch \"$CLI_LATE\") & wait",
                    ])
                    .env("CLI_STARTED", command_marker)
                    .env("CLI_LATE", late_marker);
                let result = process::run(
                    &mut command,
                    &log,
                    &worker_stop,
                    Duration::from_secs(30),
                    false,
                );
                assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Interrupted);
                worker_cleaned.store(true, Ordering::Release);
            })
            .await
            .unwrap();
        });
        let worker = Worker { stop, task };
        let started = tokio::time::timeout(Duration::from_secs(3), async {
            while !marker.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
        worker.finish().await;
        let other_running = other.try_wait().unwrap().is_none();
        other.kill().unwrap();
        other.wait().unwrap();
        started.unwrap();
        assert!(cleaned.load(Ordering::Acquire));
        assert!(other_running);
        tokio::time::sleep(Duration::from_millis(1100)).await;
        assert!(!late.exists());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn storage_fault_is_visible_and_recovery_does_not_replay_running_jobs() {
        let temp = tempfile::tempdir().unwrap();
        let home = HomeLayout::from_path(temp.path().join("faulted")).unwrap();
        let other = HomeLayout::from_path(temp.path().join("unaffected")).unwrap();
        let now = Utc::now();
        management::submit(
            &home,
            "old-runtime-one",
            management::Operation::Install,
            "running",
            now,
        )
        .unwrap();
        management::claim_next(&home, now).unwrap();
        management::submit(
            &home,
            "old-runtime-two",
            management::Operation::Install,
            "queued",
            now,
        )
        .unwrap();
        let lock = management::root(&home).join("state.lock");
        let preserved = lock.with_extension("preserved");
        std::fs::rename(&lock, &preserved).unwrap();
        // 仅阻止当前临时实例的状态写入，已有状态仍可读，不改宿主文件系统权限。
        std::fs::create_dir(&lock).unwrap();
        let worker = Worker::start(home.clone());
        let fault_observed = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while !worker_failures().contains(home.root()) {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .is_ok();
        let get = request("cli_management_get", json!({"by":"user"}));
        let fault = dispatch(&home, &get);
        let unaffected = dispatch(&other, &get);
        let before = management::load(&home).unwrap();
        std::fs::remove_dir(&lock).unwrap();
        std::fs::rename(&preserved, &lock).unwrap();
        let recovered = tokio::time::timeout(std::time::Duration::from_secs(4), async {
            loop {
                let state = management::load(&home).unwrap();
                if !worker_failures().contains(home.root())
                    && state.jobs["queued"].status == management::JobStatus::Failed
                {
                    break state;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await;
        worker.finish().await;
        assert!(fault_observed);
        assert_eq!(fault.error.unwrap().code, "cli_worker_unavailable");
        assert!(unaffected.ok);
        assert_eq!(
            before.jobs["running"].status,
            management::JobStatus::Running
        );
        assert_eq!(before.jobs["queued"].status, management::JobStatus::Queued);
        let recovered = recovered.unwrap();
        assert_eq!(
            recovered.jobs["running"].status,
            management::JobStatus::Interrupted
        );
        assert!(!management::root(&home).join("logs/running.jsonl").exists());
        assert!(recovered.installations.is_empty());
        assert!(dispatch(&home, &get).ok);
    }

    #[test]
    fn all_catalog_runtimes_have_an_explicit_installation_decision() {
        let runtimes = cccc_runtime::detect_runtimes();
        assert_eq!(runtimes.len(), 19);
        let not_applicable = runtimes
            .iter()
            .filter(|probe| {
                let runtime = serde_json::from_value(json!(probe.name)).unwrap();
                matches!(source(runtime), Source::NotApplicable { .. })
            })
            .map(|probe| probe.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(not_applicable, ["web_model", "custom"]);
    }

    #[test]
    fn retired_notification_operation_is_not_handled() {
        let temp = tempfile::tempdir().unwrap();
        let home = HomeLayout::from_path(temp.path()).unwrap();
        assert!(
            resolve_operation(&request(
                "cli_management_notification_update",
                json!({"by":"user"})
            ))
            .is_none()
        );
        assert!(!management::root(&home).exists());
    }

    #[test]
    fn cli_operation_policies_use_native_registry_without_resolving_io() {
        for op in ["cli_management_get", "cli_management_log"] {
            let operation = crate::dispatch::resolve_operation(&request(op, json!({}))).unwrap();
            assert!(matches!(operation.policy, Policy::Read));
        }
        for op in [
            "cli_management_submit",
            "cli_management_schedules_update",
            "cli_management_uninstall",
        ] {
            let operation = crate::dispatch::resolve_operation(&request(op, json!({}))).unwrap();
            assert!(matches!(operation.policy, Policy::GlobalWrite));
        }
        assert!(
            crate::dispatch::resolve_operation(&request("cli_management_unknown", json!({})))
                .is_none()
        );
    }

    #[test]
    fn unmanaged_uninstall_and_actor_requests_do_not_create_jobs() {
        let temp = tempfile::tempdir().unwrap();
        let home = HomeLayout::from_path(temp.path()).unwrap();
        for op in [
            "cli_management_get",
            "cli_management_submit",
            "cli_management_schedules_update",
            "cli_management_uninstall",
        ] {
            let response = dispatch(&home, &request(op, json!({"by":"codex"})));
            assert_eq!(response.error.unwrap().code, "permission_denied");
        }
        for (op, args) in [
            ("cli_management_uninstall", json!({"by":"user"})),
            (
                "cli_management_submit",
                json!({"by":"user","runtime":"codex","operation":"uninstall","request_id":"uninstall-1"}),
            ),
        ] {
            let response = dispatch(&home, &request(op, args));
            assert_eq!(response.error.unwrap().code, "invalid_args");
        }
        assert!(management::load(&home).unwrap().jobs.is_empty());
    }

    #[test]
    fn rejects_arbitrary_commands_and_persists_idempotent_requests() {
        let temp = tempfile::tempdir().unwrap();
        let home = HomeLayout::from_path(temp.path()).unwrap();
        for runtime in ["custom", "web_model", "npm:untrusted", "../codex"] {
            assert!(!dispatch(&home, &request("cli_management_submit", json!({"by":"user","runtime":runtime,"operation":"install","request_id":"test"}))).ok);
        }
        assert!(!dispatch(&home, &request("cli_management_submit", json!({"by":"user","runtime":"codex","operation":"install","request_id":"test","command":"arbitrary"}))).ok);
        assert!(!management::root(&home).exists());
        let args =
            json!({"by":"user","runtime":"codex","operation":"install","request_id":"install-1"});
        let first = dispatch(&home, &request("cli_management_submit", args.clone()));
        assert!(first.ok);
        assert_eq!(
            first.result,
            dispatch(&home, &request("cli_management_submit", args)).result
        );
        assert_eq!(management::load(&home).unwrap().jobs.len(), 1);
        assert!(management::load(&home).unwrap().installations.is_empty());
    }

    #[test]
    fn schedule_edits_require_a_current_revision() {
        let temp = tempfile::tempdir().unwrap();
        let home = HomeLayout::from_path(temp.path()).unwrap();
        let args = json!({"by":"user","revision":0,"rules":[{"id":"daily","enabled":true,"trigger":{"kind":"cron","cron":"0 3 * * *","timezone":"UTC"}}]});
        assert!(
            dispatch(
                &home,
                &request("cli_management_schedules_update", args.clone())
            )
            .ok
        );
        assert_eq!(
            dispatch(&home, &request("cli_management_schedules_update", args))
                .error
                .unwrap()
                .code,
            "cli_schedule_revision_conflict"
        );
    }
}
