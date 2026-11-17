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
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use cccc_core::HomeLayout;
use cccc_core::attended_update::{self, BuildIdentity};
use cccc_core::local_sessions::NativeHost;
use cccc_web::{UpdateMode, UpdatePreparation, UpdateRejection, UpdateRequest};
use serde::{Deserialize, Serialize};
use tokio::sync::{oneshot, watch};

/// The relay record this build writes before exec and reads after it. A
/// candidate lists the schemas its `--resume` reads in its self-check.
pub(crate) const RELAY_SCHEMA: u32 = 1;
const SELF_CHECK_SCHEMA: u32 = 1;
const TRANSACTION_SCHEMA: u32 = 1;
const INSTALLER_MAX_BYTES: usize = 1024 * 1024;
/// The installer's own line naming the phase contract it implements.
const INSTALLER_PHASE_API: &str = "# CCCC_INSTALL_PHASE_API=1";
const SELF_CHECK_MAX_BYTES: usize = 64 * 1024;
const PHASE_OUTPUT_MAX_BYTES: usize = 256 * 1024;
const PROGRAM: &str = if cfg!(windows) { "cccc.exe" } else { "cccc" };
/// The host sees its own relay only; agents are not checked yet.
const AGENTS_NOT_OBSERVED: &str = "not_observed";
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
    };
    println!("{}", serde_json::to_string(&check)?);
    Ok(())
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
    pub mode: String,
    pub install_dir: PathBuf,
    pub launch: LaunchSpec,
    pub outcome: RelayOutcome,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum RelayOutcome {
    /// The new files are in place; the new program finishes the update.
    Applied,
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
    pub agents: String,
    pub finished_at: String,
}

/// Written when a transaction is created, so an update that never reserved
/// this home's startup can still be cleaned up in its own installation, and
/// never behind the back of the CCCC that is still preparing it.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TransactionRecord {
    schema: u32,
    install_dir: PathBuf,
    creator: NativeHost,
}

/// One update's private directory under `<home>/update`.
#[derive(Clone, Debug)]
pub(crate) struct Transaction {
    pub id: String,
    pub dir: PathBuf,
}

impl Transaction {
    fn create(home: &HomeLayout, id: &str, install_dir: &Path) -> io::Result<Self> {
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
                install_dir: install_dir.to_path_buf(),
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
        if record.schema != TRANSACTION_SCHEMA || !record.install_dir.is_absolute() {
            bail!(
                "update record {} is not usable",
                self.record_path().display()
            );
        }
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

/// The installation and launch an update replaces and replays.
#[derive(Clone, Debug)]
pub(crate) struct UpdateContext {
    pub home: HomeLayout,
    pub install_dir: PathBuf,
    /// The installed program: what is replaced, and what the host relays to.
    pub executable: PathBuf,
    pub launch: LaunchSpec,
}

impl UpdateContext {
    /// Only a standalone installation can replace its own files in place. The
    /// launch to replay is filled in when the Web hands an update over.
    pub(crate) fn for_running_program(home: &HomeLayout) -> Result<Self> {
        let executable =
            std::env::current_exe().context("could not resolve the CCCC executable")?;
        let install_dir = crate::commands::update::standalone_install_dir(&executable)?;
        Ok(Self::for_installation(home, install_dir))
    }

    fn for_installation(home: &HomeLayout, install_dir: PathBuf) -> Self {
        Self {
            home: home.clone(),
            executable: install_dir.join(PROGRAM),
            install_dir,
            launch: LaunchSpec::default(),
        }
    }
}

/// A prepared update: staged, checked, and holding the install lock.
#[derive(Debug)]
pub(crate) struct Prepared {
    pub transaction: Transaction,
    pub version: String,
    pub mode: UpdateMode,
    pub target: BuildIdentity,
}

enum Slot {
    Idle,
    /// A preparation or its cleanup is running. `abandoned` asks it to undo.
    Busy {
        id: String,
        abandoned: bool,
    },
    Prepared(Prepared),
}

/// The host's one attended update, shared with the Web as its update hook.
#[derive(Clone)]
pub(crate) struct Coordinator {
    inner: Arc<Inner>,
}

struct Inner {
    context: UpdateContext,
    slot: Mutex<Slot>,
    /// Bumped whenever a preparation or cleanup settles.
    settled: watch::Sender<u64>,
    /// Wakes the running preparation once it is abandoned.
    cancel: tokio::sync::Notify,
}

impl Coordinator {
    pub(crate) fn new(context: UpdateContext) -> Self {
        Self {
            inner: Arc::new(Inner {
                context,
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
                Some(prepared)
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
                            Slot::Prepared(prepared) => Some(prepared),
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
        if request.mode == UpdateMode::Keep {
            return Err(rejection(
                "keep_preflight_incomplete",
                "keeping agents running through an update is not available yet; choose restart",
            ));
        }
        let version = release_version(&request.version)?;
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
            let result = prepare_unless_abandoned(&inner, &id, &version, mode).await;
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
                        Slot::Prepared(prepared) => Some(prepared),
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

/// Prepare until done or abandoned. An abandoned preparation is dropped at
/// once, which stops its download and kills its installer's process group;
/// what it left behind is then undone like any failed preparation.
async fn prepare_unless_abandoned(
    inner: &Inner,
    id: &str,
    version: &str,
    mode: UpdateMode,
) -> Result<Prepared, PrepareFailure> {
    unless_abandoned(inner, id, prepare(&inner.context, id, version, mode)).await
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
                    *slot = Slot::Prepared(prepared);
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
                "update {} still holds this CCCC home; if it does not finish shortly, stop CCCC and recover it with `{}`",
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

type PrepareFailure = (Option<Transaction>, UpdateRejection);

/// Fetch the target release's installer, have it stage the release, and check
/// the staged program. Nothing running is stopped and nothing is replaced.
async fn prepare(
    context: &UpdateContext,
    id: &str,
    version: &str,
    mode: UpdateMode,
) -> Result<Prepared, PrepareFailure> {
    let transaction =
        Transaction::create(&context.home, id, &context.install_dir).map_err(|error| {
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
    let staged = context
        .install_dir
        .join(format!("{PROGRAM}.cccc-install.{}", transaction.id));
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
    if !check.relay_schemas.contains(&RELAY_SCHEMA) {
        return Err(fail(
            "update_candidate_rejected",
            format!("CCCC {version} cannot finish an update started by this version"),
        ));
    }
    Ok(Prepared {
        transaction: transaction.clone(),
        version: version.into(),
        mode,
        target: BuildIdentity {
            version: check.version,
            source_id: check.build.source_id,
        },
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
        .env("CCCC_INSTALL_DIR", &context.install_dir)
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

async fn self_check(staged: &Path) -> Result<SelfCheck> {
    let mut command = cccc_runtime::background_tokio_command(staged);
    command
        .args(["update", "--self-check"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(SELF_CHECK_TIMEOUT, command.output())
        .await
        .map_err(|_| anyhow!("the staged program did not answer in time"))?
        .context("could not run the staged program")?;
    if !output.status.success() {
        bail!("the staged program cannot check itself ({})", output.status);
    }
    if output.stdout.len() > SELF_CHECK_MAX_BYTES {
        bail!("the staged program's self-check is too large");
    }
    let check: SelfCheck =
        serde_json::from_slice(&output.stdout).context("unreadable self-check")?;
    if check.schema != SELF_CHECK_SCHEMA {
        bail!("unsupported self-check schema {}", check.schema);
    }
    Ok(check)
}

/// Publish the startup intent right before the stop request. Until it is
/// withdrawn, only this update's relay may start a daemon for this home.
pub(crate) fn arm(context: &UpdateContext, prepared: &Prepared) -> Result<()> {
    let intent = attended_update::Intent::new(
        &context.home,
        &prepared.transaction.id,
        prepared.target.clone(),
        context.install_dir.clone(),
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
) -> Result<()> {
    let record = RelayRecord {
        schema: RELAY_SCHEMA,
        transaction: prepared.transaction.id.clone(),
        from_version: crate::PRODUCT_VERSION.into(),
        to_version: prepared.version.clone(),
        mode: prepared.mode.as_str().into(),
        install_dir: context.install_dir.clone(),
        launch: context.launch.clone(),
        outcome,
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
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) fn announcement(&self) -> String {
        match &self.outcome {
            RelayOutcome::Applied => format!("Starting CCCC {}...", self.prepared.version),
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

/// Write this home's latest attended update result. Paths are fixed.
pub(crate) fn record_result(home: &HomeLayout, record: &RelayRecord, result: &str) {
    let (stage, reason) = match &record.outcome {
        RelayOutcome::Applied => (None, None),
        RelayOutcome::Abandoned { stage, reason } => (Some(stage.clone()), Some(reason.clone())),
    };
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
            agents: AGENTS_NOT_OBSERVED.into(),
            finished_at: cccc_contracts::utc_now(),
        },
    );
}

/// The Web closed for an update that then could not start; nothing stopped.
pub(crate) fn record_not_started(context: &UpdateContext, prepared: &Prepared, reason: &str) {
    write_last(
        &context.home,
        LastResult {
            transaction: prepared.transaction.id.clone(),
            from_version: Some(crate::PRODUCT_VERSION.into()),
            to_version: prepared.version.clone(),
            mode: Some(prepared.mode.as_str().into()),
            result: "not_started".into(),
            stage: Some("arm".into()),
            reason: Some(reason.into()),
            agents: AGENTS_NOT_OBSERVED.into(),
            finished_at: cccc_contracts::utc_now(),
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
/// program, so the previous files can go and ordinary starts may resume. This
/// records the installation only; the agents are not observed here. An error
/// says what is left to do.
pub(crate) async fn confirm(
    home: &HomeLayout,
    transaction: &Transaction,
    record: &RelayRecord,
) -> Result<(), String> {
    let context = UpdateContext::for_installation(home, record.install_dir.clone());
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
        record_result(home, record, "unconfirmed");
        return Err(problem);
    }
    record_result(home, record, "installed");
    transaction.remove();
    cccc_runtime::diagnostic(format_args!(
        "[cccc] Updated CCCC {} to {}",
        record.from_version, record.to_version
    ));
    Ok(())
}

/// What `--resume` hands the host: the transaction to confirm once the daemon
/// is ready and the Web listens on the new program.
pub(crate) struct Resumed {
    pub transaction: Transaction,
    pub record: RelayRecord,
}

/// `cccc update --resume`: start the host again from the recorded launch.
pub(crate) async fn resume(home: HomeLayout, dir: PathBuf) -> Result<crate::web_host::LaunchExit> {
    #[cfg(target_os = "linux")]
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
        RelayOutcome::Applied => {
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
            record_result(&home, &record, "abandoned");
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
/// this home's startup replaced nothing, so only its files are cleaned up.
pub(crate) async fn recover(home: &HomeLayout, dir: &Path) -> Result<()> {
    // The intent first: when it cannot be used, its error says how to get out,
    // even if the transaction directory is gone.
    let active = attended_update::active(home)?;
    let transaction = Transaction::open(home, dir)?;
    let Some(intent) = active.filter(|intent| intent.transaction_id == transaction.id) else {
        return clean_up(home, &transaction).await;
    };
    let context = UpdateContext::for_installation(home, intent.install_dir.clone());
    // Proves that the update host has exited and that no daemon runs for this
    // home while files are restored or kept.
    let _daemon_lock = attended_update::claim_stopped(home, &transaction.id)?;
    if !transaction.script().is_file() {
        bail!(
            "update {} is damaged: its installer {} is missing, so what it changed cannot be told; nothing was changed. \
             After checking {} by hand, remove {} to start CCCC again",
            transaction.id,
            transaction.script().display(),
            intent.install_dir.display(),
            home.root().join("update/active.json").display()
        );
    }
    let target = &intent.target;
    let run = run_phase(&context, &transaction, "abort", None, PHASE_TIMEOUT).await?;
    if run.aborted(&transaction) {
        attended_update::disarm(home, &transaction.id)?;
        record_recovered(home, &transaction, target, "undone");
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
    record_recovered(home, &transaction, target, "kept");
    transaction.remove();
    println!(
        "The update had already replaced the previous program, so CCCC {} stays installed. CCCC is not running; start it with `cccc`.",
        target.version
    );
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
    let context = UpdateContext::for_installation(home, record.install_dir);
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
    result: &str,
) {
    // The relay record exists only when the host got as far as the relay.
    let relay = transaction.read_relay().ok();
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
            agents: AGENTS_NOT_OBSERVED.into(),
            finished_at: cccc_contracts::utc_now(),
        },
    );
}

/// Children of the replaced program survive exec without anyone to wait for
/// them. Reap each one when it exits, without touching children started later.
#[cfg(target_os = "linux")]
pub(crate) fn reap_inherited_children() {
    let me = std::process::id();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return;
    };
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<i32>().ok())
        else {
            continue;
        };
        let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        // The command name may contain spaces or parentheses; fields resume
        // after its last ')': state, then the parent PID.
        let parent = stat
            .rsplit_once(')')
            .and_then(|(_, rest)| rest.split_whitespace().nth(1))
            .and_then(|field| field.parse::<u32>().ok());
        if parent == Some(me) {
            std::thread::spawn(move || {
                let _ = nix::sys::wait::waitpid(nix::unistd::Pid::from_raw(pid), None);
            });
        }
    }
}

#[cfg(test)]
#[path = "update_host_tests.rs"]
mod tests;
