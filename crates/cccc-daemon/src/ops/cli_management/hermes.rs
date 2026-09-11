//! Hermes 不发布普通 wheel；保留完整源码与资源，采用官方 editable 安装。
use super::process::{self, Log};
use cccc_core::runtime_mcp;
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

const REPOSITORY: &str = "https://github.com/NousResearch/hermes-agent.git";

pub(super) fn install(
    directory: &Path,
    version: &str,
    environment: &BTreeMap<String, String>,
    log: &Log,
    stop: &AtomicBool,
) -> io::Result<PathBuf> {
    // mise 的 GitHub pipx 后端只用于发行标签查询，不执行 wheel 安装。
    let tag = release_tag(version)?;
    let mut env = environment.clone();
    env.insert("GIT_TERMINAL_PROMPT".into(), "0".into());
    env.insert("GIT_CONFIG_NOSYSTEM".into(), "1".into());
    env.insert(
        "GIT_CONFIG_GLOBAL".into(),
        directory
            .join("config/gitconfig")
            .to_string_lossy()
            .into_owned(),
    );
    let find = |name: &str| {
        runtime_mcp::find_program(name, env.get("PATH").map(std::ffi::OsStr::new))
            .ok_or_else(|| io::Error::other(format!("Hermes 安装需要 {name}")))
    };
    let git = find("git")?;
    let uv = find("uv")?;
    let python = find("python")?;
    let npm = find(if cfg!(windows) { "npm.cmd" } else { "npm" })?;
    let source = directory.join("hermes-agent");
    let args = [
        "clone",
        "--depth",
        "1",
        "--single-branch",
        "--branch",
        &tag,
        "--",
        REPOSITORY,
    ]
    .into_iter()
    .map(str::to_owned)
    .chain([source.to_string_lossy().into_owned()])
    .collect::<Vec<_>>();
    process::run(
        &mut process::command(&git, &args, directory, &env),
        log,
        stop,
        Duration::from_secs(900),
        false,
    )?;
    let commit = process::run(
        &mut process::command(&git, &["rev-parse".into(), "HEAD".into()], &source, &env),
        log,
        stop,
        Duration::from_secs(30),
        true,
    )?;
    if commit.trim().len() != 40 || !commit.trim().bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(io::Error::other("Hermes 源码提交标识无效"));
    }
    log.write(
        "stage",
        &format!("Hermes {tag}，源码提交 {}；使用官方锁文件", commit.trim()),
    )?;
    if !source.join("uv.lock").is_file() || !source.join("package-lock.json").is_file() {
        return Err(io::Error::other(
            "Hermes 发行缺少锁文件，拒绝自动改用未锁定安装",
        ));
    }
    let config = directory.join("config/uv");
    std::fs::create_dir_all(&config)?;
    let venv = source.join("venv");
    for (key, value) in [
        ("UV_PROJECT_ENVIRONMENT", venv.clone()),
        ("UV_PYTHON", python),
        ("UV_CACHE_DIR", directory.join("uv-cache")),
        ("XDG_CONFIG_HOME", config.clone()),
        ("XDG_CONFIG_DIRS", config),
    ] {
        env.insert(key.into(), value.to_string_lossy().into_owned());
    }
    env.insert("UV_PYTHON_DOWNLOADS".into(), "never".into());
    // 保留该源码的 tool.uv 配置，不继承系统配置；不设置 HERMES_NIX_BUILD。
    process::run(
        &mut process::command(
            &uv,
            &[
                "sync".into(),
                "--extra".into(),
                "all".into(),
                "--locked".into(),
                "--no-dev".into(),
            ],
            &source,
            &env,
        ),
        log,
        stop,
        Duration::from_secs(900),
        false,
    )?;
    let npm_args = [
        "ci",
        "--workspace",
        "ui-tui",
        "--workspace",
        "web",
        "--include-workspace-root",
        "--no-audit",
        "--no-fund",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    process::run(
        &mut process::command(&npm, &npm_args, &source, &env),
        log,
        stop,
        Duration::from_secs(900),
        false,
    )?;
    process::run(
        &mut process::command(
            &npm,
            &[
                "run".into(),
                "build".into(),
                "--workspace".into(),
                "ui-tui".into(),
            ],
            &source,
            &env,
        ),
        log,
        stop,
        Duration::from_secs(900),
        false,
    )?;
    log.write(
        "stage",
        "Hermes 源码、Python 依赖与 TUI 已安装；未安装浏览器/桌面组件，也未登录模型",
    )?;
    Ok(venv.join(if cfg!(windows) { "Scripts" } else { "bin" }))
}

fn release_tag(version: &str) -> io::Result<String> {
    let version = version.strip_prefix('v').unwrap_or(version);
    let parts: Vec<_> = version.split('.').collect();
    if parts.len() != 3
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(io::Error::other("Hermes 发行标签不是已知的版本格式"));
    }
    Ok(format!("v{version}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hermes_release_uses_explicit_official_tag_not_an_option_or_branch() {
        assert_eq!(
            release_tag("v2026.9.7").expect("hermes release uses"),
            "v2026.9.7"
        );
        assert_eq!(
            release_tag("2026.9.7").expect("hermes release uses"),
            "v2026.9.7"
        );
        for invalid in ["main", "latest", "--branch", "2026.9.7\n", "1.2.3/../../"] {
            assert!(release_tag(invalid).is_err());
        }
    }
}
