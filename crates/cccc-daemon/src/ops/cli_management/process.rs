use cccc_core::{HomeLayout, cli_management};
use cccc_runtime::OwnedProcessTree;
use regex::Regex;
use serde_json::json;
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub(super) struct Log {
    file: Arc<Mutex<File>>,
    secrets: Arc<Vec<String>>,
    directory: PathBuf,
}

impl Log {
    pub fn open(home: &HomeLayout, job_id: &str) -> io::Result<Self> {
        cli_management::validate_id(job_id)?;
        let directory = cli_management::root(home).join("logs");
        std::fs::create_dir_all(&directory)?;
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        Ok(Self {
            file: Arc::new(Mutex::new(
                options.open(directory.join(format!("{job_id}.jsonl")))?,
            )),
            secrets: Arc::new(Self::environment_secrets()),
            directory: directory.join(job_id),
        })
    }

    fn environment_secrets() -> Vec<String> {
        std::env::vars()
            .filter(|(key, value)| {
                let key = key.to_ascii_uppercase();
                value.len() >= 4
                    && ["TOKEN", "SECRET", "PASSWORD", "API_KEY"]
                        .iter()
                        .any(|needle| key.contains(needle))
            })
            .map(|(_, value)| value)
            .collect()
    }

    fn output_file(&self, stream: &str) -> io::Result<(PathBuf, File)> {
        let mut directory = std::fs::DirBuilder::new();
        directory.recursive(true);
        let mut options = OpenOptions::new();
        options.create_new(true).append(true).read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
            directory.mode(0o700);
            options.mode(0o600);
        }
        directory.create(&self.directory)?;
        let filename = format!("{}.{stream}.log", uuid::Uuid::new_v4());
        let path = self.directory.join(&filename);
        let output = options.open(&path)?;
        let mut index = self
            .file
            .lock()
            .map_err(|_| io::Error::other("CLI 日志锁不可用"))?;
        serde_json::to_writer(
            &mut *index,
            &json!({
                "ts":chrono::Utc::now().to_rfc3339(), "stream":stream,
                "output_file":filename, "text":"原始输出文件；页面提供脱敏尾部视图",
            }),
        )?;
        index.write_all(b"\n")?;
        index.flush()?;
        Ok((path, output))
    }

    pub fn redact(&self, text: &str) -> String {
        Self::redact_text(text, &self.secrets)
    }

    fn redact_text(text: &str, secrets: &[String]) -> String {
        // 先还原终端显示文本，再匹配凭据；颜色码不能把敏感字段拆开绕过脱敏。
        static ANSI: OnceLock<Regex> = OnceLock::new();
        let mut text = ANSI
            .get_or_init(|| Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]").expect("valid ANSI pattern"))
            .replace_all(text, "")
            .into_owned();
        // 环境变量遍历顺序不稳定，先处理长值，避免短前缀替换后暴露长凭据尾部。
        let mut secrets: Vec<_> = secrets.iter().collect();
        secrets.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
        for secret in secrets {
            text = text.replace(secret, "[REDACTED]");
        }
        static PATTERNS: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
        for (pattern, replacement) in PATTERNS.get_or_init(|| vec![
            (Regex::new(r"(?i)(https?://)[^\s/@]+:[^\s/@]+@").expect("valid URL credential pattern"), "$1[REDACTED]@"),
            (Regex::new(r#"(?i)((?:api[_-]?key|access[_-]?token|token|password|secret)["']?\s*[:=]\s*["']?)[^\s"'&,;]+"#).expect("valid secret field pattern"), "$1[REDACTED]"),
            (Regex::new(r"(?i)(bearer\s+)[a-z0-9._~+/=-]+").expect("valid bearer pattern"), "$1[REDACTED]"),
        ]) {
            text = pattern.replace_all(&text, *replacement).into_owned();
        }
        text
    }

    pub fn write(&self, stream: &str, text: &str) -> io::Result<()> {
        // 先对完整文本脱敏，再按 UTF-8 边界分片；不能让凭据跨片绕过脱敏。
        // JSON 转义最坏会放大六倍，64 KiB 仍小于日志读取接口的 1 MiB 单记录上限。
        let text = self.redact(text);
        let timestamp = chrono::Utc::now().to_rfc3339();
        let mut file = self
            .file
            .lock()
            .map_err(|_| io::Error::other("CLI 日志锁不可用"))?;
        let mut start = 0;
        loop {
            let mut end = (start + 65_536).min(text.len());
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            let value = json!({
                "ts":timestamp,"stream":stream,"text":&text[start..end],
                "continuation":end < text.len(),
            });
            serde_json::to_writer(&mut *file, &value)?;
            file.write_all(b"\n")?;
            if end == text.len() {
                break;
            }
            start = end;
        }
        file.flush()
    }

    pub fn sync(&self) -> io::Result<()> {
        self.file
            .lock()
            .map_err(|_| io::Error::other("CLI 日志锁不可用"))?
            .sync_data()
    }
}

/// 不继承模型凭据及任意工具开关，仅保留软件安装所需的系统与网络环境。
pub(super) fn install_environment() -> BTreeMap<String, String> {
    const KEYS: &[&str] = &[
        "PATH",
        "HOME",
        "USER",
        "USERPROFILE",
        "SystemRoot",
        "SYSTEMROOT",
        "TEMP",
        "TMP",
        "TMPDIR",
        "LANG",
        "LC_ALL",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "NO_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "no_proxy",
        "SSL_CERT_FILE",
        "SSL_CERT_DIR",
        "CURL_CA_BUNDLE",
        "NODE_EXTRA_CA_CERTS",
    ];
    KEYS.iter()
        .filter_map(|key| std::env::var(key).ok().map(|value| ((*key).into(), value)))
        .collect()
}

pub(super) fn run(
    command: &mut Command,
    log: &Log,
    stop: &AtomicBool,
    timeout: Duration,
    capture: bool,
) -> io::Result<String> {
    if stop.load(Ordering::Acquire) {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "CLI 后台已停止"));
    }
    log.write(
        "command",
        &format!(
            "{:?} {:?}",
            command.get_program(),
            command.get_args().collect::<Vec<_>>()
        ),
    )?;
    // 与原生 Daemon/cloudflared 相同：子进程直接写文件，不按行收集输出。
    let (out_path, out) = log.output_file("stdout")?;
    let (_, err) = log.output_file("stderr")?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(out.try_clone()?))
        .stderr(Stdio::from(err.try_clone()?));
    let (mut child, owner) = OwnedProcessTree::spawn(command)?;
    let result = (|| -> io::Result<std::process::ExitStatus> {
        let deadline = Instant::now() + timeout;
        loop {
            if stop.load(Ordering::Acquire) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "CLI 后台已停止"));
            }
            if let Some(status) = owner.try_wait(|| child.try_wait())? {
                return Ok(status);
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("CLI 命令超过 {} 秒", timeout.as_secs()),
                ));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    })();
    // 保留原生进程树归属与有限回收；没有读取管道，也就不再等待管道 EOF。
    let cleanup = owner.terminate().and_then(|()| {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if child.try_wait()?.is_some() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "CLI 进程回收超过 5 秒，未确认进程已退出",
                ));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    });
    // 先记录命令原始结果；清理或文件同步失败也不能悄悄改变为成功。
    let recorded = log.write(
        "exit",
        &match &result {
            Ok(status) => status.to_string(),
            Err(error) => error.to_string(),
        },
    );
    if let Err(error) = cleanup {
        let message = format!("CLI 进程树收尾失败，未确认进程已停止：{error}");
        let _ = log.write("error", &message);
        let _ = log.sync();
        return Err(io::Error::new(error.kind(), message));
    }
    recorded?;
    out.sync_data()?;
    err.sync_data()?;
    log.sync()?;
    let status = result?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "CLI 命令失败：{status}；详情见操作日志"
        )));
    }
    if !capture {
        return Ok(String::new());
    }
    // 只有版本/路径查询才读回结果；上限约束结构化解析，不约束日志写入。
    let mut bytes = Vec::new();
    File::open(out_path)?
        .take(1_048_577)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 1_048_576 {
        return Err(io::Error::other(
            "CLI 结构化输出超过 1 MiB；完整原始输出仍保留在受保护的日志文件中",
        ));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

pub(super) fn read_log(home: &HomeLayout, id: &str, offset: u64) -> io::Result<serde_json::Value> {
    let mut page = cli_management::read_log(home, id, offset)?;
    for entry in page["entries"].as_array_mut().into_iter().flatten() {
        let Some(filename) = entry.get("output_file").and_then(|value| value.as_str()) else {
            continue;
        };
        let valid = [".stdout.log", ".stderr.log"].iter().any(|suffix| {
            filename
                .strip_suffix(suffix)
                .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok())
        });
        if !valid {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CLI 输出日志引用无效",
            ));
        }
        let directory = cli_management::root(home).join("logs").join(id);
        let path = directory.join(filename);
        // 不提供任意文件读取，也不跟随任务目录/文件链接或打开命名管道。
        if !std::fs::symlink_metadata(&directory)?.is_dir()
            || !std::fs::symlink_metadata(&path)?.is_file()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "CLI 输出日志必须是任务目录内的普通文件",
            ));
        }
        let lines = crate::ops::diagnostics::tail::read_last_lines(&path, 200)?;
        entry["text"] = json!(format!(
            "原始输出的脱敏尾部视图（最多 200 行 / 8 MiB，不代表完整文件）：\n{}",
            Log::redact_text(&lines.join("\n"), &Log::environment_secrets()),
        ));
        entry
            .as_object_mut()
            .expect("output log entry is an object")
            .remove("output_file");
    }
    Ok(page)
}

pub(super) fn command(
    program: &Path,
    args: &[String],
    cwd: &Path,
    env: &BTreeMap<String, String>,
) -> Command {
    let mut command = Command::new(program);
    command.args(args).current_dir(cwd).env_clear().envs(env);
    command
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn terminal_formatting_and_overlapping_credentials_cannot_bypass_redaction() {
        let temp = tempfile::tempdir().expect("terminal formatting and");
        let home = HomeLayout::from_path(temp.path()).expect("terminal formatting and");
        let mut log = Log::open(&home, "formatted-secrets").expect("terminal formatting and");
        // 不修改进程环境，也不使用任何真实凭据。
        log.secrets = Arc::new(vec!["sample-secret".into(), "sample-secret-longer".into()]);
        let cases = [
            ("to\x1b[31mken=private-value", "token=[REDACTED]"),
            ("Bearer\x1b[0m private-value", "Bearer [REDACTED]"),
            ("sample-\x1b[32msecret-longer", "[REDACTED]"),
            ("sample-secret-longer", "[REDACTED]"),
            (
                "https\x1b[0m://user:private-value@proxy.invalid/",
                "https://[REDACTED]@proxy.invalid/",
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(log.redact(input), expected, "{input:?}");
            log.write("stderr", input).expect("terminal formatting and");
        }
        log.sync().expect("terminal formatting and");
        let stored = std::fs::read_to_string(
            cli_management::root(&home).join("logs/formatted-secrets.jsonl"),
        )
        .expect("terminal formatting and");
        assert!(!stored.contains("private-value"));
        assert!(!stored.contains("longer"));
        assert!(!stored.contains("sample-"));
    }

    #[test]
    fn process_output_uses_private_files_without_waiting_for_newlines() {
        use std::io::{Seek, SeekFrom};
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().expect("process output uses");
        let home = HomeLayout::from_path(temp.path()).expect("process output uses");
        cli_management::submit(
            &home,
            "codex",
            cli_management::Operation::Install,
            "raw-output",
            chrono::Utc::now(),
        )
        .expect("process output uses");
        let log = Log::open(&home, "raw-output").expect("process output uses");
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "head -c 10485760 /dev/zero | tr '\\000' x; printf '\\nlast-out\\n'; printf 'token=private-value\\n' >&2"]);
        run(
            &mut cmd,
            &log,
            &AtomicBool::new(false),
            Duration::from_secs(10),
            false,
        )
        .expect("process output uses");
        assert_eq!(
            std::fs::metadata(&log.directory)
                .expect("process output uses")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        let raw = std::fs::read_dir(&log.directory)
            .expect("process output uses")
            .map(|entry| entry.expect("process output uses").path())
            .collect::<Vec<_>>();
        assert_eq!(raw.len(), 2);
        for path in &raw {
            assert_eq!(
                std::fs::metadata(path)
                    .expect("process output uses")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        let out = raw
            .iter()
            .find(|path| path.to_string_lossy().ends_with(".stdout.log"))
            .expect("process output uses");
        assert_eq!(
            std::fs::metadata(out).expect("process output uses").len(),
            10_485_760 + 10
        );
        let mut file = File::open(out).expect("process output uses");
        file.seek(SeekFrom::End(-10)).expect("process output uses");
        let mut tail = String::new();
        file.read_to_string(&mut tail).expect("process output uses");
        assert_eq!(tail, "\nlast-out\n");
        let mut offset = 0;
        let mut texts = String::new();
        loop {
            let page = read_log(&home, "raw-output", offset).expect("process output uses");
            assert!(
                page["entries"]
                    .as_array()
                    .expect("process output uses")
                    .iter()
                    .all(|entry| entry.get("output_file").is_none())
            );
            texts.push_str(&page.to_string());
            offset = page["next_offset"].as_u64().expect("process output uses");
            if !page["has_more"].as_bool().expect("process output uses") {
                break;
            }
        }
        assert!(texts.contains("last-out") && texts.contains("[REDACTED]"));
        assert!(!texts.contains("private-value"));
        assert!(texts.contains("exit status: 0"));
    }

    #[test]
    fn query_capture_is_separate_from_unlimited_process_logs() {
        let temp = tempfile::tempdir().expect("query capture is");
        let home = HomeLayout::from_path(temp.path()).expect("query capture is");
        let log = Log::open(&home, "query").expect("query capture is");
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "printf '1.2.3'; printf 'diagnostic' >&2"]);
        assert_eq!(
            run(
                &mut cmd,
                &log,
                &AtomicBool::new(false),
                Duration::from_secs(2),
                true
            )
            .expect("query capture is"),
            "1.2.3"
        );
        let mut oversized = Command::new("/bin/sh");
        oversized.args(["-c", "head -c 1048577 /dev/zero"]);
        let error = run(
            &mut oversized,
            &log,
            &AtomicBool::new(false),
            Duration::from_secs(2),
            true,
        )
        .expect_err("query capture is");
        assert!(error.to_string().contains("结构化输出超过"));
        assert!(
            std::fs::read_dir(&log.directory)
                .expect("query capture is")
                .any(|entry| entry
                    .expect("query capture is")
                    .metadata()
                    .expect("query capture is")
                    .len()
                    == 1_048_577)
        );
        let mut parent = Command::new("/bin/sh");
        parent.args(["-c", "sleep 30 & printf 'no-newline'"]);
        let started = Instant::now();
        assert_eq!(
            run(
                &mut parent,
                &log,
                &AtomicBool::new(false),
                Duration::from_secs(2),
                true
            )
            .expect("query capture is"),
            "no-newline"
        );
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn long_unicode_and_escaped_logs_remain_readable_and_redacted_across_pages() {
        let temp = tempfile::tempdir().expect("long unicode and");
        let home = HomeLayout::from_path(temp.path()).expect("long unicode and");
        cli_management::submit(
            &home,
            "codex",
            cli_management::Operation::Install,
            "long-log",
            chrono::Utc::now(),
        )
        .expect("long unicode and");
        let log = Log::open(&home, "long-log").expect("long unicode and");
        let content = format!(
            "{}token=must-not-leak {}",
            "中".repeat(21_843),
            "\0\\\"多行\n".repeat(150_000)
        );
        log.write("stdout", &content).expect("long unicode and");
        log.sync().expect("long unicode and");
        let expected = log.redact(&content);
        let mut result = String::new();
        let mut offset = 0;
        let mut pages = 0;
        loop {
            let page =
                cli_management::read_log(&home, "long-log", offset).expect("long unicode and");
            for entry in page["entries"].as_array().expect("long unicode and") {
                result.push_str(entry["text"].as_str().expect("long unicode and"));
            }
            pages += 1;
            let next = page["next_offset"].as_u64().expect("long unicode and");
            assert!(next > offset);
            offset = next;
            if !page["has_more"].as_bool().expect("long unicode and") {
                break;
            }
        }
        assert!(pages > 1);
        assert_eq!(result, expected);
        assert!(!result.contains("must-not-leak"));
        assert!(!result.contains('\u{fffd}'));
    }

    #[test]
    fn logs_both_streams_redacts_credentials_and_preserves_failure() {
        let temp = tempfile::tempdir().expect("logs both streams");
        let home = HomeLayout::from_path(temp.path()).expect("logs both streams");
        let log = Log::open(&home, "test").expect("logs both streams");
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "printf 'token=example-value\\n'; printf 'https://account:example-password@proxy.invalid/\\n' >&2; exit 7"]);
        assert!(
            run(
                &mut cmd,
                &log,
                &AtomicBool::new(false),
                Duration::from_secs(2),
                true
            )
            .is_err()
        );
        let contents = std::fs::read_to_string(cli_management::root(&home).join("logs/test.jsonl"))
            .expect("logs both streams");
        assert!(
            contents.contains("stdout")
                && contents.contains("stderr")
                && contents.contains("exit status: 7")
        );
        assert!(!contents.contains("example-value") && !contents.contains("example-password"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn log_storage_failure_prevents_starting_the_command() {
        let temp = tempfile::tempdir().expect("log storage failure");
        let home = HomeLayout::from_path(temp.path()).expect("log storage failure");
        let log = Log::open(&home, "storage-failure").expect("log storage failure");
        *log.file.lock().expect("log storage failure") = OpenOptions::new()
            .write(true)
            .open("/dev/full")
            .expect("log storage failure");
        let marker = temp.path().join("not-started");
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "touch \"$CLI_TEST_MARKER\""])
            .env("CLI_TEST_MARKER", &marker);
        let error = run(
            &mut cmd,
            &log,
            &AtomicBool::new(false),
            Duration::from_secs(2),
            false,
        )
        .expect_err("log storage failure");
        assert_eq!(error.raw_os_error(), Some(28));
        assert!(!marker.exists());
    }

    #[test]
    fn timeout_stops_descendants_without_harming_other_processes() {
        let temp = tempfile::tempdir().expect("timeout stops descendants");
        let home = HomeLayout::from_path(temp.path()).expect("timeout stops descendants");
        let log = Log::open(&home, "timeout").expect("timeout stops descendants");
        let marker = temp.path().join("should-not-exist");
        let mut unrelated = Command::new("sleep")
            .arg("5")
            .spawn()
            .expect("timeout stops descendants");
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "(sleep 1; touch \"$CLI_TEST_MARKER\") & wait"])
            .env("CLI_TEST_MARKER", &marker);
        let result = run(
            &mut cmd,
            &log,
            &AtomicBool::new(false),
            Duration::from_millis(80),
            false,
        );
        assert_eq!(
            result.expect_err("timeout stops descendants").kind(),
            io::ErrorKind::TimedOut
        );
        assert!(
            unrelated
                .try_wait()
                .expect("timeout stops descendants")
                .is_none()
        );
        unrelated.kill().expect("timeout stops descendants");
        unrelated.wait().expect("timeout stops descendants");
        std::thread::sleep(Duration::from_millis(1100));
        assert!(!marker.exists());
    }
}
