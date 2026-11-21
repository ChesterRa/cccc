use super::admission::Admissions;
use super::backend::Backend;
use super::protocol::{CONTRACT, Failure, MAX_FRAME_BYTES, Mutation, Request, Response};
use anyhow::{Context, Result};
use cccc_core::HomeLayout;
pub(super) use cccc_core::unix_socket_path::SocketPath;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Notify, Semaphore};

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Address {
    pub owner_id: String,
    pub instance: String,
    pub contract: u32,
    pub pid: u32,
    pub socket: PathBuf,
}

pub(super) struct Paths {
    pub owner_id: String,
    pub directory: PathBuf,
    pub socket: PathBuf,
    pub address: PathBuf,
    pub lock: PathBuf,
}

impl Paths {
    pub(super) fn new(home: &HomeLayout, owner_id: &str) -> io::Result<Self> {
        let id = uuid::Uuid::parse_str(owner_id)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid session owner id"))?;
        let directory = home
            .daemon_dir()
            .join("session-owners")
            .join(id.simple().to_string());
        Ok(Self {
            owner_id: id.simple().to_string(),
            socket: directory.join("control.sock"),
            address: directory.join("address.json"),
            lock: directory.join("owner.lock"),
            directory,
        })
    }

    pub(super) fn initialize(&self) -> io::Result<()> {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.directory)?;
        std::fs::set_permissions(&self.directory, std::fs::Permissions::from_mode(0o700))
    }
}

struct Service {
    home: HomeLayout,
    address: Address,
    admissions: Mutex<Admissions>,
    backend: Arc<Backend>,
    stopped: Notify,
    epoch_changed: tokio::sync::watch::Sender<u64>,
}

pub(super) async fn run(
    home: HomeLayout,
    owner_id: &str,
    launcher: Option<Arc<dyn super::browser::BrowserLauncher>>,
) -> Result<()> {
    let paths = Paths::new(&home, owner_id)?;
    paths.initialize()?;
    let lock = cccc_core::fs::try_exclusive_lock(&paths.lock)?
        .context("session owner id is already running")?;
    // Do this in the child, not via process_group(0): a group leader cannot
    // call setsid. No inherited terminal or parent cleanup ledger owns it.
    nix::unistd::setsid().context("detach session owner from the controlling terminal")?;
    let mut owner_guard = OwnerGuard {
        paths,
        _lock: lock,
        retired: false,
    };
    cccc_runtime::protect_owned_process_groups(
        &owner_guard
            .paths
            .directory
            .join("owned-process-groups.json"),
    )?;
    crate::runtime_start_gate::allow(&home).map_err(anyhow::Error::msg)?;
    match std::fs::remove_file(&owner_guard.paths.socket) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let listener = {
        let socket = SocketPath::new(&owner_guard.paths.socket)?;
        UnixListener::bind(&socket.path)?
    };
    std::fs::set_permissions(
        &owner_guard.paths.socket,
        std::fs::Permissions::from_mode(0o600),
    )?;
    let address = Address {
        owner_id: owner_guard.paths.owner_id.clone(),
        instance: uuid::Uuid::new_v4().simple().to_string(),
        contract: CONTRACT,
        pid: std::process::id(),
        socket: owner_guard.paths.socket.clone(),
    };
    let mut backend = Backend::with_owner(
        home.clone(),
        address.owner_id.clone(),
        address.instance.clone(),
    );
    backend.set_browser_launcher(launcher);
    let backend = Arc::new(backend);
    super::install_backend(backend.clone())?;
    let service = Arc::new(Service {
        home,
        admissions: Mutex::new(Admissions::new(address.instance.clone())),
        backend,
        address,
        stopped: Notify::new(),
        epoch_changed: tokio::sync::watch::channel(0).0,
    });
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let mut hangup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;
    cccc_core::fs::write_json(&owner_guard.paths.address, &service.address)?;
    let connections = Arc::new(Semaphore::new(64));
    loop {
        tokio::select! {
            _ = service.stopped.notified() => {
                owner_guard.retired = service.backend.is_empty().unwrap_or(false);
                break;
            },
            _ = terminate.recv() => {
                if stop_for_signal(service.clone()).await { owner_guard.retired = true; break; }
            },
            _ = interrupt.recv() => {
                if stop_for_signal(service.clone()).await { owner_guard.retired = true; break; }
            },
            _ = hangup.recv() => {
                if stop_for_signal(service.clone()).await { owner_guard.retired = true; break; }
            },
            accepted = listener.accept() => {
                let (stream, _) = match accepted {
                    Ok(stream) => stream,
                    Err(error) => {
                        tracing::warn!(kind = ?error.kind(), "session owner accept failed; keeping members and ownership evidence");
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        continue;
                    }
                };
                match same_user(&stream) {
                    Ok(true) => {},
                    Ok(false) => continue,
                    Err(error) => {
                        tracing::warn!(kind = ?error.kind(), "session owner peer identity unavailable");
                        continue;
                    }
                }
                let Ok(permit) = connections.clone().try_acquire_owned() else { continue; };
                let service = service.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    // The frame and operation bodies are deliberately never logged.
                    if let Err(error) = connection(stream, service).await {
                        tracing::debug!(kind = ?error.kind(), "session owner connection ended");
                    }
                });
            }
        }
    }
    Ok(())
}

async fn stop_for_signal(service: Arc<Service>) -> bool {
    let result = tokio::task::spawn_blocking(move || {
        {
            let _admission = service
                .admissions
                .lock()
                .map_err(|_| Failure::new("owner_unavailable", "admission lock poisoned"))?;
            service.backend.validate(&Mutation::StopAll)?;
            service.backend.reserve(&Mutation::StopAll)?;
        }
        service.backend.execute(&service.home, Mutation::StopAll)?;
        service.backend.is_empty()
    })
    .await;
    match result {
        Ok(Ok(true)) => true,
        _ => {
            tracing::warn!(
                "session owner signal stop is unconfirmed; retaining members and discovery evidence for explicit Stop"
            );
            false
        }
    }
}

fn same_user(stream: &UnixStream) -> io::Result<bool> {
    #[cfg(target_os = "linux")]
    {
        // peer_cred uses the OS socket identity, not a caller-provided uid.
        let uid = std::fs::metadata("/proc/self")?;
        use std::os::unix::fs::MetadataExt;
        Ok(stream.peer_cred()?.uid() == uid.uid())
    }
    #[cfg(target_os = "macos")]
    {
        Ok(stream.peer_cred()?.uid() == nix::unistd::geteuid().as_raw())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = stream;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "session owner peer credentials are unavailable on this platform",
        ))
    }
}

async fn connection(stream: UnixStream, service: Arc<Service>) -> io::Result<()> {
    // Capture before the first frame can wait. A PID supplied in JSON is never
    // an identity, and a peer that exits while sending must not name a reused PID.
    let peer = stream
        .peer_cred()
        .and_then(|credentials| {
            credentials
                .pid()
                .and_then(|pid| u32::try_from(pid).ok())
                .ok_or_else(|| io::Error::other("native MCP peer PID is unavailable"))
        })
        .and_then(cccc_core::local_sessions::NativeHost::capture);
    let mut reader = BufReader::new(stream);
    let mut lease = StreamLease {
        service: service.clone(),
        epoch: None,
    };
    loop {
        let mut frame = Vec::new();
        let size = (&mut reader)
            .take(MAX_FRAME_BYTES as u64 + 1)
            .read_until(b'\n', &mut frame)
            .await?;
        if size == 0 {
            return Ok(());
        }
        if size > MAX_FRAME_BYTES || frame.last() != Some(&b'\n') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "session owner frame too large or incomplete",
            ));
        }
        let request: Request = serde_json::from_slice(&frame).map_err(io::Error::other)?;
        if let Request::TerminalStream {
            actor,
            epoch,
            request,
        } = request
        {
            let valid_request = request.op == "term_attach"
                && request
                    .args
                    .get("group_id")
                    .and_then(serde_json::Value::as_str)
                    == Some(actor.group_id.as_str())
                && request
                    .args
                    .get("actor_id")
                    .and_then(serde_json::Value::as_str)
                    == Some(actor.actor_id.as_str());
            if !valid_request {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "invalid terminal stream identity",
                ));
            }
            {
                let admissions = service.admissions.lock().map_err(lock_error)?;
                admissions
                    .require_current(epoch)
                    .map_err(io::Error::other)?;
                service
                    .backend
                    .validate(&Mutation::EnsureViewer { actor })
                    .map_err(io::Error::other)?;
            }
            let mut changed = service.epoch_changed.subscribe();
            let (shutdown, receiver) = tokio::sync::watch::channel(false);
            let watcher = tokio::spawn(async move {
                loop {
                    let current = *changed.borrow_and_update();
                    if current != epoch {
                        break;
                    }
                    if changed.changed().await.is_err() {
                        break;
                    }
                }
                shutdown.send_replace(true);
            });
            let guard_service = service.clone();
            let guard: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(move || {
                guard_service
                    .admissions
                    .lock()
                    .is_ok_and(|admissions| admissions.require_current(epoch).is_ok())
            });
            let result = crate::server_terminal_attach::handle_guarded(
                reader,
                service.home.clone(),
                request,
                receiver,
                Some(guard),
            )
            .await;
            watcher.abort();
            return result.map_err(io::Error::other);
        }
        let response = match request {
            Request::BrowserResources => match service.backend.browsers.list() {
                Ok(resources) => Response::BrowserResources { resources },
                Err(error) => Response::Failed { error },
            },
            Request::McpRegister { report } => match &peer {
                Ok(native) if native.alive() => {
                    let instance = report.instance.clone();
                    let launch_id = report.launch_id.clone();
                    match service.backend.register_mcp(native.clone(), report) {
                        Ok(()) => Response::McpRegistered {
                            registration: cccc_core::owned_mcp::REGISTRATION_API,
                            instance,
                            launch_id,
                        },
                        Err(error) => Response::Failed { error },
                    }
                }
                _ => failed(
                    "mcp_peer_unconfirmed",
                    "native MCP peer identity is unavailable or changed; restart the member",
                ),
            },
            Request::Hello { contract } => {
                if contract != CONTRACT {
                    failed(
                        "incompatible_contract",
                        "session owner contract is incompatible",
                    )
                } else {
                    Response::Hello {
                        contract: CONTRACT,
                        instance: service.address.instance.clone(),
                        pid: service.address.pid,
                        build: cccc_core::build_info::current(),
                        accepting_starts: service.backend.accepting_starts(),
                        mcp_registration: cccc_core::owned_mcp::REGISTRATION_API,
                    }
                }
            }
            Request::Acquire { contract, instance } => {
                if contract != CONTRACT || instance != service.address.instance {
                    failed("incompatible_owner", "owner instance or contract changed")
                } else if lease.epoch.is_some() {
                    failed(
                        "invalid_stream",
                        "a control stream acquires exactly one epoch",
                    )
                } else {
                    let acquired = {
                        let mut admissions = service.admissions.lock().map_err(lock_error)?;
                        admissions.acquire().and_then(|epoch| {
                            service.backend.control_acquired(epoch)?;
                            Ok(epoch)
                        })
                    };
                    match acquired {
                        Ok(epoch) => {
                            lease.epoch = Some(epoch);
                            service.epoch_changed.send_replace(epoch);
                            Response::Acquired { epoch }
                        }
                        Err(error) => Response::Failed { error },
                    }
                }
            }
            Request::Mutate {
                epoch,
                sequence,
                mutation,
            } => {
                if let Mutation::Recovery { action } = *mutation {
                    let admitted = {
                        let mut admissions = service.admissions.lock().map_err(lock_error)?;
                        if lease.epoch != Some(epoch) {
                            Err(Failure::new(
                                "invalid_stream",
                                "control stream does not own this epoch",
                            ))
                        } else {
                            admissions.administrative(epoch, sequence).map(|_| ())
                        }
                    };
                    match admitted {
                        Err(error) => Response::Failed { error },
                        Ok(()) => {
                            // Guard/filesystem work runs outside admission and
                            // off-worker. Lost replies require another listing.
                            let service = service.clone();
                            match tokio::task::spawn_blocking(move || {
                                service.backend.recover(
                                    &service.home,
                                    &service.address.owner_id,
                                    action,
                                )
                            })
                            .await
                            {
                                Ok(Ok(value)) => Response::Recovery { value },
                                Ok(Err(error)) => Response::Failed { error },
                                Err(_) => failed(
                                    "owner_error",
                                    "recovery failed; list the evidence again",
                                ),
                            }
                        }
                    }
                } else if lease.epoch != Some(epoch) {
                    failed("invalid_stream", "mutation does not belong to this stream")
                } else {
                    // Validate and register under the fencing lock. A decoded
                    // request cannot cross a concurrent takeover unnoticed.
                    let mut admissions = service.admissions.lock().map_err(lock_error)?;
                    let admitted = admissions
                        .require_current(epoch)
                        .and_then(|()| service.backend.validate(&mutation))
                        .and_then(|()| match &*mutation {
                            Mutation::Acknowledge { .. } => {
                                admissions.administrative(epoch, sequence)
                            }
                            Mutation::Retire => admissions
                                .require_retirable(service.backend.accepting_starts())
                                .and_then(|()| admissions.administrative(epoch, sequence)),
                            Mutation::StopAll => {
                                admissions.admit_stop(epoch, sequence, frame.len(), true)
                            }
                            Mutation::Recovery { .. } => admissions.administrative(epoch, sequence),
                            Mutation::Stop { .. } | Mutation::BrowserClose { .. } => {
                                admissions.admit_stop(epoch, sequence, frame.len(), false)
                            }
                            _ => admissions.admit(
                                epoch,
                                sequence,
                                mutation.source().cloned(),
                                frame.len(),
                            ),
                        });
                    match admitted {
                        Err(error) => Response::Failed { error },
                        Ok(operation) => {
                            if let Err(error) = service.backend.reserve(&mutation) {
                                admissions.finish(&operation, Err(error));
                                Response::Admitted { operation }
                            } else if let Mutation::Acknowledge {
                                operation: completed,
                            } = *mutation
                            {
                                let result = admissions
                                    .acknowledge(&completed)
                                    .map(|()| serde_json::Value::Null);
                                let response = match &result {
                                    Ok(_) => Response::Acknowledged,
                                    Err(error) => Response::Failed {
                                        error: error.clone(),
                                    },
                                };
                                admissions.finish(&operation, result);
                                // This command itself has no delivery to settle.
                                let _ = admissions.acknowledge(&operation);
                                response
                            } else if matches!(*mutation, Mutation::Retire) {
                                let result =
                                    service.backend.retire().map(|()| serde_json::Value::Null);
                                let response = match &result {
                                    Ok(_) => Response::Retired,
                                    Err(error) => Response::Failed {
                                        error: error.clone(),
                                    },
                                };
                                admissions.finish(&operation, result);
                                let _ = admissions.acknowledge(&operation);
                                response
                            } else {
                                let service = service.clone();
                                let task_operation = operation.clone();
                                tokio::task::spawn_blocking(move || {
                                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                        service.backend.execute_in_epoch(&service.home, *mutation, task_operation.epoch)
                                    })).unwrap_or_else(|_| Err(Failure::new("owner_execution_failed", "execution task failed; its input outcome is not confirmed")));
                                    if let Ok(mut admissions) = service.admissions.lock() {
                                        admissions.finish(&task_operation, result);
                                    }
                                });
                                Response::Admitted { operation }
                            }
                        }
                    }
                }
            }
            Request::Snapshot => {
                let backend = service.backend.clone();
                match tokio::task::spawn_blocking(move || backend.owner_snapshot()).await {
                    Ok(Ok(snapshot)) => Response::Snapshot { snapshot },
                    Ok(Err(error)) => Response::Failed { error },
                    Err(_) => failed("owner_error", "member observation failed"),
                }
            }
            Request::RecoveryList => {
                let service = service.clone();
                match tokio::task::spawn_blocking(move || {
                    service.backend.recover(
                        &service.home,
                        &service.address.owner_id,
                        super::RecoveryAction::List,
                    )
                })
                .await
                {
                    Ok(Ok(value)) => Response::Recovery { value },
                    Ok(Err(error)) => Response::Failed { error },
                    Err(_) => failed("owner_error", "recovery observation failed"),
                }
            }
            Request::Query { operation } => Response::Operation {
                state: service
                    .admissions
                    .lock()
                    .map_err(lock_error)?
                    .query(&operation),
            },
            Request::Find { source } => Response::Found {
                operations: service.admissions.lock().map_err(lock_error)?.find(&source),
            },
            Request::FindEvent {
                group_id,
                actor_id,
                actor_created_at,
                event_id,
            } => Response::FoundEvent {
                operations: service.admissions.lock().map_err(lock_error)?.find_event(
                    &group_id,
                    &actor_id,
                    &actor_created_at,
                    &event_id,
                ),
            },
            Request::TerminalRead { actor, read } => {
                let backend = service.backend.clone();
                match tokio::task::spawn_blocking(move || backend.read_terminal(&actor, read)).await
                {
                    Ok(Ok(value)) => Response::Terminal { value },
                    Ok(Err(error)) => Response::Failed { error },
                    Err(_) => failed("owner_error", "terminal observation failed"),
                }
            }
            Request::TerminalStream { .. } => unreachable!("terminal stream handled above"),
        };
        let retired = matches!(response, Response::Retired);
        write_one(reader.get_mut(), &response).await?;
        if retired {
            service.stopped.notify_one();
            return Ok(());
        }
    }
}

async fn write_one(stream: &mut UnixStream, response: &Response) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(&response).map_err(io::Error::other)?;
    if bytes.len() >= MAX_FRAME_BYTES {
        return Err(io::Error::other(
            "session owner response exceeds frame limit",
        ));
    }
    bytes.push(b'\n');
    stream.write_all(&bytes).await?;
    stream.flush().await
}

struct StreamLease {
    service: Arc<Service>,
    epoch: Option<u64>,
}

impl Drop for StreamLease {
    fn drop(&mut self) {
        if let Some(epoch) = self.epoch
            && let Ok(mut admissions) = self.service.admissions.lock()
            && admissions.close(epoch)
        {
            self.service.epoch_changed.send_replace(0);
        }
    }
}

struct OwnerGuard {
    paths: Paths,
    _lock: File,
    retired: bool,
}

impl Drop for OwnerGuard {
    fn drop(&mut self) {
        if !self.retired {
            return;
        }
        let _ = std::fs::remove_file(&self.paths.address);
        let _ = std::fs::remove_file(&self.paths.socket);
    }
}

fn failed(code: &str, message: &str) -> Response {
    Response::Failed {
        error: Failure::new(code, message),
    }
}
fn lock_error<T>(_: std::sync::PoisonError<T>) -> io::Error {
    io::Error::other("session owner admission lock poisoned")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mcp_registration_uses_the_native_peer_while_start_and_control_are_pending() {
        use super::super::protocol::{ActorRef, PreparedLaunch};
        use cccc_core::owned_mcp::{REGISTRATION_API, Registration};
        use std::collections::BTreeMap;
        use std::time::Duration;

        let temp = tempfile::tempdir().expect("isolated owner home");
        let home = HomeLayout::from_path(temp.path().join("long-home-".repeat(9))).expect("home");
        home.initialize().expect("initialize home");
        let paths =
            Paths::new(&home, &uuid::Uuid::new_v4().simple().to_string()).expect("owner paths");
        paths.initialize().expect("initialize owner directory");
        let address = Address {
            owner_id: paths.owner_id,
            instance: uuid::Uuid::new_v4().simple().to_string(),
            contract: CONTRACT,
            pid: std::process::id(),
            socket: paths.socket,
        };
        let listener = {
            let socket = SocketPath::new(&address.socket).expect("short socket alias");
            UnixListener::bind(&socket.path).expect("bind owner endpoint")
        };
        let backend = Arc::new(Backend::with_owner(
            home.clone(),
            address.owner_id.clone(),
            address.instance.clone(),
        ));
        let actor = cccc_contracts::Actor::new("peer");
        let group: cccc_core::GroupDoc = serde_json::from_value(serde_json::json!({"v":1,"group_id":"g_mcp_wire","title":"wire fixture","created_at":"created","updated_at":"created"})).expect("group fixture");
        let identity = ActorRef {
            group_id: group.group_id.clone(),
            actor_id: actor.id.clone(),
            actor_created_at: actor.created_at.clone(),
            generation: None,
        };
        let start = Mutation::Start {
            actor: identity.clone(),
            launch: Box::new(PreparedLaunch::Managed {
                group,
                actor,
                host_environment: BTreeMap::new(),
            }),
        };
        backend.validate(&start).expect("validate launch");
        backend
            .reserve(&start)
            .expect("reserve launch without executing provider");
        let binding = backend
            .mcp_binding(&identity.group_id, &identity.actor_id)
            .expect("actual owner's nonce");
        assert_eq!(
            backend
                .validate(&start)
                .expect_err("pending Start cannot replace the binding")
                .code,
            "actor_owned"
        );
        assert_eq!(
            backend.mcp_binding(&identity.group_id, &identity.actor_id),
            Some(binding.clone())
        );
        let service = Arc::new(Service {
            home: home.clone(),
            admissions: Mutex::new(Admissions::new(address.instance.clone())),
            address,
            backend: backend.clone(),
            stopped: Notify::new(),
            epoch_changed: tokio::sync::watch::channel(0).0,
        });

        // A control/Start path can hold admission while waiting for provider
        // initialize. Registration must finish without waiting on that mutex.
        let locked_service = service.clone();
        let (locked, locked_rx) = std::sync::mpsc::channel();
        let (release, release_rx) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || {
            let _guard = locked_service
                .admissions
                .lock()
                .expect("hold control admission");
            locked.send(()).expect("publish held lock");
            let _ = release_rx.recv_timeout(Duration::from_secs(5));
        });
        locked_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("admission is held");
        let connection_service = service.clone();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept MCP peer");
            assert!(same_user(&stream).expect("kernel peer user"));
            connection(stream, connection_service).await
        });
        let report = Registration {
            registration: REGISTRATION_API,
            instance: binding.instance.clone(),
            launch_id: binding.launch_id.clone(),
            version: "0.4.58".into(),
            source_id: "a".repeat(64),
            mcp_contract: cccc_contracts::MCP_DAEMON_CONTRACT,
        };
        let registered = tokio::time::timeout(
            Duration::from_secs(2),
            cccc_client::owned_mcp::register(&home, &binding, &report),
        )
        .await;
        let _ = release.send(());
        holder.join().expect("release control admission");
        registered
            .expect("registration must not wait for control/Start")
            .expect("registration succeeds without a daemon");
        server
            .await
            .expect("registration connection task")
            .expect("registration connection");
        assert!(!home.daemon_dir().join("ccccd.sock").exists());
        let snapshot = backend
            .snapshots()
            .expect("retained owner observations")
            .pop()
            .expect("pending member snapshot")
            .mcp
            .expect("MCP snapshot");
        assert!(snapshot.initialized);
        assert_eq!(snapshot.processes.len(), 1);
        assert_eq!(
            snapshot.processes[0].native,
            cccc_core::local_sessions::NativeHost::capture(std::process::id())
                .expect("actual socket peer identity")
        );
        assert_eq!(snapshot.processes[0].report, report);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[tokio::test]
    async fn long_owner_paths_support_control_and_terminal_connections() {
        use std::io::Write;
        use std::os::unix::ffi::OsStrExt;

        let temp = tempfile::tempdir().expect("isolated test directory");
        let home = HomeLayout::from_path(temp.path().join("long-home-".repeat(8)))
            .expect("resolve long home");
        let paths = Paths::new(&home, &uuid::Uuid::new_v4().simple().to_string())
            .expect("resolve owner paths");
        paths.initialize().expect("initialize owner directory");
        assert!(paths.socket.as_os_str().as_bytes().len() > 108);
        let listener = {
            let socket = SocketPath::new(&paths.socket).expect("short bind address");
            UnixListener::bind(&socket.path).expect("bind long owner socket")
        };
        std::fs::set_permissions(&paths.socket, std::fs::Permissions::from_mode(0o600))
            .expect("restrict owner socket");

        let mut control = {
            let socket = SocketPath::new(&paths.socket).expect("short control address");
            std::os::unix::net::UnixStream::connect(&socket.path).expect("connect control socket")
        };
        control.write_all(b"control").expect("write control frame");
        let (mut peer, _) = listener.accept().await.expect("accept control connection");
        let mut frame = [0; 7];
        peer.read_exact(&mut frame)
            .await
            .expect("read control frame");
        assert_eq!(&frame, b"control");

        let mut terminal = {
            let socket = SocketPath::new(&paths.socket).expect("short terminal address");
            UnixStream::connect(&socket.path)
                .await
                .expect("connect terminal socket")
        };
        terminal
            .write_all(b"terminal")
            .await
            .expect("write terminal frame");
        let (mut peer, _) = listener.accept().await.expect("accept terminal connection");
        let mut frame = [0; 8];
        peer.read_exact(&mut frame)
            .await
            .expect("read terminal frame");
        assert_eq!(&frame, b"terminal");
        assert_eq!(
            std::fs::metadata(&paths.socket)
                .expect("socket metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(&paths.directory)
                .expect("directory metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        drop(listener);
        std::fs::remove_file(&paths.socket).expect("remove actual socket path");
        assert!(!paths.socket.exists());
    }

    #[test]
    fn unexpected_owner_exit_retains_its_discovery_evidence() {
        let temp = tempfile::tempdir().expect("isolated test directory");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("resolve isolated home");
        let paths =
            Paths::new(&home, &uuid::Uuid::new_v4().to_string()).expect("resolve owner paths");
        paths.initialize().expect("initialize fixture state");
        let address = paths.address.clone();
        std::fs::write(&address, b"unconfirmed owner identity").expect("write fixture file");
        let lock = File::create(&paths.lock).expect("create owner lock file");
        drop(OwnerGuard {
            paths,
            _lock: lock,
            retired: false,
        });
        assert!(address.exists());
    }

    #[test]
    fn confirmed_owner_retirement_clears_its_discovery_evidence() {
        let temp = tempfile::tempdir().expect("isolated test directory");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("resolve isolated home");
        let paths =
            Paths::new(&home, &uuid::Uuid::new_v4().to_string()).expect("resolve owner paths");
        paths.initialize().expect("initialize fixture state");
        let address = paths.address.clone();
        std::fs::write(&address, b"retired owner identity").expect("write fixture file");
        let lock = File::create(&paths.lock).expect("create owner lock file");
        drop(OwnerGuard {
            paths,
            _lock: lock,
            retired: true,
        });
        assert!(!address.exists());
    }
}
