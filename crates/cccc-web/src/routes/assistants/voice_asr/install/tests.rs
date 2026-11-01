use super::*;
use crate::routes::assistants::voice_asr::payload::{
    Pinned, current_version, current_version_with, pin_count, snapshot, valid_payload_dir,
};
use crate::routes::assistants::voice_asr::status_from;
use std::collections::HashMap;
use std::sync::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const MODEL: &str = "test_model";
const UNUSED_URL: &str = "http://127.0.0.1:9/unused";

struct Fixture {
    _temp: tempfile::TempDir,
    home: HomeLayout,
    root: PathBuf,
}

fn fixture() -> Fixture {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let root = model_dir(&home, MODEL);
    Fixture {
        _temp: temp,
        home,
        root,
    }
}

impl Fixture {
    /// Lists the test model with one plain file, `model.bin`, expected to hold `expected`.
    fn list_model(&self, url: &str, expected: &[u8]) {
        let mut artifact = plain("", "model.bin", expected);
        artifact["url"] = json!(url);
        self.list_artifacts(vec![artifact], &["model.bin"]);
    }

    fn list_artifacts(&self, artifacts: Vec<Value>, required: &[&str]) {
        let manifest = json!({"voice_secretary_asr_models":[{
            "model_id":MODEL,"kind":"asr","required_files":required,"artifacts":artifacts
        }]});
        cccc_core::fs::write_json(
            &self.home.root().join("config/voice-models.json"),
            &manifest,
        )
        .expect("manifest");
    }

    fn model(&self) -> Model {
        catalog(&self.home)
            .expect("catalog")
            .remove(MODEL)
            .expect("model")
    }

    fn status(&self) -> Value {
        model_status(&self.home, &self.model())
    }

    async fn run_install(&self) -> Value {
        install(Arc::new(start(&self.home, MODEL).expect("start install"))).await;
        self.status()
    }

    async fn install_dir_after_install(&self) -> PathBuf {
        let status = self.run_install().await;
        assert_eq!(status["status"], "ready", "{status}");
        PathBuf::from(status["install_dir"].as_str().expect("install dir"))
    }

    fn state(&self) -> Value {
        serde_json::from_slice(&fs::read(self.root.join(INSTALL_STATE)).expect("state"))
            .expect("state JSON")
    }

    fn write_state(&self, state: Value) {
        cccc_core::fs::write_json(&self.root.join(INSTALL_STATE), &state).expect("write state");
    }

    fn ready_at(&self, payload_dir: &str) {
        self.write_state(json!({"status":"ready","payload_dir":payload_dir,"installed_at":"t"}));
    }

    /// A version directory holding the test model's required file.
    fn version(&self, name: &str) -> PathBuf {
        let dir = self.root.join(name);
        fs::create_dir_all(&dir).expect("version dir");
        fs::write(dir.join("model.bin"), b"model").expect("model file");
        dir
    }

    fn entries(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&self.root)
            .expect("model dir")
            .map(|entry| {
                entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }
}

fn plain(base: &str, path: &str, expected: &[u8]) -> Value {
    json!({
        "path":path,"url":format!("{base}/{path}"),"sha256":sha(expected),
        "size_bytes":expected.len(),"archive":""
    })
}

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn name(path: &Path) -> String {
    path.file_name()
        .expect("file name")
        .to_string_lossy()
        .into_owned()
}

fn tar_with(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (path, bytes) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, path, *bytes)
            .expect("tar entry");
    }
    builder.into_inner().expect("tar")
}

async fn wait_until(mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "timed out");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[derive(Clone)]
enum Reply {
    Body(Vec<u8>),
    Status(u16),
    /// Sends the start of a longer body, then nothing more.
    Stall(Vec<u8>),
}

/// Serves fixed replies by path on a local port and returns its base URL.
async fn serve(routes: Vec<(&str, Reply)>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let base = format!("http://{}", listener.local_addr().expect("address"));
    let routes: HashMap<String, Reply> = routes
        .into_iter()
        .map(|(path, reply)| (path.to_owned(), reply))
        .collect();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let routes = routes.clone();
            tokio::spawn(async move {
                let mut request = Vec::new();
                let mut buffer = [0_u8; 1024];
                while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                    match socket.read(&mut buffer).await {
                        Ok(0) | Err(_) => return,
                        Ok(read) => request.extend_from_slice(&buffer[..read]),
                    }
                }
                let request = String::from_utf8_lossy(&request).into_owned();
                let path = request.split_whitespace().nth(1).unwrap_or("");
                let reply = routes.get(path).cloned().unwrap_or(Reply::Status(404));
                let (status, body, length) = match &reply {
                    Reply::Body(body) => (200, body.clone(), body.len()),
                    Reply::Status(status) => (*status, Vec::new(), 0),
                    Reply::Stall(start) => (200, start.clone(), start.len() + 1_000_000),
                };
                let head = format!(
                    "HTTP/1.1 {status} Fixture\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n"
                );
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(&body).await;
                if matches!(reply, Reply::Stall(_)) {
                    std::future::pending::<()>().await;
                }
            });
        }
    });
    base
}

#[tokio::test]
async fn an_update_that_fails_verification_keeps_the_installed_version() {
    let f = fixture();
    let base = serve(vec![
        ("/a.bin", Reply::Body(b"first".to_vec())),
        ("/b.bin", Reply::Body(b"second".to_vec())),
    ])
    .await;
    let artifacts = |b: &[u8]| vec![plain(&base, "a.bin", b"first"), plain(&base, "b.bin", b)];
    f.list_artifacts(artifacts(b"second"), &["a.bin", "b.bin"]);
    let installed = f.run_install().await;
    assert_eq!(installed["status"], "ready", "{installed}");
    let dir = PathBuf::from(installed["install_dir"].as_str().expect("install dir"));
    // The update expects a new second file after the first has downloaded and been recorded,
    // but the server still serves the old one.
    f.list_artifacts(artifacts(b"second, updated"), &["a.bin", "b.bin"]);
    let status = f.run_install().await;
    assert_eq!(status["status"], "ready", "{status}");
    assert_eq!(status["installed_at"], installed["installed_at"]);
    assert_eq!(
        status["installed_manifest_sha256"],
        installed["installed_manifest_sha256"]
    );
    assert_eq!(status["update_available"], true);
    assert_eq!(
        status["last_update_error"]["code"],
        "voice_model_hash_mismatch"
    );
    assert_eq!(status["install_dir"], installed["install_dir"]);
    assert_eq!(fs::read(dir.join("b.bin")).expect("old file"), b"second");
    assert_eq!(f.entries(), [INSTALL_STATE, name(&dir).as_str()]);
}

#[tokio::test]
async fn updates_the_server_refuses_keep_the_installed_version_and_leave_nothing() {
    let f = fixture();
    let base = serve(vec![
        ("/model.bin", Reply::Body(b"model".to_vec())),
        ("/refused.bin", Reply::Status(503)),
    ])
    .await;
    f.list_model(&format!("{base}/model.bin"), b"model");
    let installed = f.run_install().await;
    let entries = f.entries();
    f.list_model(&format!("{base}/refused.bin"), b"newer");
    for attempt in 0..3 {
        let status = f.run_install().await;
        assert_eq!(status["status"], "ready", "attempt {attempt}: {status}");
        assert_eq!(status["installed_at"], installed["installed_at"]);
        assert_eq!(
            status["last_update_error"]["code"],
            "voice_model_download_failed"
        );
        assert_eq!(f.entries(), entries, "attempt {attempt}");
    }
}

#[tokio::test]
async fn a_first_install_the_server_refuses_fails() {
    let f = fixture();
    let base = serve(vec![("/refused.bin", Reply::Status(503))]).await;
    f.list_model(&format!("{base}/refused.bin"), b"model");
    let status = f.run_install().await;
    assert_eq!(status["status"], "failed", "{status}");
    assert_eq!(status["installed"], false);
    assert_eq!(status["error"]["code"], "voice_model_download_failed");
    assert_eq!(f.entries(), [INSTALL_STATE]);
}

#[tokio::test]
async fn an_interrupted_update_keeps_the_installed_version_ready() {
    let f = fixture();
    let base = serve(vec![
        ("/a.bin", Reply::Body(b"first".to_vec())),
        ("/b.bin", Reply::Body(b"second".to_vec())),
        ("/stalled.bin", Reply::Stall(b"sec".to_vec())),
    ])
    .await;
    f.list_artifacts(
        vec![
            plain(&base, "a.bin", b"first"),
            plain(&base, "b.bin", b"second"),
        ],
        &["a.bin", "b.bin"],
    );
    let installed = f.run_install().await;
    let dir = PathBuf::from(installed["install_dir"].as_str().expect("install dir"));
    let mut stalled = plain(&base, "b.bin", b"second, updated");
    stalled["url"] = json!(format!("{base}/stalled.bin"));
    f.list_artifacts(
        vec![plain(&base, "a.bin", b"first"), stalled],
        &["a.bin", "b.bin"],
    );
    let task = tokio::spawn(install(Arc::new(
        start(&f.home, MODEL).expect("start update"),
    )));
    // The first file is recorded; the second starts and then stops arriving.
    wait_until(|| f.state()["artifact_index"] == 1).await;
    task.abort();
    let _ = task.await;
    let lock = install_lock_path(&f.home, MODEL);
    wait_until(|| probe_install_lock(&lock) == LockProbe::Free).await;
    // What a crash at this point leaves: still downloading, still naming the installed version.
    let state = f.state();
    assert_eq!(state["status"], "downloading");
    assert_eq!(state["installed_at"], installed["installed_at"]);
    assert_eq!(state["payload_dir"], name(&dir));
    let status = f.status();
    assert_eq!(status["status"], "ready", "{status}");
    assert_eq!(status["install_dir"], installed["install_dir"]);
    collect_all_garbage(&f.home);
    assert_eq!(f.entries(), [INSTALL_STATE, name(&dir).as_str()]);
}

#[tokio::test]
async fn updating_the_legacy_layout_moves_the_model_into_a_version_directory() {
    let f = fixture();
    let archive = tar_with(&[("model.bin", b"unpacked")]);
    let base = serve(vec![("/model.tar", Reply::Body(archive.clone()))]).await;
    f.list_artifacts(
        vec![json!({
            "path":"model.tar","url":format!("{base}/model.tar"),"sha256":sha(&archive),
            "size_bytes":archive.len(),"archive":"tar"
        })],
        &["model.bin"],
    );
    // What older versions left: the model's files and archive in the model directory itself,
    // an interrupted replacement, and the old lock marker.
    fs::create_dir_all(f.root.join(".install-previous-x")).expect("previous");
    fs::write(f.root.join("model.bin"), b"legacy").expect("legacy file");
    fs::write(f.root.join("model.tar"), b"legacy archive").expect("legacy archive");
    fs::write(f.root.join(INSTALL_LOCK), b"1\n").expect("marker");
    f.write_state(json!({
        "model_id":MODEL,"status":"ready","installed_at":"2026-01-01T00:00:00Z",
        "manifest_sha256":"legacy"
    }));
    assert_eq!(f.status()["install_dir"], json!(f.root));
    let begun = begin_install(f.home.clone(), MODEL.into())
        .await
        .expect("begin install");
    assert_eq!(begun["status"], "downloading", "{begun}");
    let lock = install_lock_path(&f.home, MODEL);
    wait_until(|| probe_install_lock(&lock) == LockProbe::Free).await;
    let status = f.status();
    assert_eq!(status["status"], "ready", "{status}");
    let dir = PathBuf::from(status["install_dir"].as_str().expect("install dir"));
    assert_eq!(dir.parent(), Some(f.root.as_path()));
    assert_eq!(fs::read(dir.join("model.bin")).expect("model"), b"unpacked");
    assert!(
        !dir.join("model.tar").exists(),
        "the archive goes once unpacked"
    );
    assert_eq!(
        f.entries(),
        [INSTALL_LOCK, INSTALL_STATE, name(&dir).as_str()]
    );
}

#[tokio::test]
async fn an_archive_the_model_requires_stays_after_unpacking() {
    let archive = tar_with(&[("model.bin", b"unpacked")]);
    let base = serve(vec![("/model.tar", Reply::Body(archive.clone()))]).await;
    // The required file may spell the archive's path in any form that resolves to it.
    let mut spellings = vec![
        "models/model.tar",
        "models//model.tar",
        "models/./model.tar",
    ];
    if cfg!(windows) {
        spellings.extend(["models/Model.tar", "models/model.tar."]);
    }
    for spelling in spellings {
        let f = fixture();
        f.list_artifacts(
            vec![json!({
                "path":"models/model.tar","url":format!("{base}/model.tar"),
                "sha256":sha(&archive),"size_bytes":archive.len(),"archive":"tar"
            })],
            &["model.bin", spelling],
        );
        let dir = f.install_dir_after_install().await;
        assert_eq!(fs::read(dir.join("model.bin")).expect("model"), b"unpacked");
        assert_eq!(
            fs::read(dir.join("models/model.tar")).expect(spelling),
            archive
        );
    }
}

#[tokio::test]
async fn an_update_removes_the_version_it_replaced_unless_a_reader_holds_it() {
    let f = fixture();
    let base = serve(vec![
        ("/1", Reply::Body(b"one".to_vec())),
        ("/2", Reply::Body(b"two".to_vec())),
        ("/3", Reply::Body(b"three".to_vec())),
    ])
    .await;
    f.list_model(&format!("{base}/1"), b"one");
    let first = f.install_dir_after_install().await;
    f.list_model(&format!("{base}/2"), b"two");
    let second = f.install_dir_after_install().await;
    assert!(!first.exists());
    let reader = current_version(&f.home, MODEL).expect("reader");
    assert_eq!(reader.snapshot.root.as_deref(), Some(second.as_path()));
    f.list_model(&format!("{base}/3"), b"three");
    let third = f.install_dir_after_install().await;
    assert_eq!(fs::read(second.join("model.bin")).expect("held"), b"two");
    drop(reader);
    // A released version waits for the next cleanup.
    assert!(second.exists());
    collect_all_garbage(&f.home);
    assert!(!second.exists());
    assert_eq!(f.entries(), [INSTALL_STATE, name(&third).as_str()]);
}

/// On Windows a file a native engine holds open cannot be deleted; the switch must not depend on
/// deleting the version it replaced.
#[cfg(windows)]
#[tokio::test]
async fn an_update_completes_while_the_replaced_version_is_held_open() {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_READ: u32 = 0x1;
    let f = fixture();
    let base = serve(vec![
        ("/1", Reply::Body(b"one".to_vec())),
        ("/2", Reply::Body(b"two".to_vec())),
    ])
    .await;
    f.list_model(&format!("{base}/1"), b"one");
    let first = f.install_dir_after_install().await;
    // Shared for reading only, as the C runtime opens model files.
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(first.join("model.bin"))
        .expect("hold");
    f.list_model(&format!("{base}/2"), b"two");
    let second = f.install_dir_after_install().await;
    assert_ne!(first, second);
    assert!(first.join("model.bin").exists());
    drop(held);
    collect_all_garbage(&f.home);
    assert!(!first.exists());
    assert_eq!(f.entries(), [INSTALL_STATE, name(&second).as_str()]);
}

#[test]
fn a_reader_that_pins_a_replaced_version_moves_to_the_current_one() {
    let f = fixture();
    f.list_model(UNUSED_URL, b"model");
    let old = f.version("payload-old");
    f.ready_at("payload-old");
    let new = f.version("payload-new");
    let mut first_read = true;
    let version = current_version_with(|| {
        let seen = snapshot(&f.home, MODEL);
        if std::mem::take(&mut first_read) {
            // Between this read and the reader's pin, an install switches versions and its
            // cleanup removes the old one, which no reader holds yet.
            let _lock = InstallLock::acquire(&install_lock_path(&f.home, MODEL)).expect("lock");
            f.ready_at("payload-new");
            collect_garbage(&f.root, &f.model(), &payload::read_state(&f.root));
        }
        seen
    })
    .expect("current version");
    assert_eq!(version.snapshot.root.as_deref(), Some(new.as_path()));
    assert!(!old.exists());
    assert_eq!(pin_count(&old), 0);
    assert_eq!(pin_count(&new), 1);
}

#[test]
fn a_pinned_value_is_released_before_its_pin() {
    struct Probe {
        root: PathBuf,
        pins_at_drop: Arc<Mutex<Option<usize>>>,
    }
    impl Drop for Probe {
        fn drop(&mut self) {
            *self.pins_at_drop.lock().expect("probe") = Some(pin_count(&self.root));
        }
    }
    let f = fixture();
    f.list_model(UNUSED_URL, b"model");
    let dir = f.version("payload-a");
    f.ready_at("payload-a");
    let pin = current_version(&f.home, MODEL).expect("version").pin;
    let pins_at_drop = Arc::new(Mutex::new(None));
    let probe = Probe {
        root: dir.clone(),
        pins_at_drop: Arc::clone(&pins_at_drop),
    };
    drop(Pinned::new(probe, pin.expect("pin")));
    assert_eq!(*pins_at_drop.lock().expect("probe"), Some(1));
    assert_eq!(pin_count(&dir), 0);
}

#[tokio::test]
async fn pins_dropped_by_finished_or_cancelled_tasks_are_released() {
    let f = fixture();
    f.list_model(UNUSED_URL, b"model");
    let dir = f.version("payload-a");
    f.ready_at("payload-a");
    // As a capture whose socket closes normally, ending its task.
    let finished = current_version(&f.home, MODEL).expect("version");
    tokio::spawn(async move { drop(finished) })
        .await
        .expect("task");
    assert_eq!(pin_count(&dir), 0);
    // As a capture whose task is cancelled while it waits.
    let cancelled = current_version(&f.home, MODEL).expect("version");
    let task = tokio::spawn(async move {
        let _held = cancelled;
        std::future::pending::<()>().await;
    });
    tokio::task::yield_now().await;
    assert_eq!(pin_count(&dir), 1);
    task.abort();
    assert!(task.await.expect_err("cancelled").is_cancelled());
    assert_eq!(pin_count(&dir), 0);
}

#[test]
fn status_follows_one_snapshot_of_the_state_and_the_lock() {
    let f = fixture();
    f.list_model(UNUSED_URL, b"model");
    assert_eq!(f.status()["status"], "not_installed");
    let dir = f.version("payload-a");
    f.ready_at("payload-a");
    let status = f.status();
    assert_eq!(status["status"], "ready");
    assert_eq!(status["install_dir"], json!(dir));
    // A snapshot keeps describing the version it read.
    let seen = snapshot(&f.home, MODEL);
    f.version("payload-b");
    f.ready_at("payload-b");
    assert_eq!(seen.root.as_deref(), Some(dir.as_path()));
    assert_eq!(
        status_from(&f.home, &f.model(), &seen)["install_dir"],
        json!(dir)
    );
    // A recorded download that no install runs any more: the complete version still serves...
    f.write_state(json!({"status":"downloading","payload_dir":"payload-a","installed_at":"t"}));
    assert_eq!(f.status()["status"], "ready");
    // ...while one that runs is downloading.
    let lock = InstallLock::acquire(&install_lock_path(&f.home, MODEL)).expect("lock");
    assert_eq!(f.status()["status"], "downloading");
    drop(lock);
    f.write_state(json!({"status":"downloading"}));
    let status = f.status();
    assert_eq!(status["status"], "failed");
    assert_eq!(status["error"]["code"], "voice_model_install_interrupted");
    // A state that cannot be read, or names no valid directory, is not "not installed".
    fs::write(f.root.join(INSTALL_STATE), b"{").expect("corrupt state");
    let status = f.status();
    assert_eq!(status["status"], "failed");
    assert_eq!(status["error"]["code"], "voice_model_state_unreadable");
    f.write_state(json!({"status":"ready","payload_dir":"../elsewhere","installed_at":"t"}));
    assert_eq!(f.status()["error"]["code"], "voice_model_state_unreadable");
}

#[cfg(unix)]
#[test]
fn a_lock_that_cannot_be_checked_leaves_the_recorded_download_as_it_is() {
    let f = fixture();
    f.list_model(UNUSED_URL, b"model");
    f.version("payload-a");
    f.write_state(json!({"status":"downloading","payload_dir":"payload-a","installed_at":"t"}));
    let lock = install_lock_path(&f.home, MODEL);
    drop(InstallLock::acquire(&lock).expect("lock file"));
    if deny(&lock) {
        assert_eq!(f.status()["status"], "downloading");
    }
    allow(&lock);
}

#[test]
fn a_failed_state_write_is_judged_by_reading_the_state_back() {
    let staged = |f: &Fixture| {
        let job = start(&f.home, MODEL).expect("start");
        fs::write(job.staging.join("model.bin"), b"model").expect("model");
        job
    };
    // The write lands, then syncing its directory fails: the new version is current.
    let f = fixture();
    f.list_model(UNUSED_URL, b"model");
    let job = staged(&f);
    let outcome = commit(&job, |path, state| {
        write_state_file(path, state)?;
        Err(io::Error::other("directory sync failed"))
    });
    assert!(matches!(outcome, Commit::Switched));
    assert_eq!(f.state()["payload_dir"], job.payload_dir.as_str());
    // The write never lands: the installed version stays, and the new one is cleared away.
    let f = fixture();
    f.list_model(UNUSED_URL, b"model");
    let job = staged(&f);
    let Commit::NotSwitched(error) = commit(&job, |_, _| Err(io::Error::other("disk full"))) else {
        panic!("expected no switch");
    };
    assert_eq!(f.state()["status"], "downloading");
    finish_failed(&job, &error).expect("finish");
    // The renamed version waits for a cleanup that can confirm the current one.
    assert_eq!(f.entries(), [INSTALL_STATE, job.payload_dir.as_str()]);
    assert_eq!(f.state()["last_update_error"]["message"], "disk full");
    // The state cannot be read back: nothing is cleared or written over.
    let f = fixture();
    f.list_model(UNUSED_URL, b"model");
    let job = staged(&f);
    let outcome = commit(&job, |path, _| {
        fs::write(path, b"{")?;
        Err(io::Error::other("torn write"))
    });
    assert!(matches!(outcome, Commit::Unknown(_)));
    assert!(f.root.join(&job.payload_dir).join("model.bin").is_file());
    // Nor does a state that names neither version.
    let f = fixture();
    f.list_model(UNUSED_URL, b"model");
    let job = staged(&f);
    let outcome = commit(&job, |path, _| {
        fs::write(
            path,
            br#"{"status":"ready","payload_dir":"payload-unexpected"}"#,
        )?;
        Err(io::Error::other("raced"))
    });
    assert!(matches!(outcome, Commit::Unknown(_)));
}

#[test]
fn a_failed_install_never_deletes_a_version_directory() {
    let f = fixture();
    f.list_model(UNUSED_URL, b"model");
    let current = f.version("payload-same");
    f.ready_at("payload-same");
    let reader = current_version(&f.home, MODEL).expect("reader");
    // An install whose new version would carry the current version's name...
    let colliding = |f: &Fixture| {
        let mut job = start(&f.home, MODEL).expect("start");
        job.payload_dir = "payload-same".into();
        job
    };
    // ...fails before it downloads anything,
    let job = colliding(&f);
    let refused = VoiceError::new("voice_model_download_failed", "refused");
    finish_failed(&job, &refused).expect("finish");
    drop(job);
    // or when its unpacked files cannot take that name.
    let job = colliding(&f);
    fs::write(job.staging.join("model.bin"), b"new").expect("new file");
    let Commit::NotSwitched(error) = commit(&job, write_state_file) else {
        panic!("expected no switch");
    };
    finish_failed(&job, &error).expect("finish");
    assert!(!job.staging.exists());
    drop(job);
    assert_eq!(
        fs::read(current.join("model.bin")).expect("current"),
        b"model"
    );
    let status = f.status();
    assert_eq!(status["status"], "ready", "{status}");
    assert_eq!(status["install_dir"], json!(current));
    assert_eq!(pin_count(&current), 1);
    drop(reader);
}

#[test]
fn a_new_version_takes_a_name_no_entry_has() {
    let f = fixture();
    fs::create_dir_all(f.root.join("payload-taken")).expect("version");
    fs::create_dir_all(f.root.join(".install-staging-staged")).expect("staging");
    let mut names = vec!["taken", "staged"];
    // A dangling link still takes its name. Windows needs a privilege to create one.
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("/nonexistent", f.root.join("payload-linked")).expect("link");
        names.push("linked");
    }
    names.push("free");
    let mut candidates = names.into_iter();
    let id = claim_staging(&f.root, || candidates.next().expect("candidate").to_owned());
    assert_eq!(id.expect("free name"), "free");
    assert!(f.root.join(".install-staging-free").is_dir());
    let error = claim_staging(&f.root, || "taken".to_owned()).expect_err("no free name");
    assert_eq!(error.code, "voice_model_install_failed");
    // A name that cannot be checked is not free.
    #[cfg(unix)]
    {
        if deny(&f.root) {
            assert!(claim_staging(&f.root, || "unchecked".to_owned()).is_err());
        }
        allow(&f.root);
    }
}

#[test]
fn a_payload_dir_names_one_version_directory() {
    for name in ["payload-0f3a", "payload-a_b-c"] {
        assert!(valid_payload_dir(name), "{name}");
    }
    for name in [
        "payload-",
        "payload-a/b",
        "payload-..",
        "../payload-a",
        "/payload-a",
        "model",
        "payload-a b",
    ] {
        assert!(!valid_payload_dir(name), "{name}");
    }
    let root = Path::new("/models/m");
    assert_eq!(version_root(root, &json!({})).expect("legacy"), root);
    assert_eq!(
        version_root(root, &json!({"payload_dir":null})).expect("legacy"),
        root
    );
    assert_eq!(
        version_root(root, &json!({"payload_dir":"payload-a"})).expect("version"),
        root.join("payload-a")
    );
    assert!(version_root(root, &json!({"payload_dir":"../x"})).is_err());
    assert!(version_root(root, &json!({"payload_dir":7})).is_err());
}

#[tokio::test]
async fn blocking_work_keeps_the_lock_after_its_install_is_cancelled() {
    let f = fixture();
    f.list_model(UNUSED_URL, b"model");
    let lock = install_lock_path(&f.home, MODEL);
    let job = Arc::new(start(&f.home, MODEL).expect("start"));
    let (release, wait) = std::sync::mpsc::channel::<()>();
    let work = blocking(&job, move |_| {
        let _ = wait.recv();
        Ok(())
    });
    drop(work);
    drop(job);
    assert_eq!(probe_install_lock(&lock), LockProbe::Held);
    release.send(()).expect("release");
    wait_until(|| probe_install_lock(&lock) == LockProbe::Free).await;
}

#[test]
fn a_download_that_stops_early_removes_its_part() {
    let temp = tempfile::tempdir().expect("tempdir");
    let part = temp.path().join("nested/model.binpart");
    let (sender, receiver) = mpsc::channel(4);
    sender
        .try_send(Chunk::Data(b"partial".to_vec()))
        .expect("chunk");
    drop(sender);
    let error = write_artifact(&part, receiver)
        .err()
        .expect("an unfinished download fails");
    assert_eq!(error.code, "voice_model_download_failed");
    assert!(!part.exists());
}

#[cfg(target_os = "linux")]
#[test]
fn a_full_disk_fails_the_download_and_removes_its_part() {
    if !Path::new("/dev/full").exists() {
        return;
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let part = temp.path().join("model.binpart");
    // Every write through this link fails as on a full disk.
    std::os::unix::fs::symlink("/dev/full", &part).expect("link");
    let (sender, receiver) = mpsc::channel(4);
    sender
        .try_send(Chunk::Data(vec![0_u8; 64 * 1024]))
        .expect("chunk");
    sender.try_send(Chunk::Finish).expect("finish");
    let error = write_artifact(&part, receiver)
        .err()
        .expect("a full disk fails");
    assert_eq!(error.code, "voice_io_error");
    assert!(fs::symlink_metadata(&part).is_err());
}

#[test]
fn progress_writes_keep_the_installed_version() {
    let f = fixture();
    f.list_model(UNUSED_URL, b"model");
    f.version("payload-a");
    f.write_state(json!({
        "status":"ready","payload_dir":"payload-a","installed_at":"t","manifest_sha256":"m",
        "last_update_error":{"code":"earlier"}
    }));
    let kept = Kept::read(&f.root, &payload::read_state(&f.root)).expect("kept");
    let state = progress_state(&f.model(), &kept, 3, Some(0));
    assert_eq!(state["status"], "downloading");
    assert_eq!(state["installed_at"], "t");
    assert_eq!(state["installed_manifest_sha256"], "m");
    assert_eq!(state["payload_dir"], "payload-a");
    assert_eq!(state["last_update_error"]["code"], "earlier");
    assert_eq!(state["current_artifact_path"], "model.bin");
}

#[test]
fn an_install_refuses_to_replace_a_state_it_cannot_use() {
    let f = fixture();
    f.list_model(UNUSED_URL, b"model");
    f.version("payload-a");
    // A state naming no valid directory stays as it is.
    f.write_state(json!({"status":"ready","payload_dir":"../payload-a","installed_at":"t"}));
    let before = fs::read(f.root.join(INSTALL_STATE)).expect("state");
    let error = start(&f.home, MODEL).err().expect("refused");
    assert_eq!(error.code, "voice_model_state_unreadable");
    assert_eq!(fs::read(f.root.join(INSTALL_STATE)).expect("state"), before);
    // So does one this user cannot read, though the directory would let a write replace it.
    #[cfg(unix)]
    {
        f.ready_at("payload-a");
        let state = f.root.join(INSTALL_STATE);
        if deny(&state) {
            let error = start(&f.home, MODEL).err().expect("refused");
            assert_eq!(error.code, "voice_model_state_unreadable");
            allow(&state);
            assert_eq!(f.state()["payload_dir"], "payload-a");
        }
        allow(&state);
        // A link to a missing state is not a missing state, and the link stays.
        fs::remove_file(&state).expect("remove state");
        std::os::unix::fs::symlink(f.root.join("gone.json"), &state).expect("link");
        let error = start(&f.home, MODEL).err().expect("refused");
        assert_eq!(error.code, "voice_model_state_unreadable");
        assert_eq!(f.status()["status"], "failed");
        assert!(
            fs::symlink_metadata(&state)
                .expect("link")
                .file_type()
                .is_symlink()
        );
        fs::remove_file(&state).expect("unlink");
        assert_eq!(f.status()["status"], "not_installed");
    }
}

#[test]
fn reserved_names_are_not_model_ids() {
    for id in [
        ".", "..", ".locks", "_shared", "-model", "model.", "model..",
    ] {
        let f = fixture();
        let manifest = json!({"voice_secretary_asr_models":[{"model_id":id,"artifacts":[]}]});
        cccc_core::fs::write_json(&f.home.root().join("config/voice-models.json"), &manifest)
            .expect("manifest");
        let error = catalog(&f.home).expect_err("rejected");
        assert_eq!(error.code, "voice_model_manifest_invalid", "{id}");
        // Startup cleanup reads the same catalog and touches nothing.
        let staging = f
            .home
            .root()
            .join("cache/voice-models/.locks/.install-staging-x");
        fs::create_dir_all(&staging).expect("staging");
        collect_all_garbage(&f.home);
        assert!(staging.exists(), "{id}");
    }
}

#[cfg(unix)]
#[test]
fn an_overlay_that_cannot_be_read_stops_cleanup() {
    let f = fixture();
    let builtin = catalog(&f.home)
        .expect("built-in catalog")
        .into_keys()
        .next()
        .expect("a built-in model");
    let staging = model_dir(&f.home, &builtin).join(format!("{STAGING_PREFIX}left"));
    fs::create_dir_all(&staging).expect("staging");
    let config = f.home.root().join("config");
    let overlay = config.join("voice-models.json");
    fs::create_dir_all(&config).expect("config");
    // A dangling link is not a missing overlay,
    std::os::unix::fs::symlink(config.join("gone.json"), &overlay).expect("link");
    assert!(catalog(&f.home).is_err());
    collect_all_garbage(&f.home);
    assert!(staging.exists());
    fs::remove_file(&overlay).expect("unlink");
    // and neither is one in a directory that cannot be read.
    fs::write(&overlay, br#"{"voice_secretary_asr_models":[]}"#).expect("overlay");
    if deny(&config) {
        assert!(catalog(&f.home).is_err());
        collect_all_garbage(&f.home);
        assert!(staging.exists());
    }
    allow(&config);
    fs::remove_file(&overlay).expect("remove");
    collect_all_garbage(&f.home);
    assert!(
        !staging.exists(),
        "cleanup runs once the overlay is really absent"
    );
}

#[test]
fn cleanup_keeps_every_version_of_a_model_without_required_files() {
    let f = fixture();
    f.list_artifacts(vec![plain(UNUSED_URL, "model.bin", b"model")], &[]);
    f.version("payload-a");
    f.version("payload-b");
    f.ready_at("payload-a");
    collect_garbage(&f.root, &f.model(), &payload::read_state(&f.root));
    assert!(f.root.join("payload-b").exists());
}

#[test]
fn cleanup_keeps_every_version_until_the_current_one_is_confirmed() {
    type Setup = fn(&Fixture);
    let cases: Vec<(&str, Setup)> = vec![
        ("missing state", |_| {}),
        ("corrupt state", |f| {
            fs::write(f.root.join(INSTALL_STATE), b"{").expect("corrupt");
        }),
        ("invalid pointer", |f| f.ready_at("../payload-a")),
        ("missing directory", |f| f.ready_at("payload-gone")),
        ("incomplete directory", |f| {
            fs::remove_file(f.root.join("payload-a/model.bin")).expect("remove");
            f.ready_at("payload-a");
        }),
        ("legacy layout", |f| {
            f.write_state(json!({"status":"ready","installed_at":"t"}));
        }),
    ];
    for (case, setup) in cases {
        let f = fixture();
        f.list_model(UNUSED_URL, b"model");
        f.version("payload-a");
        f.version("payload-b");
        fs::write(f.root.join("model.bin"), b"legacy").expect("legacy");
        fs::create_dir(f.root.join(".install-staging-x")).expect("staging");
        setup(&f);
        let mut expected = f.entries();
        expected.retain(|name| name != ".install-staging-x");
        collect_garbage(&f.root, &f.model(), &payload::read_state(&f.root));
        assert_eq!(f.entries(), expected, "{case}");
    }
}

#[cfg(unix)]
#[test]
fn cleanup_keeps_every_version_when_the_state_or_a_version_cannot_be_read() {
    for denied in [INSTALL_STATE, "payload-a"] {
        let f = fixture();
        f.list_model(UNUSED_URL, b"model");
        f.version("payload-a");
        f.version("payload-b");
        f.ready_at("payload-a");
        fs::create_dir(f.root.join(".install-staging-x")).expect("staging");
        let path = f.root.join(denied);
        if deny(&path) {
            collect_garbage(&f.root, &f.model(), &payload::read_state(&f.root));
            allow(&path);
            assert!(f.root.join("payload-b").exists(), "{denied}");
            assert!(!f.root.join(".install-staging-x").exists(), "{denied}");
        }
        allow(&path);
    }
}

#[test]
fn cleanup_removes_what_the_confirmed_version_replaced_once_no_reader_holds_it() {
    let f = fixture();
    f.list_model(UNUSED_URL, b"model");
    for version in ["payload-a", "payload-b", "payload-c"] {
        f.version(version);
    }
    fs::write(f.root.join("model.bin"), b"legacy").expect("legacy");
    fs::write(f.root.join(".install-state.json.0.tmp"), b"{}").expect("temp");
    fs::write(f.root.join(INSTALL_LOCK), b"1\n").expect("marker");
    fs::create_dir(f.root.join(".install-previous-x")).expect("previous");
    fs::create_dir(f.root.join(".install-staging-x")).expect("staging");
    // Readers still hold the legacy layout and payload-b from before payload-a became current.
    f.write_state(json!({"status":"ready","installed_at":"t"}));
    let legacy_reader = current_version(&f.home, MODEL).expect("legacy reader");
    f.ready_at("payload-b");
    let reader = current_version(&f.home, MODEL).expect("reader");
    f.ready_at("payload-a");
    collect_garbage(&f.root, &f.model(), &payload::read_state(&f.root));
    assert_eq!(
        f.entries(),
        [
            ".install-previous-x",
            ".install-state.json.0.tmp",
            INSTALL_LOCK,
            INSTALL_STATE,
            "model.bin",
            "payload-a",
            "payload-b",
        ]
    );
    drop((reader, legacy_reader));
    collect_garbage(&f.root, &f.model(), &payload::read_state(&f.root));
    assert_eq!(f.entries(), [INSTALL_LOCK, INSTALL_STATE, "payload-a"]);
}

#[test]
fn startup_cleanup_leaves_a_model_whose_install_runs() {
    let f = fixture();
    f.list_model(UNUSED_URL, b"model");
    let staging = f.root.join(".install-staging-x");
    fs::create_dir_all(&staging).expect("staging");
    let lock = InstallLock::acquire(&install_lock_path(&f.home, MODEL)).expect("lock");
    collect_all_garbage(&f.home);
    assert!(staging.exists());
    drop(lock);
    collect_all_garbage(&f.home);
    assert!(!staging.exists());
}

#[test]
fn the_lock_probe_tells_free_held_and_unknown() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("model.lock");
    assert_eq!(probe_install_lock(&path), LockProbe::Free);
    let lock = InstallLock::acquire(&path).expect("lock");
    assert_eq!(probe_install_lock(&path), LockProbe::Held);
    drop(lock);
    assert_eq!(probe_install_lock(&path), LockProbe::Free);
    #[cfg(unix)]
    {
        if deny(&path) {
            assert_eq!(probe_install_lock(&path), LockProbe::Unknown);
        }
        allow(&path);
    }
}

#[test]
fn taking_the_lock_waits_out_a_status_probe() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("model.lock");
    drop(InstallLock::acquire(&path).expect("lock file"));
    // As a status poll that holds the lock at the moment of a click.
    let probe = fs::File::open(&path).expect("probe");
    probe.try_lock_exclusive().expect("probe lock");
    let released = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(5));
        drop(probe);
    });
    let lock = InstallLock::acquire(&path).expect("lock after the probe");
    released.join().expect("probe thread");
    drop(lock);
}

/// Takes away all access; false when this user reads through any mode, as root does.
#[cfg(unix)]
fn deny(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o000)).expect("chmod");
    fs::File::open(path).is_err()
}

#[cfg(unix)]
fn allow(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o755));
}
