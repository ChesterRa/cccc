use cccc_core::HomeLayout;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct DaemonPaths {
    pub home: HomeLayout,
    pub daemon_dir: PathBuf,
    pub socket: PathBuf,
    pub address: PathBuf,
    pub pid: PathBuf,
    pub log: PathBuf,
    pub lock: PathBuf,
}

impl DaemonPaths {
    #[must_use]
    pub fn new(home: HomeLayout) -> Self {
        let daemon_dir = home.daemon_dir();
        Self {
            home,
            socket: daemon_dir.join("ccccd.sock"),
            address: daemon_dir.join("ccccd.addr.json"),
            pid: daemon_dir.join("ccccd.pid"),
            log: daemon_dir.join("ccccd.log"),
            lock: daemon_dir.join("ccccd.lock"),
            daemon_dir,
        }
    }
}
