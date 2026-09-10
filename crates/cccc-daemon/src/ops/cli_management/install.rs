use super::process::{self, Log};
use super::{Source, source};
use cccc_contracts::ActorRuntime;
use cccc_core::{HomeLayout, cli_management as management, runtime_mcp};
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

pub(super) fn install(
    home: &HomeLayout,
    job: &management::Job,
    log: &Log,
    stop: &AtomicBool,
) -> io::Result<management::Installation> {
    let runtime =
        runtime_mcp::from_name(&job.runtime).ok_or_else(|| io::Error::other("未知 Runtime"))?;
    let (tool, node) = match source(runtime) {
        Source::Mise { tool, node } => (tool, node),
        Source::Deepseek => ("deepseek", true),
        Source::Official { .. } if runtime == ActorRuntime::Devin => ("http:cccc-devin", false),
        Source::Official { .. } if runtime == ActorRuntime::Kiro => ("http:cccc-kiro", false),
        Source::Official { .. } if runtime == ActorRuntime::Droid => ("http:cccc-droid", false),
        Source::Official { .. } if runtime == ActorRuntime::Hermes => {
            ("pipx:NousResearch/hermes-agent", true)
        }
        _ => {
            return Err(io::Error::other(
                "此发行来源的隔离安装适配仍在开发中，未修改原有安装",
            ));
        }
    };
    let env = process::install_environment();
    let mise = runtime_mcp::find_program("mise", env.get("PATH").map(std::ffi::OsStr::new))
        .ok_or_else(|| {
            io::Error::other("未找到 mise；请在 CCCC 所在环境安装 mise 并重新启动 CCCC")
        })?;
    install_mise(home, job, runtime, tool, node, &mise, env, log, stop)
}

pub(super) fn concrete_version(value: &str) -> io::Result<String> {
    let value = value.trim();
    if value.is_empty()
        || !value.as_bytes()[0].is_ascii_alphanumeric()
        || value.len() > 128
        || value == "latest"
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._+-".contains(&c))
    {
        return Err(io::Error::other("mise 未返回有效的固定版本号"));
    }
    Ok(value.to_owned())
}

#[allow(clippy::too_many_arguments)]
fn install_mise(
    home: &HomeLayout,
    job: &management::Job,
    runtime: ActorRuntime,
    tool: &str,
    node: bool,
    mise: &Path,
    mut env: BTreeMap<String, String>,
    log: &Log,
    stop: &AtomicBool,
) -> io::Result<management::Installation> {
    // 每次操作独立安装目录；即使同版本重装失败，也不覆盖被旧 Actor 使用的文件。
    let directory = management::root(home).join("versions").join(&job.id);
    std::fs::create_dir_all(&directory)?;
    let directory = directory.canonicalize()?;
    for (key, relative) in [
        ("MISE_DATA_DIR", "data"),
        ("MISE_CACHE_DIR", "cache"),
        ("MISE_CONFIG_DIR", "config"),
        ("MISE_STATE_DIR", "state"),
        ("MISE_SYSTEM_CONFIG_DIR", "system"),
        ("MISE_GLOBAL_CONFIG_FILE", "config/config.toml"),
        ("npm_config_cache", "npm-cache"),
    ] {
        env.insert(
            key.into(),
            directory.join(relative).to_string_lossy().into_owned(),
        );
    }
    env.insert(
        "MISE_CEILING_PATHS".into(),
        directory.to_string_lossy().into_owned(),
    );
    env.insert("MISE_YES".into(), "1".into());
    env.insert("MISE_COLOR".into(), "0".into());
    env.insert("MISE_JOBS".into(), "1".into());
    env.insert("MISE_HTTP_TIMEOUT".into(), "60s".into());
    env.insert("MISE_FETCH_REMOTE_VERSIONS_CACHE".into(), "0s".into());
    env.insert("CI".into(), "1".into());
    env.insert(
        "CCCC_HOME".into(),
        home.root().to_string_lossy().into_owned(),
    );
    let release = if tool.starts_with("http:") {
        let release = super::distribution::resolve(runtime, log, stop)?;
        std::fs::create_dir_all(directory.join("config"))?;
        std::fs::write(directory.join("config/config.toml"), release.config(tool)?)?;
        Some(release)
    } else {
        None
    };
    let execute = |args: Vec<String>, env: &BTreeMap<String, String>, capture| {
        process::run(
            &mut process::command(mise, &args, &directory, env),
            log,
            stop,
            Duration::from_secs(900),
            capture,
        )
    };
    let deepseek = runtime == ActorRuntime::Deepseek;
    let hermes = runtime == ActorRuntime::Hermes;
    let version = if deepseek {
        cccc_contracts::DEEPSEEK_RELEASE_VERSION.to_owned()
    } else if let Some(release) = &release {
        release.version.clone()
    } else {
        let mut version_env = env.clone();
        if runtime == ActorRuntime::Amp {
            // Amp 正常发行包含时间戳与 Git 后缀，被 mise 当作 SemVer 预发布过滤。
            // 只调整 Amp 的版本查询；其他 CLI 和依赖仍只解析稳定版本。
            version_env.insert("MISE_PRERELEASES".into(), "1".into());
        }
        concrete_version(&execute(
            vec!["latest".into(), tool.into()],
            &version_env,
            true,
        )?)?
    };
    let selected = management::load(home)?
        .installations
        .get(&job.runtime)
        .cloned();
    if let Some(selected) = selected.filter(|selected| selected.version == version) {
        log.write("stage", "已是目标版本，验证现有选中版本")?;
        let mut verify_env = env.clone();
        let checked =
            management::apply_environment(home, &job.runtime, &mut verify_env).and_then(|_| {
                verify(
                    runtime,
                    &selected.executable,
                    &version,
                    &directory,
                    &verify_env,
                    log,
                    stop,
                )
            });
        match checked {
            Ok(()) => return Ok(selected),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => return Err(error),
            Err(error) => {
                log.write(
                    "stage",
                    &format!("现有安装验证失败：{error}；在新目录重新安装，不覆盖原文件"),
                )?;
            }
        }
    }
    let mut tools = if deepseek || hermes {
        Vec::new()
    } else {
        vec![format!("{tool}@{version}")]
    };
    let mut dependencies = if node { vec!["node@22"] } else { Vec::new() };
    if hermes {
        dependencies.extend(["python@3.11", "uv", "ripgrep", "ffmpeg"]);
    }
    for dependency in dependencies {
        let dependency_version = concrete_version(&execute(
            vec!["latest".into(), dependency.into()],
            &env,
            true,
        )?)?;
        let dependency_tool = format!(
            "{}@{dependency_version}",
            dependency.split('@').next().unwrap_or(dependency)
        );
        execute(vec!["install".into(), dependency_tool.clone()], &env, false)?;
        let dependency_env = execute(
            vec!["env".into(), "--json".into(), dependency_tool.clone()],
            &env,
            true,
        )?;
        let bins = managed_paths(&dependency_env, &directory)?;
        prepend_paths(&mut env, &bins)?;
        tools.push(dependency_tool);
    }
    let bin_paths = if deepseek {
        let dsh_home = directory.join("deepseek");
        std::fs::create_dir(&dsh_home)?;
        let cccc_executable = crate::ops::codex_mcp::resolve_cccc_executable()
            .ok_or_else(|| io::Error::other("找不到用于 DeepSeek 配置的 CCCC 可执行文件"))?;
        crate::deepseek_setup::prepare_isolated(
            home,
            dsh_home,
            &mut env,
            &cccc_executable,
            |root, env| {
                cccc_core::fs::write_json(
                    &root.join("package.json"),
                    &cccc_runtime::canonical_deepseek_runtime_manifest(),
                )
                .map_err(|error| error.to_string())?;
                let npm = runtime_mcp::find_program(
                    if cfg!(windows) { "npm.cmd" } else { "npm" },
                    env.get("PATH").map(std::ffi::OsStr::new),
                )
                .ok_or("DeepSeek 安装需要 npm")?;
                process::run(
                    &mut process::command(&npm, &crate::deepseek_setup::install_args(), root, env),
                    log,
                    stop,
                    Duration::from_secs(900),
                    false,
                )
                .map(|_| ())
                .map_err(|error| error.to_string())
            },
        )
        .map_err(io::Error::other)?;
        managed_paths(&serde_json::to_string(&env)?, &directory)?
    } else if hermes {
        let bin = super::hermes::install(&directory, &version, &env, log, stop)?;
        prepend_paths(&mut env, &[bin])?;
        managed_paths(&serde_json::to_string(&env)?, &directory)?
    } else {
        let mut args = vec!["install".into()];
        if release.is_none() {
            args.extend(tools.iter().cloned());
        }
        execute(args, &env, false)?;
        let mut args = vec!["env".into(), "--json".into()];
        if release.is_none() {
            args.extend(tools);
        }
        managed_paths(&execute(args, &env, true)?, &directory)?
    };
    let path = std::env::join_paths(&bin_paths).map_err(io::Error::other)?;
    let name = cccc_runtime::default_command(runtime)
        .into_iter()
        .next()
        .ok_or_else(|| io::Error::other("Runtime 没有 CLI 命令"))?;
    let executable = runtime_mcp::find_program_in(&name, Some(&path), &directory)
        .ok_or_else(|| io::Error::other(format!("安装完成但找不到可执行文件 {name}")))?;
    if !executable.canonicalize()?.starts_with(&directory) {
        return Err(io::Error::other("CLI 可执行文件不在本次隔离安装目录中"));
    }
    prepend_paths(&mut env, &bin_paths)?;
    verify(runtime, &executable, &version, &directory, &env, log, stop)?;
    Ok(management::Installation {
        version,
        executable,
        bin_paths,
        installed_at: chrono::Utc::now().to_rfc3339(),
    })
}

fn managed_paths(output: &str, directory: &Path) -> io::Result<Vec<PathBuf>> {
    let values: serde_json::Value = serde_json::from_str(output)?;
    let path = values
        .get("PATH")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| io::Error::other("mise 环境结果缺少 PATH"))?;
    let mut paths = Vec::new();
    for path in std::env::split_paths(path) {
        if path.starts_with(directory)
            && path.is_dir()
            && path.canonicalize()?.starts_with(directory)
            && !paths.contains(&path)
        {
            paths.push(path);
        }
    }
    if paths.is_empty() {
        return Err(io::Error::other("mise 未提供本次安装的可执行目录"));
    }
    Ok(paths)
}

fn prepend_paths(env: &mut BTreeMap<String, String>, paths: &[PathBuf]) -> io::Result<()> {
    let mut combined = paths.to_vec();
    if let Some(path) = env.get("PATH") {
        combined.extend(std::env::split_paths(path).filter(|path| !paths.contains(path)));
    }
    env.insert(
        "PATH".into(),
        std::env::join_paths(combined)
            .map_err(io::Error::other)?
            .to_string_lossy()
            .into_owned(),
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn verify(
    runtime: ActorRuntime,
    executable: &Path,
    expected: &str,
    cwd: &Path,
    env: &BTreeMap<String, String>,
    log: &Log,
    stop: &AtomicBool,
) -> io::Result<()> {
    log.write(
        "stage",
        "验证 CLI 可执行文件；此检查不代表已登录或模型调用可用",
    )?;
    if runtime == ActorRuntime::Deepseek {
        log.write(
            "stage",
            "检查 CCCC 固定 DeepSeek 组件、锁文件、Node 和 ACP 配置，不启动模型",
        )?;
        return cccc_runtime::deepseek_preflight(&[executable.to_string_lossy().into_owned()], env)
            .map_err(io::Error::other);
    }
    let mut probe_env = env.clone();
    if runtime == ActorRuntime::Hermes {
        // Hermes 版本入口可能初始化自身数据，不允许探针触碰用户的真实配置。
        probe_env.insert(
            "HERMES_HOME".into(),
            cwd.join("version-probe").to_string_lossy().into_owned(),
        );
    }
    let version = process::run(
        &mut process::command(executable, &["--version".into()], cwd, &probe_env),
        log,
        stop,
        Duration::from_secs(30),
        true,
    )?;
    if !matches_version(&version, expected)? {
        return Err(io::Error::other(
            "CLI 返回的版本与安装目标不一致，未切换选中版本",
        ));
    }
    Ok(())
}

fn matches_version(output: &str, expected: &str) -> io::Result<bool> {
    let expected = regex::escape(expected.trim_start_matches('v'));
    // Copilot 的版本句子以句点结尾；不把版本数字后的任意点都当作边界。
    let pattern = regex::Regex::new(&format!(
        r"(?:^|[^0-9A-Za-z._+-])v?{expected}(?:$|[^0-9A-Za-z._+-]|\.(?:$|\s))"
    ))
    .map_err(io::Error::other)?;
    Ok(pattern.is_match(output))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_probe_accepts_sentence_punctuation_not_different_versions() {
        assert!(
            matches_version(
                "GitHub Copilot CLI 1.0.83.\nRun 'copilot update'.",
                "1.0.83"
            )
            .unwrap()
        );
        assert!(matches_version("Hermes Agent v0.21.1 (2026.9.7)", "v2026.9.7").unwrap());
        for wrong in [
            "11.0.83",
            "1.0.830",
            "1.0.83.1",
            "1.0.83-beta",
            "1.0.83+build",
        ] {
            assert!(!matches_version(wrong, "1.0.83").unwrap());
        }
    }

    /// 仅在显式指定的无凭据测试机环境运行，保留安装和日志供检查。
    #[test]
    #[ignore = "需要测试机、真实 mise 与官方下载网络；不在本地或普通 CI 安装 CLI"]
    fn real_install_and_update_in_explicit_lab() {
        let root = std::env::var("CLI_MANAGEMENT_TEST_ROOT").expect("必须指定新的隔离测试目录");
        assert!(!Path::new(&root).exists(), "拒绝使用已有数据目录");
        let runtime = std::env::var("CLI_MANAGEMENT_TEST_RUNTIME").expect("必须指定已知 Runtime");
        assert!(runtime_mcp::from_name(&runtime).is_some());
        let home = HomeLayout::from_path(&root).unwrap();
        home.initialize().unwrap();
        for (id, operation) in [
            ("install", management::Operation::Install),
            ("update", management::Operation::Update),
        ] {
            let now = chrono::Utc::now();
            let job = management::submit(&home, &runtime, operation, id, now).unwrap();
            management::claim_next(&home, now).unwrap();
            let log = Log::open(&home, id).unwrap();
            let result = install(&home, &job, &log, &AtomicBool::new(false));
            let summary = match &result {
                Ok(selected) => format!(
                    "{} {:?}：{}，{}",
                    runtime,
                    operation,
                    selected.version,
                    selected.executable.display()
                ),
                Err(error) => log.redact(&error.to_string()),
            };
            log.write("验收", &summary).unwrap();
            log.sync().unwrap();
            let success = result.is_ok();
            management::finish(
                &home,
                id,
                result.map_err(|error| log.redact(&error.to_string())),
                chrono::Utc::now(),
            )
            .unwrap();
            assert!(
                success,
                "{summary}；完整日志：{root}/cli-management/logs/{id}.jsonl"
            );
            println!("{summary}");
        }
    }

    #[test]
    fn version_and_path_results_cannot_inject_options_or_select_external_binaries() {
        for value in ["", "latest", "../1", "--force", "1\n2", "1;command"] {
            assert!(concrete_version(value).is_err(), "{value}");
        }
        assert_eq!(concrete_version("0.153.2\n").unwrap(), "0.153.2");
        let temp = tempfile::tempdir().unwrap();
        assert!(managed_paths(r#"{"PATH":"/usr/bin"}"#, temp.path()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn isolated_install_and_failed_update_keep_the_previous_executable() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let home = HomeLayout::from_path(temp.path().join("home")).unwrap();
        home.initialize().unwrap();
        let mise = temp.path().join("mise");
        let binary = temp.path().join("fake-codex");
        std::fs::write(&binary, "#!/bin/sh\nprintf 'codex-cli %s\\n' \"$CLI_TEST_VERSION\"\n[ \"$CLI_TEST_FAIL\" != 1 ]\n").unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(
            &mise,
            r#"#!/bin/sh
set -eu
case "$1" in
  latest) printf '%s\n' "$CLI_TEST_VERSION" ;;
  install)
    mkdir -p "$MISE_DATA_DIR/bin"
    cp "$CLI_TEST_BINARY" "$MISE_DATA_DIR/bin/codex"
    ;;
  env) printf '{"PATH":"%s/bin"}\n' "$MISE_DATA_DIR" ;;
  *) exit 9 ;;
esac
"#,
        )
        .unwrap();
        std::fs::set_permissions(&mise, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut env = process::install_environment();
        env.insert(
            "CLI_TEST_BINARY".into(),
            binary.to_string_lossy().into_owned(),
        );
        env.insert("CLI_TEST_VERSION".into(), "1.2.3".into());
        let now = chrono::Utc::now();
        let job = management::submit(&home, "codex", management::Operation::Install, "first", now)
            .unwrap();
        management::claim_next(&home, now).unwrap();
        let log = Log::open(&home, &job.id).unwrap();
        let mut result = install_mise(
            &home,
            &job,
            ActorRuntime::Codex,
            "codex",
            false,
            &mise,
            env.clone(),
            &log,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(
            result
                .executable
                .starts_with(management::root(&home).join("versions/first"))
        );
        let dependency = management::root(&home).join("versions/first/dependency");
        std::fs::create_dir(&dependency).unwrap();
        result.bin_paths.push(dependency.clone());
        management::finish(&home, &job.id, Ok(result.clone()), now).unwrap();
        let previous_bytes = std::fs::read(&result.executable).unwrap();
        env.insert("CLI_TEST_VERSION".into(), "2.0.0".into());
        env.insert("CLI_TEST_FAIL".into(), "1".into());
        let job = management::submit(&home, "codex", management::Operation::Update, "second", now)
            .unwrap();
        management::claim_next(&home, now).unwrap();
        let error = install_mise(
            &home,
            &job,
            ActorRuntime::Codex,
            "codex",
            false,
            &mise,
            env.clone(),
            &Log::open(&home, &job.id).unwrap(),
            &AtomicBool::new(false),
        )
        .unwrap_err();
        management::finish(&home, &job.id, Err(error.to_string()), now).unwrap();
        assert_eq!(
            management::load(&home).unwrap().installations["codex"],
            result
        );
        assert_eq!(std::fs::read(&result.executable).unwrap(), previous_bytes);

        env.insert("CLI_TEST_VERSION".into(), "1.2.3".into());
        env.remove("CLI_TEST_FAIL");
        for scenario in [
            "healthy",
            "missing-dependency",
            "missing-executable",
            "failed-probe",
            "failed-repair",
            "stopped",
        ] {
            let previous = management::load(&home).unwrap().installations["codex"].clone();
            let preserved = if scenario == "missing-executable" {
                let moved = previous.executable.with_extension("preserved");
                std::fs::rename(&previous.executable, &moved).unwrap();
                moved
            } else {
                previous.executable.clone()
            };
            if scenario == "missing-dependency" {
                std::fs::rename(&dependency, dependency.with_extension("preserved")).unwrap();
            }
            if matches!(scenario, "failed-probe" | "failed-repair") {
                std::fs::write(&preserved, "#!/bin/sh\nprintf 'wrong version\\n'\n").unwrap();
            }
            if scenario == "failed-repair" {
                env.insert("CLI_TEST_FAIL".into(), "1".into());
            }
            let preserved_bytes = std::fs::read(&preserved).unwrap();
            let job =
                management::submit(&home, "codex", management::Operation::Update, scenario, now)
                    .unwrap();
            management::claim_next(&home, now).unwrap();
            let outcome = install_mise(
                &home,
                &job,
                ActorRuntime::Codex,
                "codex",
                false,
                &mise,
                env.clone(),
                &Log::open(&home, &job.id).unwrap(),
                &AtomicBool::new(scenario == "stopped"),
            );
            match scenario {
                "healthy" => assert_eq!(outcome.as_ref().unwrap(), &previous),
                "failed-repair" => assert!(outcome.is_err()),
                "stopped" => assert_eq!(
                    outcome.as_ref().unwrap_err().kind(),
                    io::ErrorKind::Interrupted
                ),
                _ => {
                    let installed = outcome.as_ref().unwrap();
                    assert_eq!(installed.version, previous.version);
                    assert_ne!(installed.executable, previous.executable);
                    assert!(
                        installed
                            .executable
                            .starts_with(management::root(&home).join("versions").join(scenario))
                    );
                    assert_eq!(
                        std::fs::read(&installed.executable).unwrap(),
                        previous_bytes
                    );
                }
            }
            // 安装返回时仍未改变选中版本，旧文件也不被原地修复或移除。
            assert_eq!(
                management::load(&home).unwrap().installations["codex"],
                previous
            );
            assert_eq!(std::fs::read(&preserved).unwrap(), preserved_bytes);
            management::finish(
                &home,
                &job.id,
                outcome.map_err(|error| error.to_string()),
                now,
            )
            .unwrap();
            if matches!(scenario, "failed-repair" | "stopped") {
                assert_eq!(
                    management::load(&home).unwrap().installations["codex"],
                    previous
                );
            }
            if matches!(scenario, "healthy" | "stopped") {
                assert!(
                    !management::root(&home)
                        .join("versions")
                        .join(scenario)
                        .join("data")
                        .exists()
                );
            }
        }
    }
}
