//! Attended update for the Web host (`cccc` and `cccc web`).
//!
//! The Web confirms a version and a mode. This host prepares that update while
//! the Web keeps serving: it fetches the target release's installer, has it
//! download, verify and stage the release, and checks the staged program. Once
//! the Web hands the update over, the host stops its daemon, has the installer
//! replace the files and relays to the installed program in this same process
//! with `cccc update --resume`. The new program starts the daemon and the Web
//! where they ran before and confirms the update once both are back. Whether
//! the agents came back is not observed here.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use cccc_core::HomeLayout;
use cccc_core::attended_update::{self, BuildIdentity, Source};
use cccc_core::local_sessions::NativeHost;
use cccc_core::update_capabilities::{
    KeepDecision, KeepRefusal, UpdateCandidate, UpdateCapabilities,
};
use cccc_core::update_image::FileIdentity;
use cccc_daemon::session_owner::{AgentIdentity, AgentObservations, ShutdownMode, ShutdownReport};
use cccc_web::{
    UpdateAgent, UpdateAgents, UpdateMode, UpdatePreparation, UpdateRejection, UpdateRequest,
    UpdateSource,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{oneshot, watch};

use crate::update_checker::LaunchProgram;

/// The relay record this build writes before exec and reads after it. A
/// candidate lists the schemas its `--resume` reads in its self-check.
pub(crate) const RELAY_SCHEMA: u32 = 2;
const SELF_CHECK_SCHEMA: u32 = 1;
const TRANSACTION_SCHEMA: u32 = 2;
const INSTALLER_MAX_BYTES: usize = 1024 * 1024;
/// The installer's own line naming the phase contract it implements.
const INSTALLER_PHASE_API: &str = "# CCCC_INSTALL_PHASE_API=1";
const SELF_CHECK_MAX_BYTES: usize = 64 * 1024;
const PHASE_OUTPUT_MAX_BYTES: usize = 256 * 1024;
const PROGRAM: &str = if cfg!(windows) { "cccc.exe" } else { "cccc" };
/// How long a host that came back looks for the agents its update stopped or
/// kept before it records what it found.
const AGENTS_OBSERVE_BUDGET: Duration = Duration::from_secs(10);

/// Set while this host looks for the agents of the result it just recorded.
/// The same host serves that result, so one still marked as observing while
/// nothing looks was cut short.
static OBSERVING_AGENTS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether this host is still looking for the agents of its latest result.
pub(crate) fn observing_agents() -> bool {
    OBSERVING_AGENTS.load(std::sync::atomic::Ordering::Acquire)
}

/// Marks this host as looking for agents until it is dropped.
struct Observing;

impl Observing {
    fn start() -> Self {
        OBSERVING_AGENTS.store(true, std::sync::atomic::Ordering::Release);
        Self
    }
}

impl Drop for Observing {
    fn drop(&mut self) {
        OBSERVING_AGENTS.store(false, std::sync::atomic::Ordering::Release);
    }
}
const PREPARE_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const PHASE_TIMEOUT: Duration = Duration::from_secs(120);
const SELF_CHECK_TIMEOUT: Duration = Duration::from_secs(20);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(60);

/// What `update --self-check` prints: constants compiled into the program,
/// read without touching any CCCC home, daemon or provider.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct SelfCheck {
    pub schema: u32,
    pub version: String,
    pub build: SelfCheckBuild,
    pub daemon_compatibility: String,
    pub relay_schemas: Vec<u32>,
    #[serde(default)]
    pub capabilities: UpdateCapabilities,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct SelfCheckBuild {
    pub source_id: String,
}

pub(crate) fn print_self_check() -> Result<()> {
    let check = SelfCheck {
        schema: SELF_CHECK_SCHEMA,
        version: crate::PRODUCT_VERSION.into(),
        build: SelfCheckBuild {
            source_id: cccc_core::build_info::source_id()
                .unwrap_or_default()
                .into(),
        },
        daemon_compatibility: cccc_contracts::RUST_DAEMON_COMPATIBILITY.into(),
        relay_schemas: vec![RELAY_SCHEMA],
        capabilities: this_build(),
    };
    println!("{}", serde_json::to_string(&check)?);
    Ok(())
}

/// What this build can finish when an update hands over to it.
fn this_build() -> UpdateCapabilities {
    // On Linux and macOS this build reads its running image, so it finishes a
    // restart into it as a rebuilt program. It takes over the agents a stop
    // kept for it, once the daemon's own check allowed that stop, only where
    // session owners run; elsewhere agents do not outlive the daemon.
    UpdateCapabilities {
        local_restart: cfg!(any(target_os = "linux", target_os = "macos")),
        preserve_agents: cccc_daemon::session_owner::platform_supported(),
        intent_schemas: vec![attended_update::INTENT_SCHEMA],
        owner_contracts: cccc_daemon::session_owner::ownership_contract()
            .into_iter()
            .collect(),
        mcp_registration_apis: vec![cccc_core::owned_mcp::REGISTRATION_API],
        mcp_daemon_contracts: cccc_contracts::MCP_DAEMON_ACCEPTED_CONTRACTS.to_vec(),
    }
}

/// Where the host served, replayed by the program it relays to: the address
/// it actually listened on, not the options it was started with.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct LaunchSpec {
    pub host: Option<String>,
    pub port: Option<u16>,
    /// `normal` or `exhibit` when the launch chose one explicitly.
    pub web_mode: Option<String>,
    pub terminal: bool,
}

impl LaunchSpec {
    pub(crate) fn new(
        host: Option<String>,
        port: Option<u16>,
        web_mode: Option<cccc_web::WebMode>,
        terminal: bool,
    ) -> Self {
        Self {
            host,
            port,
            web_mode: web_mode.map(|mode| match mode {
                cccc_web::WebMode::Normal => "normal".into(),
                cccc_web::WebMode::Exhibit => "exhibit".into(),
            }),
            terminal,
        }
    }

    pub(crate) fn web_mode(&self) -> Result<Option<cccc_web::WebMode>> {
        match self.web_mode.as_deref() {
            None => Ok(None),
            Some("normal") => Ok(Some(cccc_web::WebMode::Normal)),
            Some("exhibit") => Ok(Some(cccc_web::WebMode::Exhibit)),
            Some(other) => bail!("unsupported Web mode in the update record: {other}"),
        }
    }
}

/// Written before exec by the program being replaced, read by `--resume`.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RelayRecord {
    pub schema: u32,
    pub transaction: String,
    pub from_version: String,
    pub to_version: String,
    pub source_id: String,
    pub mode: String,
    pub source: Source,
    pub launch: LaunchSpec,
    pub outcome: RelayOutcome,
    /// The receipt of the stop this update asked for, checked before the
    /// relay: the agents the program started next looks for. The daemon it
    /// starts clears the receipt file, so the receipt travels here.
    pub stopped: Option<ShutdownReport>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum RelayOutcome {
    /// The new files are in place; the new program finishes the update.
    Applied,
    /// The checked Local image may start; nothing was installed or restored.
    LocalReady,
    /// Nothing was replaced, or the previous files were restored.
    Abandoned { stage: String, reason: String },
}

/// The latest attended update's observed result for this home.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct LastResult {
    pub transaction: String,
    pub from_version: Option<String>,
    pub to_version: String,
    pub mode: Option<String>,
    pub result: String,
    pub stage: Option<String>,
    pub reason: Option<String>,
    pub agents: UpdateAgents,
    pub finished_at: String,
    /// `release` or `local`.
    pub source: Option<String>,
    /// For a restart into a rebuilt program, the file it restarted into, as the
    /// check that offered it named it.
    pub token: Option<String>,
    /// The build it moved to, as its self-check named it.
    pub source_id: Option<String>,
}

fn agents_not_observed() -> UpdateAgents {
    UpdateAgents {
        state: "not_observed".into(),
        ..UpdateAgents::default()
    }
}

/// How a result names what it moved to: `release` or `local`, and for a
/// rebuilt program, the file its check offered.
fn result_source(source: &Source) -> (Option<String>, Option<String>) {
    match source {
        Source::Release { .. } => (Some("release".into()), None),
        Source::Local { file_identity, .. } => (Some("local".into()), file_identity.token().ok()),
    }
}

/// Written when a transaction is created, so an update that never reserved
/// this home's startup can still be cleaned up in its own installation, and
/// never behind the back of the CCCC that is still preparing it.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TransactionRecord {
    schema: u32,
    source: Source,
    creator: NativeHost,
}

/// One update's private directory under `<home>/update`.
#[derive(Clone, Debug)]
pub(crate) struct Transaction {
    pub id: String,
    pub dir: PathBuf,
}

impl Transaction {
    fn create(home: &HomeLayout, id: &str, source: Source) -> io::Result<Self> {
        source.validate()?;
        let root = home.root().join("update");
        create_private_dir_all(&root)?;
        let dir = root.join(id);
        create_private_dir(&dir)?;
        // A directory without its record could never be cleaned up: keep both
        // or neither.
        let created = (|| {
            let transaction = Self {
                id: id.into(),
                dir: std::fs::canonicalize(&dir)?,
            };
            let record = TransactionRecord {
                schema: TRANSACTION_SCHEMA,
                source,
                creator: NativeHost::capture(std::process::id())?,
            };
            cccc_core::fs::write_secret_json(&transaction.record_path(), &record)?;
            Ok(transaction)
        })();
        if created.is_err() {
            let _ = std::fs::remove_dir_all(&dir);
        }
        created
    }

    /// A transaction this home created: `<home>/update/<uuid>`, a real directory.
    pub(crate) fn open(home: &HomeLayout, dir: &Path) -> Result<Self> {
        let root = std::fs::canonicalize(home.root().join("update"))
            .context("this CCCC home has no update transactions")?;
        let dir = std::fs::canonicalize(dir)
            .with_context(|| format!("update transaction not found: {}", dir.display()))?;
        let id = dir
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| uuid::Uuid::parse_str(name).is_ok_and(|id| !id.is_nil()))
            .ok_or_else(|| anyhow!("not an update transaction: {}", dir.display()))?
            .to_owned();
        if dir.parent() != Some(root.as_path()) {
            bail!(
                "update transaction {} does not belong to CCCC_HOME={}",
                dir.display(),
                home.root().display()
            );
        }
        Ok(Self { id, dir })
    }

    fn script(&self) -> PathBuf {
        self.dir.join("install.sh")
    }

    fn record_path(&self) -> PathBuf {
        self.dir.join("transaction.json")
    }

    fn relay_path(&self) -> PathBuf {
        self.dir.join("relay.json")
    }

    /// Where and by whom this transaction was created.
    fn record(&self) -> Result<TransactionRecord> {
        let record: TransactionRecord = cccc_core::fs::read_json(&self.record_path())
            .with_context(|| format!("could not read {}", self.record_path().display()))?;
        if record.schema != TRANSACTION_SCHEMA {
            bail!(
                "update record {} is not usable",
                self.record_path().display()
            );
        }
        record.source.validate()?;
        Ok(record)
    }

    pub(crate) fn read_relay(&self) -> Result<RelayRecord> {
        let record: RelayRecord = cccc_core::fs::read_json(&self.relay_path())
            .with_context(|| format!("could not read {}", self.relay_path().display()))?;
        if record.schema != RELAY_SCHEMA || record.transaction != self.id {
            bail!(
                "update record {} does not match this program (schema {})",
                self.relay_path().display(),
                record.schema
            );
        }
        record.source.validate()?;
        BuildIdentity {
            version: record.to_version.clone(),
            source_id: record.source_id.clone(),
        }
        .validate()?;
        if record.source != self.record()?.source
            || !matches!(record.mode.as_str(), "keep" | "restart")
            || !matches!(
                (&record.source, &record.outcome),
                (
                    Source::Release { .. },
                    RelayOutcome::Applied | RelayOutcome::Abandoned { .. }
                ) | (Source::Local { .. }, RelayOutcome::LocalReady)
            )
        {
            bail!("update relay source or outcome does not match its transaction");
        }
        if let Some(stopped) = &record.stopped {
            let asked = match record.mode.as_str() {
                "keep" => ShutdownMode::PreserveAgents,
                _ => ShutdownMode::Stop,
            };
            if stopped.v != 1
                || stopped.mode != asked
                || matches!(record.outcome, RelayOutcome::Abandoned { .. })
            {
                bail!("update relay stop receipt does not match its mode or outcome");
            }
        }
        Ok(record)
    }

    fn remove(&self) {
        if let Err(error) = std::fs::remove_dir_all(&self.dir)
            && error.kind() != io::ErrorKind::NotFound
        {
            cccc_runtime::diagnostic(format_args!(
                "could not remove update transaction {}: {error}",
                self.dir.display()
            ));
        }
    }
}

#[cfg(unix)]
fn create_private_dir(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().mode(0o700).create(path)
}

#[cfg(not(unix))]
fn create_private_dir(path: &Path) -> io::Result<()> {
    std::fs::create_dir(path)
}

fn create_private_dir_all(path: &Path) -> io::Result<()> {
    match create_private_dir(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists && path.is_dir() => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            create_private_dir(path)
        }
        Err(error) => Err(error),
    }
}

/// What an update replaces and replays.
#[derive(Clone, Debug)]
pub(crate) struct UpdateContext {
    pub home: HomeLayout,
    /// The standalone installation a release replaces in place; none when
    /// another installer or a build put this program where it is.
    pub install_dir: Option<PathBuf>,
    /// The program recovery commands name: the installed one, or the one this
    /// host was started from.
    pub executable: PathBuf,
    pub launch: LaunchSpec,
}

impl UpdateContext {
    /// Any installation can restart into a rebuilt program at its own path;
    /// only a standalone one can also replace its files with a release. The
    /// launch to replay is filled in when the Web hands an update over.
    pub(crate) fn for_running_program(home: &HomeLayout) -> Result<Self> {
        let executable =
            std::env::current_exe().context("could not resolve the CCCC executable")?;
        Ok(
            match crate::commands::update::standalone_install_dir(&executable) {
                Ok(install_dir) => Self::for_installation(home, install_dir),
                Err(_) => Self {
                    home: home.clone(),
                    install_dir: None,
                    executable,
                    launch: LaunchSpec::default(),
                },
            },
        )
    }

    fn for_installation(home: &HomeLayout, install_dir: PathBuf) -> Self {
        Self {
            home: home.clone(),
            executable: install_dir.join(PROGRAM),
            install_dir: Some(install_dir),
            launch: LaunchSpec::default(),
        }
    }

    /// The standalone installation a release replaces.
    fn release_dir(&self) -> Result<&Path> {
        self.install_dir
            .as_deref()
            .context("this CCCC was not installed by the CCCC installer")
    }
}

/// A prepared update: a staged and checked release holding the install lock,
/// or a checked rebuilt program at the path the host was started from.
#[derive(Debug)]
pub(crate) struct Prepared {
    pub transaction: Transaction,
    pub version: String,
    pub mode: UpdateMode,
    pub target: BuildIdentity,
    /// What the same self-check that named `target` declared: a final stop
    /// that keeps agents is checked against these, never against another
    /// check's.
    pub capabilities: UpdateCapabilities,
    pub source: Source,
}

impl Prepared {
    /// The program the relay starts.
    pub(crate) fn program(&self) -> PathBuf {
        match &self.source {
            Source::Release { install_dir } => install_dir.join(PROGRAM),
            Source::Local { program, .. } => program.clone(),
        }
    }

    pub(crate) fn is_local(&self) -> bool {
        matches!(self.source, Source::Local { .. })
    }

    /// What a stop that keeps agents is checked against: this build, with what
    /// its own self-check declared.
    pub(crate) fn candidate(&self) -> UpdateCandidate {
        UpdateCandidate {
            build: self.target.clone(),
            capabilities: self.capabilities.clone(),
        }
    }

    /// What this is, for the lines that report it.
    pub(crate) fn what(&self) -> String {
        if self.is_local() {
            format!("restart into the local build {}", self.version)
        } else {
            format!("update to CCCC {}", self.version)
        }
    }

    /// What did not happen when this did not start.
    pub(crate) fn unchanged(&self) -> &'static str {
        if self.is_local() {
            "CCCC was not restarted"
        } else {
            "CCCC was not updated"
        }
    }

    /// What the terminal interface says while this replaces the process.
    pub(crate) fn notice(&self) -> String {
        // What is being attempted: whether agents were kept is known only once
        // CCCC is back.
        let agents = match self.mode {
            UpdateMode::Keep => "trying to keep its agents running",
            UpdateMode::Restart => "restarting its agents",
        };
        if self.is_local() {
            format!(
                "Restarting CCCC into the local build {}: {agents}, then CCCC starts again here.",
                self.version
            )
        } else {
            format!(
                "Updating CCCC to {}: {agents}, then CCCC starts again here.",
                self.version
            )
        }
    }
}

enum Slot {
    Idle,
    /// A preparation or its cleanup is running. `abandoned` asks it to undo.
    Busy {
        id: String,
        abandoned: bool,
    },
    Prepared(Box<Prepared>),
}

/// The host's one attended update, shared with the Web as its update hook.
#[derive(Clone)]
pub(crate) struct Coordinator {
    inner: Arc<Inner>,
}

struct Inner {
    context: UpdateContext,
    /// The program this host was started from, as it was then, on a host that
    /// can restart into a rebuilt one.
    launch: Option<LaunchProgram>,
    slot: Mutex<Slot>,
    /// Bumped whenever a preparation or cleanup settles.
    settled: watch::Sender<u64>,
    /// Wakes the running preparation once it is abandoned.
    cancel: tokio::sync::Notify,
}

impl Coordinator {
    pub(crate) fn new(context: UpdateContext, launch: Option<LaunchProgram>) -> Self {
        Self {
            inner: Arc::new(Inner {
                context,
                launch,
                slot: Mutex::new(Slot::Idle),
                settled: watch::channel(0).0,
                cancel: tokio::sync::Notify::new(),
            }),
        }
    }

    pub(crate) fn context(&self) -> &UpdateContext {
        &self.inner.context
    }

    pub(crate) fn hook(&self) -> cccc_web::UpdateHook {
        Arc::new(self.clone())
    }

    /// The update the Web handed over, if it is prepared.
    pub(crate) fn take_prepared(&self) -> Option<Prepared> {
        let mut slot = self.inner.lock();
        match std::mem::replace(&mut *slot, Slot::Idle) {
            Slot::Prepared(prepared) => {
                *slot = Slot::Busy {
                    id: prepared.transaction.id.clone(),
                    abandoned: false,
                };
                Some(*prepared)
            }
            other => {
                *slot = other;
                None
            }
        }
    }

    /// Undo whatever update is reserved and wait until nothing is left: the
    /// Web closed without handing it over, or the host is stopping.
    pub(crate) async fn close(&self) {
        let mut settled = self.inner.settled.subscribe();
        loop {
            let prepared = {
                let mut slot = self.inner.lock();
                match &mut *slot {
                    Slot::Idle => return,
                    Slot::Busy { abandoned, .. } => {
                        *abandoned = true;
                        None
                    }
                    Slot::Prepared(prepared) => {
                        let id = prepared.transaction.id.clone();
                        match std::mem::replace(
                            &mut *slot,
                            Slot::Busy {
                                id,
                                abandoned: true,
                            },
                        ) {
                            Slot::Prepared(prepared) => Some(*prepared),
                            _ => unreachable!("the slot was just matched as prepared"),
                        }
                    }
                }
            };
            match prepared {
                Some(prepared) => {
                    self.undo(prepared).await;
                    return;
                }
                None => {
                    // A preparation stops at once; a cleanup runs to its end.
                    self.inner.cancel.notify_waiters();
                    if settled.changed().await.is_err() {
                        return;
                    }
                }
            }
        }
    }

    /// Undo a prepared update that will not be applied, then open the slot
    /// again. A cleanup that cannot be confirmed keeps its files, says how to
    /// finish it, and those files hold off the next update until then.
    pub(crate) async fn undo(&self, prepared: Prepared) {
        let context = &self.inner.context;
        if let Err(error) = undo(context, &prepared.transaction).await {
            cccc_runtime::diagnostic(format_args!(
                "[cccc] {}",
                cleanup_failure(context, &prepared.transaction, &error)
            ));
        }
        self.inner.release(&prepared.transaction.id);
    }

    /// Give up an update whose intent could not be withdrawn: its record stays
    /// for the recovery command, and no update starts while the intent stands.
    pub(crate) fn keep_for_recovery(&self, prepared: Prepared) {
        self.inner.release(&prepared.transaction.id);
    }
}

impl Inner {
    fn lock(&self) -> std::sync::MutexGuard<'_, Slot> {
        self.slot.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn settle(&self) {
        self.settled
            .send_modify(|count| *count = count.wrapping_add(1));
    }

    /// Whether the preparation of update `id` is still wanted.
    fn wants(&self, id: &str) -> bool {
        matches!(&*self.lock(), Slot::Busy { id: current, abandoned: false } if current == id)
    }

    /// Open the slot again if update `id` still holds it, and wake whoever
    /// waits for it to settle.
    fn release(&self, id: &str) {
        {
            let mut slot = self.lock();
            if matches!(&*slot, Slot::Busy { id: current, .. } if current == id) {
                *slot = Slot::Idle;
            }
        }
        self.settle();
    }
}

impl cccc_web::UpdateHost for Coordinator {
    fn begin(&self, request: UpdateRequest) -> Result<UpdatePreparation, UpdateRejection> {
        let requested = match request.source {
            UpdateSource::Release { version } => {
                if self.inner.context.install_dir.is_none() {
                    return Err(rejection(
                        "update_unavailable",
                        "this CCCC was not installed by the CCCC installer; update it the way it was installed",
                    ));
                }
                Requested::Release {
                    version: release_version(&version)?,
                }
            }
            UpdateSource::Local { token, source_id } => {
                let Some(launch) = self.inner.launch.clone() else {
                    return Err(rejection(
                        "local_not_supported",
                        "this CCCC cannot restart into a rebuilt program",
                    ));
                };
                Requested::Local {
                    launch,
                    token,
                    source_id,
                }
            }
        };
        if request.mode == UpdateMode::Keep
            && matches!(requested, Requested::Release { .. })
            && !self.release_keep()
        {
            return Err(rejection(
                "keep_not_supported",
                "this CCCC cannot keep agents running through an update; choose restart",
            ));
        }
        refuse_active_intent(&self.inner.context.home)?;
        let id = uuid::Uuid::new_v4().simple().to_string();
        {
            let mut slot = self.inner.lock();
            if !matches!(*slot, Slot::Idle) {
                return Err(rejection(
                    "update_in_progress",
                    "another update of this CCCC is already in progress",
                ));
            }
            refuse_unfinished_cleanup(&self.inner.context)?;
            *slot = Slot::Busy {
                id: id.clone(),
                abandoned: false,
            };
        }
        let (sender, receiver) = oneshot::channel();
        let inner = Arc::clone(&self.inner);
        let transaction = id.clone();
        let mode = request.mode;
        tokio::spawn(async move {
            let result = prepare_unless_abandoned(&inner, &id, &requested, mode).await;
            let outcome = settle_preparation(&inner, &id, result).await;
            let _ = sender.send(outcome);
        });
        Ok(UpdatePreparation {
            transaction,
            ready: Box::pin(async move {
                receiver.await.unwrap_or_else(|_| {
                    Err(rejection(
                        "update_failed",
                        "the update preparation ended unexpectedly; nothing was changed",
                    ))
                })
            }),
        })
    }

    fn release_in_place(&self) -> bool {
        self.inner.context.install_dir.is_some()
    }

    fn local_restart(&self) -> bool {
        self.inner.launch.is_some()
    }

    /// A host that replaces its installation, running a build that takes
    /// agents over, offers to keep them; the release is checked once staged.
    fn release_keep(&self) -> bool {
        self.release_in_place() && this_build().preserve_agents
    }

    fn abandon(&self, transaction: &str) {
        let mut preparing = false;
        let prepared = {
            let mut slot = self.inner.lock();
            match &mut *slot {
                Slot::Busy { id, abandoned } if id == transaction => {
                    *abandoned = true;
                    preparing = true;
                    None
                }
                Slot::Prepared(prepared) if prepared.transaction.id == transaction => {
                    let id = prepared.transaction.id.clone();
                    match std::mem::replace(
                        &mut *slot,
                        Slot::Busy {
                            id,
                            abandoned: true,
                        },
                    ) {
                        Slot::Prepared(prepared) => Some(*prepared),
                        _ => unreachable!("the slot was just matched as prepared"),
                    }
                }
                _ => None,
            }
        };
        if preparing {
            self.inner.cancel.notify_waiters();
        }
        if let Some(prepared) = prepared {
            let coordinator = self.clone();
            tokio::spawn(async move { coordinator.undo(prepared).await });
        }
    }
}

/// What a start asked for, once the host accepted that it can do it.
enum Requested {
    Release {
        version: String,
    },
    Local {
        launch: LaunchProgram,
        token: String,
        source_id: String,
    },
}

/// Prepare until done or abandoned. An abandoned preparation is dropped at
/// once, which stops its download and kills its installer's process group;
/// what it left behind is then undone like any failed preparation.
async fn prepare_unless_abandoned(
    inner: &Inner,
    id: &str,
    requested: &Requested,
    mode: UpdateMode,
) -> Result<Prepared, PrepareFailure> {
    match requested {
        Requested::Release { version } => {
            unless_abandoned(inner, id, prepare(&inner.context, id, version, mode)).await
        }
        Requested::Local {
            launch,
            token,
            source_id,
        } => {
            let preparation = prepare_local(&inner.context, launch, id, token, source_id, mode);
            unless_abandoned(inner, id, preparation).await
        }
    }
}

async fn unless_abandoned(
    inner: &Inner,
    id: &str,
    preparation: impl std::future::Future<Output = Result<Prepared, PrepareFailure>>,
) -> Result<Prepared, PrepareFailure> {
    {
        tokio::pin!(preparation);
        loop {
            let cancelled = inner.cancel.notified();
            tokio::pin!(cancelled);
            // Listen before looking, so an abandon in between is not missed.
            cancelled.as_mut().enable();
            if !inner.wants(id) {
                break;
            }
            tokio::select! {
                result = &mut preparation => return result,
                () = &mut cancelled => {}
            }
        }
    }
    let dir = inner.context.home.root().join("update").join(id);
    let transaction = if dir.is_dir() {
        Transaction::open(&inner.context.home, &dir).ok()
    } else {
        None
    };
    Err((
        transaction,
        rejection(
            "update_abandoned",
            "the update was cancelled before it started; the installed CCCC was not changed",
        ),
    ))
}

/// Record the preparation's result in the slot. An abandoned or failed
/// preparation is undone before the slot opens again.
async fn settle_preparation(
    inner: &Inner,
    id: &str,
    result: Result<Prepared, (Option<Transaction>, UpdateRejection)>,
) -> Result<(), UpdateRejection> {
    let outcome = match result {
        Ok(prepared) => {
            // Check and publish under one lock, so an abandon cannot slip in
            // between and be lost.
            let unpublished = {
                let mut slot = inner.lock();
                if matches!(&*slot, Slot::Busy { id: current, abandoned: false } if current == id) {
                    *slot = Slot::Prepared(Box::new(prepared));
                    None
                } else {
                    Some(prepared)
                }
            };
            let Some(prepared) = unpublished else {
                inner.settle();
                return Ok(());
            };
            Err(with_cleanup(
                &inner.context,
                &prepared.transaction,
                rejection(
                    "update_abandoned",
                    "the update was cancelled before it started; the installed CCCC was not changed",
                ),
            )
            .await)
        }
        Err((Some(transaction), rejection)) => {
            Err(with_cleanup(&inner.context, &transaction, rejection).await)
        }
        Err((None, rejection)) => Err(rejection),
    };
    inner.release(id);
    outcome
}

/// Undo a preparation that will not be used. When its cleanup cannot be
/// confirmed, the rejection says so and how to finish it.
async fn with_cleanup(
    context: &UpdateContext,
    transaction: &Transaction,
    mut rejection: UpdateRejection,
) -> UpdateRejection {
    if let Err(error) = undo(context, transaction).await {
        rejection.message = format!(
            "{}; {}",
            rejection.message,
            cleanup_failure(context, transaction, &error)
        );
    }
    rejection
}

fn cleanup_failure(
    context: &UpdateContext,
    transaction: &Transaction,
    error: &anyhow::Error,
) -> String {
    format!(
        "the cleanup of update {} could not be confirmed ({error:#}); its files are kept in {}; stop CCCC, then finish it with `{}`",
        transaction.id,
        transaction.dir.display(),
        cleanup_command(context, transaction)
    )
}

fn cleanup_command(context: &UpdateContext, transaction: &Transaction) -> String {
    let quote = |path: &Path| shell_words::quote(&path.to_string_lossy()).into_owned();
    format!(
        "{} update --recover {}",
        quote(&context.executable),
        quote(&transaction.dir)
    )
}

/// A transaction directory outlives its update only when the cleanup was not
/// confirmed or the host stopped halfway, possibly still holding the install
/// lock. The next update waits until that is finished.
fn refuse_unfinished_cleanup(context: &UpdateContext) -> Result<(), UpdateRejection> {
    let entries = match std::fs::read_dir(context.home.root().join("update")) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(rejection(
                "update_state_unreadable",
                format!("could not read this CCCC home's update state: {error}"),
            ));
        }
    };
    for entry in entries.flatten() {
        let Some(id) = entry
            .file_name()
            .to_str()
            .filter(|name| uuid::Uuid::parse_str(name).is_ok_and(|id| !id.is_nil()))
            .map(str::to_owned)
        else {
            continue;
        };
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            let transaction = Transaction {
                id,
                dir: entry.path(),
            };
            // Its creator, such as this very host after a failed cleanup, must
            // stop before the cleanup may run.
            let action = if transaction
                .record()
                .is_ok_and(|record| record.creator.alive())
            {
                "stop CCCC, then finish it with"
            } else {
                "finish it with"
            };
            return Err(rejection(
                "update_cleanup_pending",
                format!(
                    "an earlier update left its files in {}; {action} `{}`",
                    transaction.dir.display(),
                    cleanup_command(context, &transaction)
                ),
            ));
        }
    }
    Ok(())
}

fn rejection(code: &str, message: impl Into<String>) -> UpdateRejection {
    UpdateRejection {
        code: code.into(),
        message: message.into(),
    }
}

fn release_version(value: &str) -> Result<String, UpdateRejection> {
    let value = value.trim();
    if value.len() > 64 || semver::Version::parse(value).is_err() {
        return Err(rejection(
            "invalid_version",
            "the update version must be a release version such as 0.4.59",
        ));
    }
    Ok(value.to_owned())
}

fn refuse_active_intent(home: &HomeLayout) -> Result<(), UpdateRejection> {
    match attended_update::active(home) {
        Ok(None) => Ok(()),
        Ok(Some(intent)) => Err(rejection(
            "update_in_progress",
            format!(
                "update {} holds this CCCC home: if it is still running, wait for it; if it was interrupted, recover it with `{}`",
                intent.transaction_id,
                intent.recovery_command()
            ),
        )),
        Err(error) => Err(rejection(
            "update_state_unreadable",
            format!("could not read this CCCC home's update state: {error}"),
        )),
    }
}

/// A forced exit can cut short an update that already holds this home. Name the
/// command that finishes it, or why the update state could not be read.
pub(crate) fn forced_exit_update_hint(home: &HomeLayout) -> Option<String> {
    match attended_update::active(home) {
        Ok(None) => None,
        Ok(Some(intent)) => Some(format!(
            "An update was in progress; finish it with `{}`.",
            intent.recovery_command()
        )),
        Err(error) => Some(format!(
            "could not read this CCCC home's update state: {error}"
        )),
    }
}

type PrepareFailure = (Option<Transaction>, UpdateRejection);

/// Fetch the target release's installer, have it stage the release, and check
/// the staged program. Nothing running is stopped and nothing is replaced.
async fn prepare(
    context: &UpdateContext,
    id: &str,
    version: &str,
    mode: UpdateMode,
) -> Result<Prepared, PrepareFailure> {
    let install_dir = context
        .release_dir()
        .map_err(|error| (None, rejection("update_unavailable", format!("{error:#}"))))?
        .to_path_buf();
    let source = Source::Release {
        install_dir: install_dir.clone(),
    };
    let transaction = Transaction::create(&context.home, id, source.clone()).map_err(|error| {
        (
            None,
            rejection(
                "update_prepare_failed",
                format!("could not create the update directory: {error}"),
            ),
        )
    })?;
    let fail = |code: &str, message: String| (Some(transaction.clone()), rejection(code, message));
    let installer = download_installer(version).await.map_err(|error| {
        fail(
            "update_download_failed",
            format!("could not download the CCCC {version} installer: {error:#}"),
        )
    })?;
    // Read, never run, before it is accepted: an installer that predates
    // phases would ignore them and install everything in one go.
    check_installer(&installer, version).map_err(|error| {
        fail(
            "update_candidate_rejected",
            format!("CCCC {version} cannot be installed this way: {error:#}"),
        )
    })?;
    write_installer(&transaction.script(), &installer).map_err(|error| {
        fail(
            "update_prepare_failed",
            format!("could not keep the CCCC {version} installer: {error:#}"),
        )
    })?;
    let run = run_phase(
        context,
        &transaction,
        "prepare",
        Some(version),
        PREPARE_TIMEOUT,
    )
    .await
    .map_err(|error| {
        fail(
            "update_prepare_failed",
            format!("the CCCC installer could not prepare {version}: {error:#}"),
        )
    })?;
    let staged = install_dir.join(format!("{PROGRAM}.cccc-install.{}", transaction.id));
    match run.value("CCCC_INSTALL_STAGED").filter(|_| run.success) {
        Some(reported) if Path::new(reported) == staged => {}
        Some(reported) => {
            return Err(fail(
                "update_prepare_failed",
                format!(
                    "the CCCC installer staged {reported} instead of {}",
                    staged.display()
                ),
            ));
        }
        None => {
            return Err(fail(
                "update_prepare_failed",
                format!(
                    "the CCCC installer could not prepare {version}: {}",
                    run.failure_summary()
                ),
            ));
        }
    }
    let check = self_check(&staged).await.map_err(|error| {
        fail(
            "update_candidate_rejected",
            format!("CCCC {version} did not pass its self-check: {error:#}"),
        )
    })?;
    if check.version != version {
        return Err(fail(
            "update_candidate_rejected",
            format!(
                "the staged program reports version {} instead of {version}",
                check.version
            ),
        ));
    }
    if !check.relay_schemas.contains(&RELAY_SCHEMA)
        || !check
            .capabilities
            .intent_schemas
            .contains(&attended_update::INTENT_SCHEMA)
    {
        return Err(fail(
            "update_candidate_rejected",
            format!("CCCC {version} cannot finish an update started by this version"),
        ));
    }
    // Whether this release can take the agents over is known only now, from
    // what it declares; the daemon decides again when it is asked to stop.
    if mode == UpdateMode::Keep {
        let mut decision =
            cccc_daemon::session_owner::preservation_check(&context.home, &check.capabilities)
                .await;
        if !decision.allowed {
            name_refusals(&context.home, &mut decision);
            return Err(fail("keep_refused", keep_refused(&decision)));
        }
    }
    Ok(Prepared {
        transaction: transaction.clone(),
        version: version.into(),
        mode,
        target: BuildIdentity {
            version: check.version,
            source_id: check.build.source_id,
        },
        capabilities: check.capabilities,
        source,
    })
}

/// Check the rebuilt program at the path this host was started from: it must
/// still be the file the check offered, answer its self-check as that build,
/// and be able to finish a restart this version starts. Nothing is downloaded,
/// installed or stopped; same and older versions are allowed.
async fn prepare_local(
    context: &UpdateContext,
    launch: &LaunchProgram,
    id: &str,
    token: &str,
    source_id: &str,
    mode: UpdateMode,
) -> Result<Prepared, PrepareFailure> {
    let program = &launch.path;
    let changed = || {
        (
            None,
            rejection(
                "local_changed",
                format!(
                    "{} changed after it was checked; check again",
                    program.display()
                ),
            ),
        )
    };
    let identity = FileIdentity::read(program).map_err(|error| {
        (
            None,
            rejection(
                "local_changed",
                format!("could not read {}: {error}", program.display()),
            ),
        )
    })?;
    if identity == launch.identity || identity.token().ok().as_deref() != Some(token) {
        return Err(changed());
    }
    let check = self_check(program).await.map_err(|error| {
        (
            None,
            rejection(
                "local_candidate_rejected",
                format!("the rebuilt program did not pass its self-check: {error:#}"),
            ),
        )
    })?;
    // What answered must be the file that was checked, not one a build wrote since.
    if FileIdentity::read(program).ok().as_ref() != Some(&identity)
        || check.build.source_id != source_id
    {
        return Err(changed());
    }
    let capabilities = &check.capabilities;
    if !capabilities.local_restart
        || !capabilities
            .intent_schemas
            .contains(&attended_update::INTENT_SCHEMA)
        || !check.relay_schemas.contains(&RELAY_SCHEMA)
    {
        return Err((
            None,
            rejection(
                "local_candidate_rejected",
                "the rebuilt program cannot finish a restart started by this version",
            ),
        ));
    }
    // A first look while CCCC still serves; the daemon decides again when it
    // is asked to stop.
    if mode == UpdateMode::Keep {
        let mut decision =
            cccc_daemon::session_owner::preservation_check(&context.home, &check.capabilities)
                .await;
        if !decision.allowed {
            name_refusals(&context.home, &mut decision);
            return Err((None, rejection("keep_refused", keep_refused(&decision))));
        }
    }
    let source = Source::Local {
        program: program.clone(),
        file_identity: identity,
    };
    let transaction = Transaction::create(&context.home, id, source.clone()).map_err(|error| {
        (
            None,
            rejection(
                "update_prepare_failed",
                format!("could not create the update directory: {error}"),
            ),
        )
    })?;
    Ok(Prepared {
        transaction,
        version: check.version.clone(),
        mode,
        target: BuildIdentity {
            version: check.version,
            source_id: check.build.source_id,
        },
        capabilities: check.capabilities,
        source,
    })
}

/// Release the install lock and the staged files of an update that was not
/// applied, then remove its directory. Only this transaction's own abort result
/// lets the directory go; otherwise its state and installer stay, so the
/// cleanup can be finished later. Without its installer nothing has run.
async fn undo(context: &UpdateContext, transaction: &Transaction) -> Result<()> {
    if transaction.script().is_file() {
        let run = run_phase(context, transaction, "abort", None, PHASE_TIMEOUT).await?;
        if !run.aborted(transaction) {
            bail!("{}", run.problem());
        }
    }
    transaction.remove();
    Ok(())
}

/// The target release's own installer, so its phases match what it stages.
async fn download_installer(version: &str) -> Result<Vec<u8>> {
    let repository =
        std::env::var("CCCC_GITHUB_REPOSITORY").unwrap_or_else(|_| "ChesterRa/cccc".into());
    if repository.split('/').count() != 2
        || repository.split('/').any(str::is_empty)
        || repository.bytes().any(|byte| {
            !(byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/'))
        })
    {
        bail!("CCCC_GITHUB_REPOSITORY must use the owner/repository form");
    }
    let configured = std::env::var("CCCC_RELEASE_BASE_URL")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let base = configured
        .clone()
        .unwrap_or_else(|| format!("https://github.com/{repository}/releases"));
    let prefix = std::env::var("CCCC_RELEASE_TAG_PREFIX").unwrap_or_else(|_| "v".into());
    let url = reqwest::Url::parse(&format!(
        "{}/download/{prefix}{version}/install.sh",
        base.trim_end_matches('/')
    ))
    .context("invalid release download URL")?;
    // Like the installer itself: official downloads stay on GitHub over HTTPS;
    // an explicitly configured release location is trusted as given.
    let policy = if configured.is_some() {
        reqwest::redirect::Policy::limited(10)
    } else {
        reqwest::redirect::Policy::custom(|attempt| {
            let allowed = attempt.url().scheme() == "https"
                && attempt.url().host_str().is_some_and(|host| {
                    host == "github.com" || host.ends_with(".githubusercontent.com")
                });
            if !allowed {
                attempt.error("release asset redirected outside GitHub HTTPS")
            } else if attempt.previous().len() >= 10 {
                attempt.stop()
            } else {
                attempt.follow()
            }
        })
    };
    if configured.is_none() && url.scheme() != "https" {
        bail!("release downloads must use HTTPS");
    }
    let client = reqwest::Client::builder()
        .timeout(DOWNLOAD_TIMEOUT)
        .redirect(policy)
        .build()?;
    let mut response = client
        .get(url)
        .header(reqwest::header::USER_AGENT, "cccc-standalone-updater")
        .send()
        .await?
        .error_for_status()?;
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if body.len() + chunk.len() > INSTALLER_MAX_BYTES {
            bail!("the installer is larger than {INSTALLER_MAX_BYTES} bytes");
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// A shell installer of exactly this release that implements the phases.
fn check_installer(body: &[u8], version: &str) -> Result<()> {
    let text = std::str::from_utf8(body).context("the installer is not UTF-8 text")?;
    if !text.starts_with("#!") {
        bail!("the downloaded file is not a shell installer");
    }
    if !text
        .lines()
        .any(|line| line == format!("DEFAULT_VERSION=\"{version}\""))
    {
        bail!("the downloaded installer does not belong to CCCC {version}");
    }
    if !text.lines().any(|line| line == INSTALLER_PHASE_API) {
        bail!("its installer cannot update a running CCCC in steps; update with `cccc update`");
    }
    Ok(())
}

/// Only a complete installer gets its name: no installer, nothing has run.
fn write_installer(destination: &Path, body: &[u8]) -> io::Result<()> {
    let partial = destination.with_extension("sh.part");
    let written =
        write_private_file(&partial, body).and_then(|()| std::fs::rename(&partial, destination));
    if written.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    written
}

#[cfg(unix)]
fn write_private_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(not(unix))]
fn write_private_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    std::fs::write(path, bytes)
}

/// One installer phase's outcome. The installer reports its results as
/// `CCCC_INSTALL_*=` lines; a result counts only with the exit status the
/// installer gives it and the value this transaction expects.
struct PhaseRun {
    success: bool,
    stdout: String,
    stderr: String,
}

impl PhaseRun {
    fn value(&self, key: &str) -> Option<&str> {
        self.stdout
            .lines()
            .rev()
            .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    fn reported(&self, key: &str, expected: &str) -> bool {
        self.value(key) == Some(expected)
    }

    fn aborted(&self, transaction: &Transaction) -> bool {
        self.success && self.reported("CCCC_INSTALL_ABORTED", &transaction.id)
    }

    /// Why the phase did not give the expected result.
    fn problem(&self) -> String {
        if self.success {
            "the installer did not report the expected result".into()
        } else {
            self.failure_summary()
        }
    }

    fn failure_summary(&self) -> String {
        let reason = self
            .stderr
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .map(|line| line.trim_start_matches("CCCC installer: ").trim());
        reason.unwrap_or("the installer gave no reason").to_owned()
    }
}

async fn run_phase(
    context: &UpdateContext,
    transaction: &Transaction,
    phase: &str,
    version: Option<&str>,
    timeout: Duration,
) -> Result<PhaseRun> {
    let mut command = cccc_runtime::background_command("sh");
    command
        .arg(transaction.script())
        .env("CCCC_INSTALL_PHASE", phase)
        .env("CCCC_INSTALL_TXN", &transaction.dir)
        .env("CCCC_INSTALL_DIR", context.release_dir()?)
        .env("CCCC_HOME", context.home.root())
        .env("CCCC_NO_MODIFY_PATH", "1")
        .env("CCCC_TRUSTED_EXISTING_CLI", &context.executable);
    match version {
        Some(version) => command.env("CCCC_VERSION", version),
        None => command.env_remove("CCCC_VERSION"),
    };
    // The installer runs in its own process group: when it exits, times out or
    // is dropped, nothing it started keeps running, so no later phase races it.
    let output = cccc_runtime::capture_command(&mut command, None, timeout, PHASE_OUTPUT_MAX_BYTES)
        .await
        .with_context(|| format!("the installer did not finish {phase}"))?;
    if output.stdout_truncated || output.stderr_truncated {
        bail!("the installer printed more than {PHASE_OUTPUT_MAX_BYTES} bytes during {phase}");
    }
    Ok(PhaseRun {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

/// Runs `update --self-check` of the program at `program`: a staged release, or
/// a rebuilt program at the path a host was started from.
pub(crate) async fn self_check(program: &Path) -> Result<SelfCheck> {
    let mut command = cccc_runtime::background_tokio_command(program);
    command
        .args(["update", "--self-check"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(SELF_CHECK_TIMEOUT, command.output())
        .await
        .map_err(|_| anyhow!("the program did not answer its self-check in time"))?
        .context("could not run the program")?;
    if !output.status.success() {
        bail!("the program cannot check itself ({})", output.status);
    }
    if output.stdout.len() > SELF_CHECK_MAX_BYTES {
        bail!("the program's self-check is too large");
    }
    let check: SelfCheck =
        serde_json::from_slice(&output.stdout).context("unreadable self-check")?;
    if check.schema != SELF_CHECK_SCHEMA {
        bail!("unsupported self-check schema {}", check.schema);
    }
    Ok(check)
}

/// Publish the startup intent right before the stop request. Until it is
/// withdrawn, only this update's relay may start a daemon for this home. The
/// relay starts whatever is at a rebuilt program's path, so that must still be
/// the file that was checked.
pub(crate) fn arm(context: &UpdateContext, prepared: &Prepared) -> Result<()> {
    if let Source::Local {
        program,
        file_identity,
    } = &prepared.source
    {
        let now = FileIdentity::read(program)
            .with_context(|| format!("could not read {}", program.display()))?;
        if &now != file_identity {
            bail!(
                "{} changed after it was checked; check again",
                program.display()
            );
        }
    }
    let intent = attended_update::Intent::new(
        &context.home,
        &prepared.transaction.id,
        prepared.target.clone(),
        prepared.source.clone(),
    )?;
    attended_update::arm(&context.home, &intent)?;
    Ok(())
}

pub(crate) enum ApplyOutcome {
    /// The new files are in place.
    Applied,
    /// The installer restored the previous files, or never replaced them.
    Restored { reason: String },
    /// The files may be neither; only offline recovery can tell.
    Unknown { reason: String },
}

/// Replace the files while holding the stopped daemon's lock.
pub(crate) async fn apply(
    context: &UpdateContext,
    prepared: &Prepared,
    _daemon_lock: &std::fs::File,
) -> ApplyOutcome {
    let transaction = &prepared.transaction;
    match run_phase(context, transaction, "apply", None, PHASE_TIMEOUT).await {
        Ok(run) if run.success && run.reported("CCCC_INSTALL_APPLIED", &prepared.version) => {
            ApplyOutcome::Applied
        }
        Ok(run) if !run.success && run.reported("CCCC_INSTALL_ROLLED_BACK", &transaction.id) => {
            ApplyOutcome::Restored {
                reason: run.failure_summary(),
            }
        }
        // Refused before it started, or its outcome is unknown: undo explicitly.
        Ok(run) => abort_after_failed_apply(context, transaction, run.problem()).await,
        Err(error) => abort_after_failed_apply(context, transaction, format!("{error:#}")).await,
    }
}

async fn abort_after_failed_apply(
    context: &UpdateContext,
    transaction: &Transaction,
    reason: String,
) -> ApplyOutcome {
    match run_phase(context, transaction, "abort", None, PHASE_TIMEOUT).await {
        Ok(run) if run.aborted(transaction) => ApplyOutcome::Restored { reason },
        Ok(run) => ApplyOutcome::Unknown {
            reason: format!("{reason}; restoring failed: {}", run.problem()),
        },
        Err(error) => ApplyOutcome::Unknown {
            reason: format!("{reason}; restoring failed: {error:#}"),
        },
    }
}

/// Release the install lock and the staged files without touching the daemon.
pub(crate) async fn abort_unapplied(context: &UpdateContext, prepared: &Prepared) -> Result<()> {
    let run = run_phase(context, &prepared.transaction, "abort", None, PHASE_TIMEOUT).await?;
    if !run.aborted(&prepared.transaction) {
        bail!("{}", run.problem());
    }
    Ok(())
}

pub(crate) fn write_relay(
    context: &UpdateContext,
    prepared: &Prepared,
    outcome: RelayOutcome,
    stopped: Option<ShutdownReport>,
) -> Result<()> {
    let record = RelayRecord {
        schema: RELAY_SCHEMA,
        transaction: prepared.transaction.id.clone(),
        from_version: crate::PRODUCT_VERSION.into(),
        to_version: prepared.version.clone(),
        source_id: prepared.target.source_id.clone(),
        mode: prepared.mode.as_str().into(),
        source: prepared.source.clone(),
        launch: context.launch.clone(),
        outcome,
        stopped,
    };
    cccc_core::fs::write_secret_json(&prepared.transaction.relay_path(), &record)?;
    Ok(())
}

/// A handed-over update on its way out of the host: `main` carries it out
/// once every other part of the host has stopped.
pub(crate) struct Handoff {
    pub context: UpdateContext,
    pub prepared: Prepared,
    /// What the relay record says, for the terminal's last line.
    pub outcome: RelayOutcome,
    pub relay: Relay,
}

impl Handoff {
    /// What starts next, as the terminal interface closes for it.
    #[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))]
    pub(crate) fn announcement(&self) -> String {
        match &self.outcome {
            RelayOutcome::Applied | RelayOutcome::LocalReady => {
                format!("Starting CCCC {}...", self.prepared.version)
            }
            RelayOutcome::Abandoned { stage, reason } => format!(
                "CCCC was not updated to {} ({stage}: {reason}); starting CCCC {} again...",
                self.prepared.version,
                crate::PRODUCT_VERSION
            ),
        }
    }
}

/// Replace this process with the installed program, which resumes from the
/// transaction's record. The daemon lock closes on exec; the startup intent
/// keeps ordinary starts out until the new daemon holds the lock again.
pub(crate) struct Relay {
    pub program: PathBuf,
    pub transaction: Transaction,
    pub daemon_lock: Option<std::fs::File>,
}

#[cfg(unix)]
pub(crate) fn exec(relay: &Relay) -> io::Error {
    use std::io::Write;
    use std::os::unix::process::CommandExt;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    cccc_runtime::background_command(&relay.program)
        .arg("update")
        .arg("--resume")
        .arg(&relay.transaction.dir)
        .exec()
}

/// Carry out a relay. Returns only when exec fails and the previous program
/// could not be restored and started either.
#[cfg(unix)]
pub(crate) async fn relay(context: &UpdateContext, prepared: Prepared, relay: Relay) -> Result<()> {
    let error = exec(&relay);
    let record = prepared.transaction.read_relay()?;
    if record.outcome == RelayOutcome::LocalReady {
        bail!(
            "could not start the local CCCC build ({error}); recover with `{}`",
            recovery_command(&context.home)
        );
    }
    if record.outcome == RelayOutcome::Applied {
        // Still the old image, holding the daemon lock: put the old files back.
        let reason = format!("could not start the new program: {error}");
        // The lock lives in `relay` until this function returns.
        relay
            .daemon_lock
            .as_ref()
            .context("the daemon lock was released before exec")?;
        match abort_after_failed_apply(context, &prepared.transaction, reason.clone()).await {
            ApplyOutcome::Restored { .. } => {
                attended_update::disarm(&context.home, &prepared.transaction.id)?;
                write_relay(
                    context,
                    &prepared,
                    RelayOutcome::Abandoned {
                        stage: "exec".into(),
                        reason,
                    },
                    None,
                )?;
                let retry = exec(&relay);
                bail!("could not start CCCC again after restoring it: {retry}");
            }
            ApplyOutcome::Applied | ApplyOutcome::Unknown { .. } => {}
        }
        bail!(
            "could not start the updated CCCC ({error}); recover with `{}`",
            recovery_command(&context.home)
        );
    }
    bail!("could not start CCCC again after the update was abandoned: {error}")
}

pub(crate) fn recovery_command(home: &HomeLayout) -> String {
    match attended_update::active(home) {
        Ok(Some(intent)) => intent.recovery_command(),
        _ => "cccc".into(),
    }
}

/// Why agents cannot keep running, in CCCC's own words, one reason each.
pub(crate) fn keep_refused(decision: &KeepDecision) -> String {
    let reasons = decision
        .refusals
        .iter()
        .map(refusal_text)
        .collect::<Vec<_>>();
    format!("agents cannot keep running: {}", reasons.join(" "))
}

/// One reason, led by the agent it is about when it is about one.
pub(crate) fn refusal_text(refusal: &KeepRefusal) -> String {
    match refusal.display_name.as_ref().or(refusal.actor_id.as_ref()) {
        Some(name) => format!("{name}: {}", refusal.message),
        None => refusal.message.clone(),
    }
}

/// How the daemon answered a stop that keeps agents.
pub(crate) enum KeepingStop {
    /// It may be stopping; the receipt for this request decides what happened.
    Requested(String),
    /// It refused before choosing to stop, and stopped nothing. `withdrawn`
    /// says whether the update's intent was lifted; if not, only recovery may
    /// finish it, and it needs the update's record.
    Refused { reason: String, withdrawn: bool },
}

/// Ask the daemon to stop and keep every agent running, after it checks each
/// one again. Only a refusal it states before choosing to stop withdraws the
/// update's intent; any other answer may have stopped it.
pub(crate) async fn request_keeping_stop(
    client: &cccc_client::DaemonClient,
    home: &HomeLayout,
    prepared: &Prepared,
) -> KeepingStop {
    use cccc_client::attended_update::PreservationStopReply;
    let request_id = uuid::Uuid::new_v4().to_string();
    let reply = client
        .request_preserving_shutdown(
            std::process::id(),
            &request_id,
            &prepared.transaction.id,
            &prepared.candidate(),
        )
        .await;
    match reply {
        Ok(PreservationStopReply::Refused(mut decision)) => {
            name_refusals(home, &mut decision);
            let reason = keep_refused(&decision);
            match attended_update::disarm(home, &prepared.transaction.id) {
                Ok(()) => KeepingStop::Refused {
                    reason,
                    withdrawn: true,
                },
                // Nothing stopped, but only recovery may lift the intent now.
                Err(error) => KeepingStop::Refused {
                    reason: format!(
                        "{reason} Its update intent could not be withdrawn ({error}); after stopping CCCC, run `{}`.",
                        recovery_command(home)
                    ),
                    withdrawn: false,
                },
            }
        }
        Ok(PreservationStopReply::Requested { .. }) => KeepingStop::Requested(request_id),
        Err(error) => {
            cccc_runtime::diagnostic(format_args!(
                "[cccc] The daemon gave no clear answer to keeping agents running ({error}); waiting for its stop"
            ));
            KeepingStop::Requested(request_id)
        }
    }
}

/// Write this home's latest attended update result. Paths are fixed.
pub(crate) fn record_result(
    home: &HomeLayout,
    record: &RelayRecord,
    result: &str,
    agents: UpdateAgents,
) {
    let (stage, reason) = match &record.outcome {
        RelayOutcome::Applied | RelayOutcome::LocalReady => (None, None),
        RelayOutcome::Abandoned { stage, reason } => (Some(stage.clone()), Some(reason.clone())),
    };
    let (source, token) = result_source(&record.source);
    write_last(
        home,
        LastResult {
            transaction: record.transaction.clone(),
            from_version: Some(record.from_version.clone()),
            to_version: record.to_version.clone(),
            mode: Some(record.mode.clone()),
            result: result.into(),
            stage,
            reason,
            agents,
            finished_at: cccc_contracts::utc_now(),
            source,
            token,
            source_id: Some(record.source_id.clone()),
        },
    );
}

/// The Web closed for an update that then could not start; nothing stopped.
/// `stage` names the step that refused it.
pub(crate) fn record_not_started(
    context: &UpdateContext,
    prepared: &Prepared,
    stage: &str,
    reason: &str,
) {
    let (source, token) = result_source(&prepared.source);
    write_last(
        &context.home,
        LastResult {
            transaction: prepared.transaction.id.clone(),
            from_version: Some(crate::PRODUCT_VERSION.into()),
            to_version: prepared.version.clone(),
            mode: Some(prepared.mode.as_str().into()),
            result: "not_started".into(),
            stage: Some(stage.into()),
            reason: Some(reason.into()),
            agents: agents_not_observed(),
            finished_at: cccc_contracts::utc_now(),
            source,
            token,
            source_id: Some(prepared.target.source_id.clone()),
        },
    );
}

fn write_last(home: &HomeLayout, last: LastResult) {
    if let Err(error) =
        cccc_core::fs::write_secret_json(&home.root().join("update/last.json"), &last)
    {
        cccc_runtime::diagnostic(format_args!("could not record the update result: {error}"));
    }
}

/// After the relay: the daemon is ready and the Web listens on the new
/// program, so the previous files can go and ordinary starts may resume. The
/// result names the agents the update stopped or kept, as found now. An error
/// says what is left to do.
pub(crate) async fn confirm(
    home: &HomeLayout,
    transaction: &Transaction,
    record: &RelayRecord,
) -> Result<UpdateAgents, String> {
    let Some(install_dir) = record.source.release_dir() else {
        return confirm_local(home, transaction, record).await;
    };
    let context = UpdateContext::for_installation(home, install_dir.to_path_buf());
    let run = run_phase(&context, transaction, "confirm", None, PHASE_TIMEOUT).await;
    let confirmed = matches!(
        &run,
        Ok(run) if run.success && run.reported("CCCC_INSTALL_CONFIRMED", &record.to_version)
    );
    let problem = if !confirmed {
        let reason = match run {
            Ok(run) => run.problem(),
            Err(error) => format!("{error:#}"),
        };
        Some(format!(
            "CCCC {} is running, but the update could not be confirmed: {reason}; stop CCCC, then recover with `{}`",
            record.to_version,
            recovery_command(home)
        ))
    } else if let Err(error) = attended_update::disarm(home, &transaction.id) {
        Some(format!(
            "CCCC {} is running, but its update intent could not be withdrawn: {error}; stop CCCC, then finish it with `{}`",
            record.to_version,
            recovery_command(home)
        ))
    } else {
        None
    };
    if let Some(problem) = problem {
        cccc_runtime::diagnostic(format_args!("{problem}"));
        record_result(home, record, "unconfirmed", agents_not_observed());
        return Err(problem);
    }
    let observing = record_first(home, record, "installed");
    transaction.remove();
    cccc_runtime::diagnostic(format_args!(
        "[cccc] Updated CCCC {} to {}",
        record.from_version, record.to_version
    ));
    Ok(record_agents(home, record, "installed", observing).await)
}

/// A restart into a rebuilt program replaced no files: once it runs, ordinary
/// starts may resume.
async fn confirm_local(
    home: &HomeLayout,
    transaction: &Transaction,
    record: &RelayRecord,
) -> Result<UpdateAgents, String> {
    if let Err(error) = attended_update::disarm(home, &transaction.id) {
        let problem = format!(
            "CCCC {} is running, but its restart intent could not be withdrawn: {error}; stop CCCC, then finish it with `{}`",
            record.to_version,
            recovery_command(home)
        );
        cccc_runtime::diagnostic(format_args!("{problem}"));
        record_result(home, record, "unconfirmed", agents_not_observed());
        return Err(problem);
    }
    let observing = record_first(home, record, "restarted");
    transaction.remove();
    cccc_runtime::diagnostic(format_args!(
        "[cccc] Restarted CCCC into the local build {}",
        record.to_version
    ));
    Ok(record_agents(home, record, "restarted", observing).await)
}

/// Record the result at once. When its stop named agents, they are recorded
/// as not yet confirmed while the host looks for them.
fn record_first(home: &HomeLayout, record: &RelayRecord, result: &str) -> Option<Observing> {
    let Some(stopped) = &record.stopped else {
        record_result(home, record, result, agents_not_observed());
        return None;
    };
    let observing = Observing::start();
    let looking = Reconciled {
        unconfirmed: expected(stopped)
            .iter()
            .chain(&stopped.unconfirmed)
            .cloned()
            .collect(),
        ..Reconciled::default()
    };
    let mut agents = named(home, looking);
    agents.state = "observing".into();
    record_result(home, record, result, agents);
    Some(observing)
}

/// Look for the agents of a result just recorded and record what was found,
/// unless a later update's result has replaced it meanwhile.
async fn record_agents(
    home: &HomeLayout,
    record: &RelayRecord,
    result: &str,
    observing: Option<Observing>,
) -> UpdateAgents {
    let (Some(_observing), Some(stopped)) = (observing, &record.stopped) else {
        return agents_not_observed();
    };
    let agents = observe_agents(home, stopped).await;
    let last: Option<LastResult> =
        cccc_core::fs::read_json(&home.root().join("update/last.json")).ok();
    if last.is_some_and(|last| last.transaction == record.transaction) {
        record_result(home, record, result, agents.clone());
    }
    agents
}

/// The agents a stop receipt says the update kept or stopped.
fn expected(stopped: &ShutdownReport) -> &[AgentIdentity] {
    match stopped.mode {
        ShutdownMode::PreserveAgents => &stopped.retained,
        ShutdownMode::Stop => &stopped.confirmed_stopped,
    }
}

/// Look for the agents the stop receipt names, for a few seconds after CCCC
/// is back: one kept running should be found with the same launch, one
/// stopped with a launch of its own. What one look proves stays proven.
async fn observe_agents(home: &HomeLayout, stopped: &ShutdownReport) -> UpdateAgents {
    let deadline = Instant::now() + AGENTS_OBSERVE_BUDGET;
    let mut found = Reconciled::default();
    loop {
        let budget = deadline
            .saturating_duration_since(Instant::now())
            .clamp(Duration::from_millis(500), Duration::from_secs(3));
        let observed = cccc_daemon::session_owner::observations(home, budget)
            .await
            .ok();
        found = reconcile(stopped, observed.as_ref(), &found);
        if found.settled || Instant::now() >= deadline {
            return named(home, found);
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// The agents a stop receipt names, as one observation found them.
#[derive(Debug, Default, PartialEq, Eq)]
struct Reconciled {
    reattached: Vec<AgentIdentity>,
    restarted: Vec<AgentIdentity>,
    not_back: Vec<AgentIdentity>,
    unconfirmed: Vec<AgentIdentity>,
    /// Every agent the update kept or stopped was found running.
    settled: bool,
}

/// Each agent the receipt names, as this observation found it. One that an
/// `earlier` look found kept or started again stays so, with the launch then
/// found: a holder that does not answer now does not undo that.
fn reconcile(
    stopped: &ShutdownReport,
    observed: Option<&AgentObservations>,
    earlier: &Reconciled,
) -> Reconciled {
    let same_agent = |one: &AgentIdentity, other: &AgentIdentity| {
        one.group_id == other.group_id
            && one.actor_id == other.actor_id
            && one.actor_created_at == other.actor_created_at
    };
    let keep = stopped.mode == ShutdownMode::PreserveAgents;
    let mut found = Reconciled {
        settled: true,
        ..Reconciled::default()
    };
    for identity in expected(stopped) {
        let proven = |list: &[AgentIdentity]| {
            list.iter()
                .find(|proven| same_agent(proven, identity))
                .cloned()
        };
        if let Some(proven) = proven(&earlier.reattached) {
            found.reattached.push(proven);
            continue;
        }
        if let Some(proven) = proven(&earlier.restarted) {
            found.restarted.push(proven);
            continue;
        }
        let running = observed.and_then(|observed| {
            observed
                .running
                .iter()
                .find(|running| same_agent(running, identity))
        });
        // A launch without a known generation proves neither.
        let known = identity.generation.is_some();
        match running {
            // Kept: the same launch, with the holder that kept it.
            Some(running)
                if keep
                    && known
                    && identity.owner_id.is_some()
                    && running.generation == identity.generation
                    && running.owner_id == identity.owner_id =>
            {
                found.reattached.push(running.clone());
            }
            // Started again, with a launch of its own.
            Some(running)
                if known
                    && running.generation.is_some()
                    && running.generation != identity.generation =>
            {
                found.restarted.push(running.clone());
            }
            _ => {
                found.settled = false;
                let unknown = running.is_some()
                    || observed.is_none_or(|observed| {
                        !observed.unavailable_owners.is_empty()
                            || observed
                                .unconfirmed
                                .iter()
                                .any(|other| same_agent(other, identity))
                    });
                // Only a kept agent's absence is evidence: every holder answered
                // and none holds it. After a restart the daemon starts agents
                // again in the background, one by one, so one not seen yet may
                // still start.
                if unknown || !keep {
                    found.unconfirmed.push(identity.clone());
                } else {
                    found.not_back.push(identity.clone());
                }
            }
        }
    }
    // What the stop itself could not confirm stays unconfirmed.
    found
        .unconfirmed
        .extend(stopped.unconfirmed.iter().cloned());
    found
}

/// Agents' current titles, each Group read once. The daemon names agents by
/// ID; the person reading a result knows them by title.
struct Titles {
    store: Option<cccc_core::GroupStore>,
    groups: std::collections::BTreeMap<String, Option<cccc_core::GroupDoc>>,
}

impl Titles {
    fn new(home: &HomeLayout) -> Self {
        Self {
            store: cccc_core::GroupStore::new(home.clone()).ok(),
            groups: std::collections::BTreeMap::new(),
        }
    }

    /// The title of `actor_id`, while it is still the agent created at
    /// `created_at` when that is known.
    fn title(
        &mut self,
        group_id: &str,
        actor_id: &str,
        created_at: Option<&str>,
    ) -> Option<String> {
        let store = self.store.as_ref();
        self.groups
            .entry(group_id.to_owned())
            .or_insert_with(|| store.and_then(|store| store.load(group_id).ok()))
            .as_ref()?
            .actors
            .iter()
            .find(|actor| {
                actor.id == actor_id && created_at.is_none_or(|created| actor.created_at == created)
            })
            .map(|actor| actor.title.trim().to_owned())
            .filter(|title| !title.is_empty())
    }
}

/// Name each agent a refusal is about by its current title.
pub(crate) fn name_refusals(home: &HomeLayout, decision: &mut KeepDecision) {
    let mut titles = Titles::new(home);
    for refusal in &mut decision.refusals {
        if let (Some(group_id), Some(actor_id)) = (&refusal.group_id, &refusal.actor_id) {
            refusal.display_name = titles.title(group_id, actor_id, None);
        }
    }
}

/// Name each agent by its current title, while it is still the same agent.
fn named(home: &HomeLayout, found: Reconciled) -> UpdateAgents {
    let mut titles = Titles::new(home);
    let mut name = |identity: &AgentIdentity| UpdateAgent {
        group_id: identity.group_id.clone(),
        actor_id: identity.actor_id.clone(),
        actor_created_at: identity.actor_created_at.clone(),
        generation: identity.generation.clone(),
        owner_id: identity.owner_id.clone(),
        title: titles.title(
            &identity.group_id,
            &identity.actor_id,
            Some(&identity.actor_created_at),
        ),
    };
    let mut names = |list: Vec<AgentIdentity>| list.iter().map(&mut name).collect::<Vec<_>>();
    let unconfirmed = names(found.unconfirmed);
    UpdateAgents {
        state: if unconfirmed.is_empty() {
            "observed"
        } else {
            "unconfirmed"
        }
        .into(),
        reattached: names(found.reattached),
        restarted: names(found.restarted),
        not_back: names(found.not_back),
        unconfirmed,
    }
}

/// The agents a result names, for the terminal's one-line notice. Those not
/// found or not confirmed lead, so a narrow terminal still shows them.
pub(crate) fn agents_line(agents: &UpdateAgents) -> String {
    let names = |list: &[UpdateAgent]| {
        list.iter()
            .map(|agent| {
                agent
                    .title
                    .clone()
                    .unwrap_or_else(|| agent.actor_id.clone())
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    [
        ("Not running", &agents.not_back),
        ("Could not be confirmed", &agents.unconfirmed),
        ("Kept running", &agents.reattached),
        ("Restarted", &agents.restarted),
    ]
    .into_iter()
    .filter(|(_, list)| !list.is_empty())
    .map(|(label, list)| format!("{label}: {}.", names(list)))
    .collect::<Vec<_>>()
    .join(" ")
}

/// What `--resume` hands the host: the transaction to confirm once the daemon
/// is ready and the Web listens on the new program.
pub(crate) struct Resumed {
    pub transaction: Transaction,
    pub record: RelayRecord,
}

/// `cccc update --resume`: start the host again from the recorded launch.
pub(crate) async fn resume(home: HomeLayout, dir: PathBuf) -> Result<crate::web_host::LaunchExit> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    reap_inherited_children();
    let transaction = Transaction::open(&home, &dir)?;
    let record = transaction.read_relay()?;
    let launch = record.launch.clone();
    let terminal = {
        use std::io::IsTerminal;
        launch.terminal && std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
    };
    let web_mode = launch.web_mode()?;
    match &record.outcome {
        RelayOutcome::Applied | RelayOutcome::LocalReady => {
            crate::web_host::launch_resumed(
                home,
                launch.host,
                launch.port,
                web_mode,
                terminal,
                Resumed {
                    transaction,
                    record,
                },
            )
            .await
        }
        RelayOutcome::Abandoned { stage, reason } => {
            cccc_runtime::diagnostic(format_args!(
                "[cccc] CCCC was not updated to {} ({stage}: {reason}); starting CCCC {} again",
                record.to_version, record.from_version
            ));
            record_result(&home, &record, "abandoned", agents_not_observed());
            transaction.remove();
            let result = crate::host_state::UpdateResult {
                transaction: transaction.id.clone(),
                failed: true,
                message: format!(
                    "CCCC {} started again: it was not updated to {} ({stage}: {reason}).",
                    record.from_version, record.to_version
                ),
            };
            crate::web_host::launch_after_abandoned(
                home,
                launch.host,
                launch.port,
                web_mode,
                terminal,
                result,
            )
            .await
        }
    }
}

/// `cccc update --recover`: finish an interrupted update from what its
/// installer reports. While the previous program is kept, the update is undone.
/// Once the installer reports it gone, the installed version the user confirmed
/// is kept, after it proves to be that build. An update that never reserved
/// this home's startup replaced nothing, so only its files are cleaned up. A
/// restart into a rebuilt program replaced nothing either, so it is given up.
pub(crate) async fn recover(home: &HomeLayout, dir: &Path) -> Result<()> {
    // The intent first: when it cannot be used, its error says how to get out,
    // even if the transaction directory is gone.
    let active = attended_update::active(home)?;
    let transaction = Transaction::open(home, dir)?;
    let Some(intent) = active.filter(|intent| intent.transaction_id == transaction.id) else {
        return clean_up(home, &transaction).await;
    };
    let Some(install_dir) = intent.source.release_dir() else {
        return recover_local(home, &transaction, &intent);
    };
    let context = UpdateContext::for_installation(home, install_dir.to_path_buf());
    // Proves that the update host has exited and that no daemon runs for this
    // home while files are restored or kept.
    let _daemon_lock = attended_update::claim_stopped(home, &transaction.id)?;
    if !transaction.script().is_file() {
        bail!(
            "update {} is damaged: its installer {} is missing, so what it changed cannot be told; nothing was changed. \
             After checking {} by hand, remove {} to start CCCC again",
            transaction.id,
            transaction.script().display(),
            install_dir.display(),
            home.root().join("update/active.json").display()
        );
    }
    let target = &intent.target;
    let run = run_phase(&context, &transaction, "abort", None, PHASE_TIMEOUT).await?;
    if run.aborted(&transaction) {
        attended_update::disarm(home, &transaction.id)?;
        record_recovered(home, &transaction, target, &intent.source, "undone");
        transaction.remove();
        println!(
            "The interrupted update was undone; the previous CCCC is in place. Start CCCC again with `cccc`."
        );
        return Ok(());
    }
    if run.success || !run.reported("CCCC_INSTALL_COMMITTED", &target.version) {
        bail!(
            "could not undo update {}: {}; it still holds this CCCC home, and this command can be run again",
            transaction.id,
            run.problem()
        );
    }
    // The previous program is gone. Keep the version the user confirmed, once
    // the installed program proves to be the build this update installed.
    let check = self_check(&context.executable).await.with_context(|| {
        format!(
            "update {} cannot be undone, and the installed program did not pass its self-check; nothing was changed",
            transaction.id
        )
    })?;
    if check.version != target.version || check.build.source_id != target.source_id {
        bail!(
            "update {} cannot be undone, and the installed program is not the CCCC {} it installed; nothing was changed",
            transaction.id,
            target.version
        );
    }
    let run = run_phase(&context, &transaction, "confirm", None, PHASE_TIMEOUT).await?;
    if !(run.success && run.reported("CCCC_INSTALL_CONFIRMED", &target.version)) {
        bail!(
            "could not finish update {}: {}; this command can be run again",
            transaction.id,
            run.problem()
        );
    }
    attended_update::disarm(home, &transaction.id)?;
    record_recovered(home, &transaction, target, &intent.source, "kept");
    transaction.remove();
    println!(
        "The update had already replaced the previous program, so CCCC {} stays installed. CCCC is not running; start it with `cccc`.",
        target.version
    );
    Ok(())
}

/// A restart into a rebuilt program changed no files, so nothing is undone:
/// once its host has exited, ordinary starts may resume, and they start
/// whatever is at the program's path now.
fn recover_local(
    home: &HomeLayout,
    transaction: &Transaction,
    intent: &attended_update::Intent,
) -> Result<()> {
    // Proves that the host has exited and that no daemon runs for this home.
    let _daemon_lock = attended_update::claim_stopped(home, &transaction.id)?;
    finish_local_recovery(home, transaction, intent)?;
    println!(
        "The interrupted restart was given up; CCCC changed no files. CCCC is not running; start it with `cccc`."
    );
    Ok(())
}

/// A normal invocation can finish an interrupted Local restart. The active
/// intent remains authoritative until the previous host and the daemon lock
/// both allow recovery.
pub(crate) fn recover_local_before_start(
    home: &HomeLayout,
) -> Result<Option<crate::host_state::UpdateResult>> {
    let Some(intent) = attended_update::active(home)? else {
        return Ok(None);
    };
    if !matches!(&intent.source, Source::Local { .. }) || !intent.holder.has_exited() {
        return Ok(None);
    }
    let _daemon_lock = match attended_update::claim_stopped(home, &intent.transaction_id) {
        Ok(lock) => lock,
        Err(error) => {
            // Another startup may have finished this Local recovery first.
            if attended_update::active(home)?.is_none() {
                return Ok(None);
            }
            if error.kind() == io::ErrorKind::WouldBlock {
                return Err(error).context(
                    "Another CCCC process holds this home's daemon lock. Retry after it finishes. Run `cccc daemon status` if the lock stays busy",
                );
            }
            return Err(error.into());
        }
    };
    let transaction = Transaction::open(home, &intent.transaction_dir)?;
    finish_local_recovery(home, &transaction, &intent)?;
    let message = format!(
        "The restart to the selected local build (CCCC {}) did not complete and was abandoned. Starting CCCC {} from the program you launched.",
        intent.target.version,
        crate::PRODUCT_VERSION,
    );
    cccc_runtime::diagnostic(format_args!("[cccc] {message}"));
    Ok(Some(crate::host_state::UpdateResult {
        transaction: transaction.id,
        failed: true,
        message,
    }))
}

fn finish_local_recovery(
    home: &HomeLayout,
    transaction: &Transaction,
    intent: &attended_update::Intent,
) -> Result<()> {
    attended_update::disarm(home, &transaction.id)?;
    record_recovered(
        home,
        transaction,
        &intent.target,
        &intent.source,
        "abandoned",
    );
    transaction.remove();
    Ok(())
}

/// Release what an update that never reserved this home's startup still
/// holds: its staged files and the install lock. Nothing was replaced. Only
/// the CCCC that created it, or anyone once that CCCC has exited, may do so:
/// otherwise this could undo files a running preparation is still writing.
async fn clean_up(home: &HomeLayout, transaction: &Transaction) -> Result<()> {
    let record = transaction.record().with_context(|| {
        format!(
            "update {} cannot be cleaned up without its record; after checking the CCCC installation for its files, remove {} by hand",
            transaction.id,
            transaction.dir.display()
        )
    })?;
    if record.creator != NativeHost::capture(std::process::id())? && !record.creator.has_exited() {
        bail!(
            "update {} belongs to CCCC process {}, whose exit is not confirmed; stop that CCCC, then run this command again",
            transaction.id,
            record.creator.pid
        );
    }
    // A restart into a rebuilt program staged nothing.
    if matches!(record.source, Source::Local { .. }) {
        transaction.remove();
        println!("The unfinished restart was cleaned up; nothing was changed.");
        return Ok(());
    }
    if !transaction.script().is_file() {
        transaction.remove();
        println!("The unfinished update was cleaned up; the installed CCCC was not changed.");
        return Ok(());
    }
    let kept = || {
        format!(
            "its files are kept in {}, and this command can be run again",
            transaction.dir.display()
        )
    };
    let install_dir = record
        .source
        .release_dir()
        .context("the update record names no installation")?;
    let context = UpdateContext::for_installation(home, install_dir.to_path_buf());
    let run = run_phase(&context, transaction, "abort", None, PHASE_TIMEOUT)
        .await
        .with_context(|| format!("could not clean up update {}; {}", transaction.id, kept()))?;
    if run.aborted(transaction) {
        transaction.remove();
        println!("The unfinished update was cleaned up; the installed CCCC was not changed.");
        return Ok(());
    }
    // An update that was confirmed but left its directory behind: the installer
    // reports it committed. The installed program stays; only the rest goes.
    let Some(version) = run
        .value("CCCC_INSTALL_COMMITTED")
        .filter(|_| !run.success)
        .map(str::to_owned)
    else {
        bail!(
            "could not clean up update {}: {}; {}",
            transaction.id,
            run.problem(),
            kept()
        );
    };
    let confirmed = run_phase(&context, transaction, "confirm", None, PHASE_TIMEOUT)
        .await
        .with_context(|| format!("could not finish update {}; {}", transaction.id, kept()))?;
    if !(confirmed.success && confirmed.reported("CCCC_INSTALL_CONFIRMED", &version)) {
        bail!(
            "could not finish update {}: {}; {}",
            transaction.id,
            confirmed.problem(),
            kept()
        );
    }
    transaction.remove();
    println!(
        "CCCC {version} stays installed; the remaining files of update {} were removed.",
        transaction.id
    );
    Ok(())
}

fn record_recovered(
    home: &HomeLayout,
    transaction: &Transaction,
    target: &BuildIdentity,
    source: &Source,
    result: &str,
) {
    // The relay record exists only when the host got as far as the relay.
    let relay = transaction.read_relay().ok();
    let (source, token) = result_source(source);
    let source_id = Some(target.source_id.clone());
    write_last(
        home,
        LastResult {
            transaction: transaction.id.clone(),
            from_version: relay.as_ref().map(|relay| relay.from_version.clone()),
            to_version: target.version.clone(),
            mode: relay.map(|relay| relay.mode),
            result: result.into(),
            stage: Some("recover".into()),
            reason: None,
            agents: agents_not_observed(),
            finished_at: cccc_contracts::utc_now(),
            source,
            token,
            source_id,
        },
    );
}

/// Children of the replaced program survive exec without anyone to wait for
/// them. Reap each one when it exits, without touching children started later.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn reap_inherited_children() {
    let Ok(children) = cccc_runtime::process_children::children_of(std::process::id()) else {
        return;
    };
    // An unreaped child keeps its PID, so each wait can only ever end that child.
    for pid in children {
        let Ok(pid) = i32::try_from(pid) else {
            continue;
        };
        std::thread::spawn(move || {
            let _ = nix::sys::wait::waitpid(nix::unistd::Pid::from_raw(pid), None);
        });
    }
}

#[cfg(test)]
#[path = "update_host_tests.rs"]
mod tests;
