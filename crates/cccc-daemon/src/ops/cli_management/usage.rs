//! 复用原生 fs2 文件锁：启动先持共享锁，卸载只尝试独占锁，不终止使用者。
use cccc_contracts::ActorRuntime;
use cccc_core::{HomeLayout, cli_management as management};
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::io;

fn lock_file(home: &HomeLayout, runtime: &str) -> io::Result<File> {
    management::validate_id(runtime)?;
    let directory = management::root(home).join("usage");
    std::fs::create_dir_all(&directory)?;
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.join(format!("{runtime}.lock")))
}

#[cfg(test)]
pub(crate) fn acquire(
    home: &HomeLayout,
    runtime: ActorRuntime,
    command: &[String],
) -> io::Result<Vec<File>> {
    acquire_in(home, runtime, command, home.root())
}

pub(crate) fn acquire_in(
    home: &HomeLayout,
    runtime: ActorRuntime,
    command: &[String],
    cwd: &std::path::Path,
) -> io::Result<Vec<File>> {
    let name = serde_json::to_value(runtime)?
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let mut names = vec![name];
    let versions = management::root(home).join("versions");
    let canonical_versions = versions.canonicalize().ok();
    // 显式引用另一 Runtime 的受管目录，也必须保护实际使用的安装。
    for part in command {
        let path = cwd.join(part);
        let resolved = match path.canonicalize() {
            Ok(path) => path,
            Err(error) if path.starts_with(&versions) => return Err(error),
            Err(_) => continue,
        };
        if let Ok(relative) =
            resolved.strip_prefix(canonical_versions.as_deref().unwrap_or(&versions))
        {
            if let Some(id) = relative
                .components()
                .next()
                .and_then(|part| part.as_os_str().to_str())
            {
                if let Some(job) = management::find_job(home, id)? {
                    if job.operation != management::Operation::Uninstall {
                        names.push(job.runtime);
                    }
                } else {
                    return Err(io::Error::other("无法确认受管命令的安装归属"));
                }
            }
        }
    }
    names.sort();
    names.dedup();
    names
        .into_iter()
        .map(|name| {
            let file = lock_file(home, &name)?;
            FileExt::try_lock_shared(&file).map_err(|error| {
                if error.kind() == io::ErrorKind::WouldBlock {
                    io::Error::new(error.kind(), "CLI 正在卸载，请等待任务完成后再启动")
                } else {
                    error
                }
            })?;
            Ok(file)
        })
        .collect()
}

pub(super) fn exclusive(home: &HomeLayout, runtime: &str) -> io::Result<File> {
    // 锁由本次进程/会话持有，不通过 Actor ID 推断旧进程是否已退出。
    let file = lock_file(home, runtime)?;
    FileExt::try_lock_exclusive(&file).map_err(|error| {
        if error.kind() == io::ErrorKind::WouldBlock {
            io::Error::new(
                error.kind(),
                "CLI 仍被 Actor 或内建助手使用；请先停止相关使用者，再重新卸载",
            )
        } else {
            error
        }
    })?;
    Ok(file)
}
