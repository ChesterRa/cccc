//! The per-group IM bridge log (`state/im_bridge.log`) that operators and diagnostics read.

use cccc_core::{GroupStore, HomeLayout};
use std::io::Write;

/// Appends one line, rotating to `im_bridge.log.1` beyond 1 MiB.
pub(super) fn append(home: &HomeLayout, group_id: &str, line: &str) -> std::io::Result<()> {
    let dir = GroupStore::new(home.clone())?.state_dir(group_id)?;
    cccc_core::fs::with_exclusive_lock(&dir.join("im_bridge.log.lock"), || {
        let path = dir.join("im_bridge.log");
        if path.exists() && path.metadata()?.len() + line.len() as u64 + 1 > 1024 * 1024 {
            let backup = dir.join("im_bridge.log.1");
            if backup.exists() {
                std::fs::remove_file(&backup)?;
            }
            std::fs::rename(&path, backup)?;
        }
        let mut options = std::fs::OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path)?;
        writeln!(file, "{line}")?;
        file.sync_data()
    })
}
