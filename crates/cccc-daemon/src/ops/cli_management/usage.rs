//! 复用原生 fs2 文件锁：启动先持共享锁，卸载只尝试独占锁，不终止使用者。
use cccc_contracts::ActorRuntime;
use cccc_core::{HomeLayout, cli_management as management};
use fs2::FileExt;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

type Key = (PathBuf, String, String);
type Uses = HashMap<Key, Vec<File>>;

fn actors() -> &'static Mutex<Uses> {
    static ACTORS: OnceLock<Mutex<Uses>> = OnceLock::new();
    ACTORS.get_or_init(Default::default)
}

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

pub(crate) fn acquire(
    home: &HomeLayout,
    runtime: ActorRuntime,
    command: &[String],
) -> io::Result<Vec<File>> {
    let name = serde_json::to_value(runtime)?
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let state = management::load(home)?;
    let mut names = vec![name];
    // 显式引用另一 Runtime 的受管目录，也必须保护实际使用的安装。
    for job in state
        .jobs
        .values()
        .filter(|job| job.operation != management::Operation::Uninstall)
    {
        management::validate_id(&job.id)?;
        let directory = management::root(home).join("versions").join(&job.id);
        if command
            .iter()
            .any(|part| std::path::Path::new(part).starts_with(&directory))
        {
            names.push(job.runtime.clone());
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

pub(crate) fn retain(home: &HomeLayout, group: &str, actor: &str, files: Vec<File>) {
    // 启动已取得共享文件锁，即使登记锁中毒也必须保留这些句柄，不能放开使用保护。
    actors()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(
            (home.root().to_path_buf(), group.into(), actor.into()),
            files,
        );
}

pub(super) fn exclusive(home: &HomeLayout, runtime: &str) -> io::Result<File> {
    // 卸载是破坏性操作；登记锁中毒时无法确信归属完整，保守拒绝，不清除中毒状态。
    // 使用启动时留下的归属，不依赖可能已被修改的 Actor 配置。
    actors()
        .lock()
        .map_err(|_| io::Error::other("CLI usage registry poisoned"))?
        .retain(|(root, group, actor), _| {
            if root != home.root() {
                return true;
            }
            let pty = match cccc_runtime::status(group, actor) {
                Ok(status) => status.running,
                Err(cccc_runtime::RuntimeError::NotFound(_, _)) => false,
                Err(_) => true,
            };
            pty || crate::ops::local_headless::running(group, actor)
                || crate::ops::deepseek_runtime::running(group, actor)
        });
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
