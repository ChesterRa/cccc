//! The maintenance fixtures normally run an in-process daemon. An explicit
//! two-build run replaces only that daemon, leaving the HTTP router and its
//! authority checks intact without starting provider callback workers.
use cccc_client::DaemonClient;
use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::task::JoinHandle;

const PREFIX: &str = "CCCC_TEST_REMOTE_";
const MODE: &str = "CCCC_TEST_REMOTE_MODE";
const OLD: &str = "CCCC_TEST_REMOTE_OLD_BIN";
const NEW: &str = "CCCC_TEST_REMOTE_NEW_BIN";

struct Builds {
    old: PathBuf,
    new: PathBuf,
    mode: &'static str,
    switch: usize,
}

impl Builds {
    fn parse(options: BTreeMap<String, OsString>) -> Option<Self> {
        if options.is_empty() {
            return None;
        }
        assert!(
            options
                .keys()
                .all(|key| [MODE, OLD, NEW].contains(&key.as_str())),
            "unknown two-build remote test option"
        );
        let (mode, switch) = match options.get(MODE).and_then(|mode| mode.to_str()) {
            Some("wake") => ("wake", 1),
            Some("claim") => ("claim", 2),
            _ => panic!("two-build remote test requires MODE=wake or MODE=claim"),
        };
        let executable = |key| {
            let path = PathBuf::from(options.get(key).expect("both daemon paths are required"));
            let path = path
                .canonicalize()
                .expect("daemon test executable must exist");
            let metadata = path.metadata().expect("daemon executable metadata");
            assert!(metadata.is_file(), "daemon test executable must be a file");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_ne!(
                    metadata.permissions().mode() & 0o111,
                    0,
                    "daemon must be executable"
                );
            }
            path
        };
        let old = executable(OLD);
        let new = executable(NEW);
        assert_ne!(old, new, "two-build test requires separate executables");
        Some(Self {
            old,
            new,
            mode,
            switch,
        })
    }
}

#[derive(Default)]
pub(super) struct TestDaemons {
    builds: Option<Builds>,
    launches: usize,
    spawned_pid: Option<u32>,
    old_identity: Option<(u32, String, String)>,
    switched: bool,
}

impl TestDaemons {
    pub(super) fn new() -> Self {
        let options = std::env::vars_os()
            .filter_map(|(key, value)| {
                key.to_str()
                    .filter(|key| key.starts_with(PREFIX))
                    .map(|key| (key.to_owned(), value))
            })
            .collect();
        Self {
            builds: Builds::parse(options),
            ..Self::default()
        }
    }

    pub(super) fn spawn(&mut self, home: HomeLayout) -> JoinHandle<()> {
        let launch = self.launches;
        self.launches += 1;
        let Some(builds) = &self.builds else {
            return tokio::spawn(async move {
                cccc_daemon::run(home).await.expect("fixture daemon");
            });
        };
        assert!(
            cfg!(target_os = "linux"),
            "two-build maintenance fixture requires Linux"
        );
        let executable = if launch < builds.switch {
            &builds.old
        } else {
            &builds.new
        };
        let user_home = home.root().join("external-daemon-home");
        std::fs::create_dir_all(&user_home).expect("private daemon user home");
        let mut child = tokio::process::Command::new(executable)
            .args(["daemon", "run"])
            .env_clear()
            .env("HOME", user_home)
            .env(
                "PATH",
                std::env::var_os("PATH").expect("isolated test PATH"),
            )
            .env("CCCC_HOME", home.root())
            .env("CCCC_LAUNCHER_PATH", executable)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .expect("start selected external daemon");
        self.spawned_pid = Some(child.id().expect("external daemon PID"));
        tokio::spawn(async move {
            assert!(child.wait().await.expect("external daemon exit").success());
        })
    }

    pub(super) async fn started(&mut self, client: &DaemonClient) {
        let ping = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Ok(response) = client
                    .call(&DaemonRequest {
                        v: 1,
                        op: "ping".into(),
                        args: Default::default(),
                    })
                    .await
                    && response.ok
                {
                    break response.result;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("fixture daemon ready");
        let Some(builds) = &self.builds else {
            return;
        };
        let pid = u32::try_from(ping["pid"].as_u64().expect("observed daemon PID"))
            .expect("native daemon PID");
        assert_eq!(
            Some(pid),
            self.spawned_pid,
            "ping must come from the selected child"
        );
        let source = ping["build"]["source_id"]
            .as_str()
            .expect("daemon source identity");
        assert!(source.len() == 64 && source.bytes().all(|c| c.is_ascii_hexdigit()));
        let version = ping["version"].as_str().expect("daemon version");
        assert!(
            !version.is_empty()
                && version.len() <= 80
                && version
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b".+-_".contains(&c))
        );
        if self.launches <= builds.switch {
            if let Some((_, _, old_source)) = &self.old_identity {
                assert_eq!(
                    source, old_source,
                    "old-side restart must use the old source"
                );
            }
            self.old_identity = Some((pid, version.to_owned(), source.to_owned()));
        } else {
            let (old_pid, old_version, old_source) =
                self.old_identity.as_ref().expect("old daemon ping");
            assert_ne!(
                source, old_source,
                "two-build test requires different source identities"
            );
            assert_ne!(
                pid, *old_pid,
                "two-build test requires different daemon processes"
            );
            if !self.switched {
                eprintln!(
                    "CCCC_REMOTE_TWO_BUILDS mode={} old_pid={old_pid} old_version={old_version} old_source={old_source} new_pid={pid} new_version={version} new_source={source}",
                    builds.mode
                );
            }
            self.switched = true;
        }
    }

    #[cfg(target_os = "linux")]
    pub(super) fn runs_in_test_process(&self) -> bool {
        // Direct session-owner library calls use the caller's controller
        // registry. An external daemon is verified through IPC and receipts.
        self.builds.is_none()
    }

    #[cfg(target_os = "linux")]
    pub(super) fn assert_switched(&self) {
        assert!(
            self.builds.is_none() || self.switched,
            "two-build transition was not observed"
        );
    }

    #[cfg(target_os = "linux")]
    pub(super) fn expects_remote_receipt(&self) -> bool {
        // The selected old build predates the remote activity receipt. This is
        // an explicit old-side expectation, never a fallback on missing data.
        self.builds
            .as_ref()
            .is_none_or(|builds| self.launches > builds.switch)
    }
}

#[test]
fn explicit_two_build_options_cannot_fall_back_to_the_in_process_fixture() {
    assert!(Builds::parse(BTreeMap::new()).is_none());
    for options in [
        BTreeMap::from([(MODE.into(), "wake".into())]),
        BTreeMap::from([("CCCC_TEST_REMOTE_MDOE".into(), "wake".into())]),
        BTreeMap::from([(MODE.into(), "cliam".into())]),
        BTreeMap::from([
            (OLD.into(), "missing".into()),
            (NEW.into(), "missing".into()),
        ]),
        BTreeMap::from([
            (MODE.into(), "wake".into()),
            (OLD.into(), "missing".into()),
            (NEW.into(), "missing".into()),
        ]),
    ] {
        assert!(std::panic::catch_unwind(|| Builds::parse(options)).is_err());
    }
}
