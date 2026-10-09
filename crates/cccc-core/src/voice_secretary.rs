//! Durable task receipts and controlled document commits for the ASR secretary.
//!
//! Providers start in `workspace/`. The target and commit journal live beside it.
//! A provider turn ending is not a business receipt, and a receipt is not proof
//! that the provider process has been cleaned up.
use cccc_contracts::utc_now;
use cccc_contracts::voice_secretary::{SecretaryTaskKind, SecretaryTaskPhase, SecretaryTaskTarget};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::{HomeLayout, fs};

const MAX_DOCUMENT_BYTES: u64 = 4 * 1024 * 1024;

mod index;
use index::TaskIndex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretaryTask {
    pub schema: u32,
    pub task_id: String,
    pub target: SecretaryTaskTarget,
    pub inputs: Vec<Value>,
    #[serde(default)]
    pub guidance: String,
    pub phase: SecretaryTaskPhase,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub generation: String,
    #[serde(default)]
    pub thread_id: String,
    #[serde(default)]
    pub turn_id: String,
    #[serde(default)]
    pub runtime_fingerprint: String,
    /// True only after a launched adapter confirmed task-only native writes.
    /// Runtime selection or a missing commit alone is not proof of safe replay.
    #[serde(default)]
    pub isolated_document_writes: bool,
    #[serde(default)]
    pub document_file: Option<PathBuf>,
    #[serde(default)]
    pub base_version: String,
    #[serde(default)]
    pub prepared_version: String,
    #[serde(default)]
    pub receipt: Option<SecretaryReceipt>,
    #[serde(default)]
    pub diagnostic: String,
    #[serde(default)]
    pub cleanup_confirmed: bool,
    #[serde(default)]
    pub projected_at: String,
    #[serde(default)]
    pub projection_error: String,
    #[serde(default)]
    pub previous_task_id: String,
    #[serde(default)]
    pub superseded_by: String,
    #[serde(default)]
    pub followup: String,
    #[serde(default)]
    pub forwarded_event_id: String,
    /// Shutdown is recoverable; an explicit user cancellation is not replayed.
    #[serde(default)]
    pub cancellation_reason: String,
    #[serde(default)]
    pub recovery_attempts: u32,
    #[serde(default)]
    pub artifacts_retired: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretaryReceipt {
    pub status: SecretaryTaskPhase,
    pub output: Value,
    pub document_version: String,
    pub committed_at: String,
}

impl SecretaryTask {
    pub fn unresolved(&self) -> bool {
        !self.cleanup_confirmed
            || self.superseded_by.is_empty()
                && self.phase != SecretaryTaskPhase::Done
                && (self.phase != SecretaryTaskPhase::Cancelled
                    || self.target.kind
                        == cccc_contracts::voice_secretary::SecretaryTaskKind::Document)
    }

    /// User-facing summary, independent of the model's instruction envelope.
    pub fn preview(&self) -> String {
        let Some(input) = self.inputs.first() else {
            return String::new();
        };
        input["metadata"]["user_preview"]
            .as_str()
            .filter(|text| !text.is_empty())
            .or_else(|| {
                (self.target.kind == SecretaryTaskKind::Document)
                    .then(|| input["text"].as_str())
                    .flatten()
            })
            .unwrap_or("")
            .chars()
            .take(240)
            .collect()
    }

    pub fn new(target: SecretaryTaskTarget, inputs: Vec<Value>) -> Self {
        let now = utc_now();
        Self {
            schema: 1,
            task_id: uuid::Uuid::new_v4().simple().to_string(),
            target,
            inputs,
            guidance: String::new(),
            phase: SecretaryTaskPhase::Queued,
            created_at: now.clone(),
            updated_at: now,
            generation: String::new(),
            thread_id: String::new(),
            turn_id: String::new(),
            runtime_fingerprint: String::new(),
            isolated_document_writes: false,
            document_file: None,
            base_version: String::new(),
            prepared_version: String::new(),
            receipt: None,
            diagnostic: String::new(),
            cleanup_confirmed: true,
            projected_at: String::new(),
            projection_error: String::new(),
            previous_task_id: String::new(),
            superseded_by: String::new(),
            followup: String::new(),
            forwarded_event_id: String::new(),
            cancellation_reason: String::new(),
            recovery_attempts: 0,
            artifacts_retired: false,
        }
    }

    pub fn covers(&self, input: &Value) -> bool {
        let id = input["input_id"].as_str().filter(|id| !id.is_empty());
        id.is_some_and(|id| self.inputs.iter().any(|v| v["input_id"] == id))
    }
}

#[derive(Clone)]
pub struct SecretaryTaskStore {
    home: HomeLayout,
    index: Option<Arc<Mutex<TaskIndex>>>,
}

#[derive(Default)]
pub struct SecretaryGroupView {
    pub tasks: Vec<SecretaryTask>,
    pub busy: bool,
    pub pending: bool,
    pub status: Option<SecretaryTaskPhase>,
    pub unprocessed_document_sources: usize,
}

impl SecretaryTaskStore {
    pub fn grant_file(&self, task_id: &str) -> io::Result<PathBuf> {
        Ok(self.directory(task_id)?.join("mcp-grant"))
    }

    pub fn revoke_grant_file(&self, task_id: &str) -> io::Result<()> {
        remove_if_present(&self.grant_file(task_id)?, false)
    }

    pub fn new(home: HomeLayout) -> Self {
        Self { home, index: None }
    }

    /// The daemon owns all writes. Load durable records once, then publish each
    /// successful write to the shared index; independent readers can use `new`.
    pub fn indexed(home: HomeLayout) -> io::Result<Self> {
        let mut store = Self::new(home);
        let mut index = TaskIndex::default();
        for task in store.list()? {
            index.insert(task);
        }
        store.index = Some(Arc::new(Mutex::new(index)));
        Ok(store)
    }

    fn publish(&self, task: SecretaryTask) {
        if let Some(index) = &self.index {
            index.lock().unwrap_or_else(|e| e.into_inner()).insert(task);
        }
    }

    pub fn directory(&self, task_id: &str) -> io::Result<PathBuf> {
        if task_id.len() != 32 || !task_id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid secretary task id",
            ));
        }
        Ok(self.home.root().join("voice-secretary/jobs").join(task_id))
    }

    /// Provider-writable material is separate from daemon-owned receipts/grants.
    pub fn runtime_workspace(&self) -> PathBuf {
        self.home.root().join("voice-secretary/workspace")
    }

    pub fn session_grant_file(&self) -> PathBuf {
        self.home.root().join("voice-secretary/session-grant")
    }

    pub fn workspace(&self, task_id: &str) -> io::Result<PathBuf> {
        self.directory(task_id)?;
        Ok(self.runtime_workspace().join(task_id))
    }

    /// Move existing durable candidates before recovery; never discard them.
    pub fn relocate_workspace(&self, task_id: &str) -> io::Result<()> {
        let previous = self.directory(task_id)?.join("workspace");
        if previous.exists() {
            let next = self.workspace(task_id)?;
            if next.exists() {
                return Err(io::Error::other(
                    "Both Secretary working copies exist; recovery is required",
                ));
            }
            std::fs::create_dir_all(self.runtime_workspace())?;
            std::fs::rename(previous, next)?;
        }
        Ok(())
    }

    pub fn candidate(&self, task_id: &str) -> io::Result<String> {
        let dir = self.directory(task_id)?;
        let path = if dir.join("prepared.md").is_file() {
            dir.join("prepared.md")
        } else {
            self.workspace(task_id)?.join("document.md")
        };
        String::from_utf8(read_regular_file(&path)?).map_err(io::Error::other)
    }

    pub fn create(&self, task: &SecretaryTask) -> io::Result<()> {
        let dir = self.directory(&task.task_id)?;
        // The caller owns acceptance serialization; never replace an existing receipt.
        std::fs::create_dir_all(dir.parent().expect("jobs parent"))?;
        std::fs::create_dir(&dir)?;
        private_directory(&dir)?;
        std::fs::create_dir_all(self.workspace(&task.task_id)?)?;
        fs::write_json(&dir.join("task.json"), task)?;
        self.publish(task.clone());
        Ok(())
    }

    pub fn load(&self, task_id: &str) -> io::Result<SecretaryTask> {
        self.directory(task_id)?;
        if let Some(index) = &self.index {
            return index
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .load(task_id);
        }
        self.load_disk(task_id)
    }

    fn load_disk(&self, task_id: &str) -> io::Result<SecretaryTask> {
        let task: SecretaryTask = fs::read_json(&self.directory(task_id)?.join("task.json"))?;
        if task.schema != 1 || task.task_id != task_id {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported or mismatched secretary task record",
            ));
        }
        Ok(task)
    }

    pub fn update<T>(
        &self,
        task_id: &str,
        change: impl FnOnce(&mut SecretaryTask) -> io::Result<T>,
    ) -> io::Result<T> {
        let dir = self.directory(task_id)?;
        fs::with_exclusive_lock(&dir.join("task.lock"), || {
            let mut task = self.load_disk(task_id)?;
            let result = match change(&mut task) {
                Ok(result) => result,
                Err(error) => {
                    // A prepared commit journal can have been written before a
                    // later I/O failure. Never hide it from reconciliation.
                    if let Ok(saved) = self.load_disk(task_id) {
                        self.publish(saved);
                    }
                    return Err(error);
                }
            };
            task.updated_at = utc_now();
            if let Err(error) = fs::write_json(&dir.join("task.json"), &task) {
                if let Ok(saved) = self.load_disk(task_id) {
                    self.publish(saved);
                }
                return Err(error);
            }
            self.publish(task);
            Ok(result)
        })
    }

    pub fn list(&self) -> io::Result<Vec<SecretaryTask>> {
        if let Some(index) = &self.index {
            return Ok(index.lock().unwrap_or_else(|e| e.into_inner()).list(None));
        }
        let root = self.home.root().join("voice-secretary/jobs");
        let entries = match std::fs::read_dir(root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut tasks = Vec::new();
        for entry in entries {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let id = entry.file_name().to_string_lossy().into_owned();
                // Incomplete acceptance directories have no published task.
                if self.directory(&id)?.join("task.json").exists() {
                    tasks.push(self.load(&id)?);
                }
            }
        }
        tasks.sort_by(|a, b| (&a.created_at, &a.task_id).cmp(&(&b.created_at, &b.task_id)));
        Ok(tasks)
    }

    pub fn group_tasks(&self, group_id: &str) -> io::Result<Vec<SecretaryTask>> {
        if let Some(index) = &self.index {
            return Ok(index
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .list(Some(group_id)));
        }
        Ok(self
            .list()?
            .into_iter()
            .filter(|t| t.target.group_id == group_id)
            .collect())
    }

    /// Clone only unresolved work and the requested recent history, not every
    /// receipt retained for durable source deduplication.
    pub fn group_view(&self, group_id: &str, recent: usize) -> io::Result<SecretaryGroupView> {
        if let Some(index) = &self.index {
            return Ok(index
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .group_view(group_id, recent));
        }
        let mut index = TaskIndex::default();
        for task in self.group_tasks(group_id)? {
            index.insert(task);
        }
        Ok(index.group_view(group_id, recent))
    }

    pub fn source_task(&self, group_id: &str, input_id: &str) -> io::Result<Option<String>> {
        if let Some(index) = &self.index {
            return Ok(index
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .source_task(group_id, input_id));
        }
        Ok(self
            .group_tasks(group_id)?
            .into_iter()
            .find(|t| t.inputs.iter().any(|v| v["input_id"] == input_id))
            .map(|t| t.task_id))
    }

    pub fn successor(&self, task_id: &str) -> io::Result<Option<SecretaryTask>> {
        if let Some(index) = &self.index {
            return Ok(index
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .successor(task_id));
        }
        Ok(self
            .list()?
            .into_iter()
            .find(|t| t.previous_task_id == task_id))
    }

    pub fn queued(&self) -> io::Result<Vec<SecretaryTask>> {
        if let Some(index) = &self.index {
            return Ok(index.lock().unwrap_or_else(|e| e.into_inner()).queued());
        }
        let mut tasks = self
            .list()?
            .into_iter()
            .filter(|t| t.phase == SecretaryTaskPhase::Queued)
            .collect::<Vec<_>>();
        tasks.sort_by_key(queued_order);
        Ok(tasks)
    }

    pub fn unfinished(&self) -> io::Result<Vec<SecretaryTask>> {
        if let Some(index) = &self.index {
            return Ok(index.lock().unwrap_or_else(|e| e.into_inner()).unfinished());
        }
        Ok(self
            .list()?
            .into_iter()
            .filter(|t| {
                !t.cleanup_confirmed
                    || t.phase != SecretaryTaskPhase::Done && t.superseded_by.is_empty()
            })
            .collect())
    }

    /// Retire heavy artifacts only after business projection and process cleanup.
    /// Keep compact source IDs/receipts so canonical sources cannot be replayed.
    pub fn retire_artifacts(&self, task_id: &str) -> io::Result<()> {
        self.retire_resolved_artifacts(task_id, task_id)
    }

    /// Completed continuations resolve their predecessors' sources. Retain the
    /// original outcome/receipt, without an obsolete full working copy.
    pub fn retire_resolved_artifacts(&self, task_id: &str, resolution_id: &str) -> io::Result<()> {
        let resolved = self.load(resolution_id)?;
        if !business_settled(&resolved) {
            return Ok(());
        }
        let old = self.load(task_id)?;
        if !can_retire_artifacts(&old) && !resolved_by(&old, &resolved) {
            return Ok(());
        }
        self.update(task_id, |task| {
            if !can_retire_artifacts(task) && !resolved_by(task, &resolved) { return Ok(()); }
            let dir = self.directory(task_id)?;
            remove_if_present(&self.workspace(task_id)?, true)?;
            remove_if_present(&dir.join("prepared.md"), false)?;
            for input in &mut task.inputs {
                let preview = input["text"].as_str().unwrap_or("").chars().take(240).collect::<String>();
                let user_preview = input["metadata"]["user_preview"].as_str().unwrap_or("").chars().take(240).collect::<String>();
                *input = serde_json::json!({"input_id":input["input_id"],"input_append_id":input["input_append_id"],"seq":input["seq"],"text":preview,"metadata":{"user_preview":user_preview}});
            }
            task.artifacts_retired = true;
            // Settled continuations no longer need model instructions. Keep
            // source identities and business receipts for recovery/deduplication.
            task.guidance.clear();
            Ok(())
        })
    }

    pub fn remove(&self, task_id: &str) -> io::Result<()> {
        let task = self.load(task_id)?;
        if task.phase.executing() || !task.cleanup_confirmed {
            return Err(io::Error::other("Secretary task cleanup is unresolved"));
        }
        remove_if_present(&self.workspace(task_id)?, true)?;
        remove_if_present(&self.directory(task_id)?, true)?;
        if let Some(index) = &self.index {
            index
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(task_id);
        }
        Ok(())
    }

    /// Capture the version the model actually reads, after earlier queued work.
    /// The host resolves and validates this path; it is never a tool parameter.
    pub fn prepare_document(&self, task_id: &str, file: &Path) -> io::Result<()> {
        let bytes = read_regular_file(file)?;
        let file = std::fs::canonicalize(file)?;
        let workspace = self.workspace(task_id)?;
        self.update(task_id, |task| {
            if task.phase != SecretaryTaskPhase::Starting || !task.base_version.is_empty() {
                return Err(io::Error::other("document snapshot is already fixed"));
            }
            fs::atomic_write(&workspace.join("document.md"), &bytes)?;
            task.base_version = digest(&bytes);
            task.document_file = Some(file);
            Ok(())
        })
    }

    /// Terminal document action. `validate` checks the original Group/scope and
    /// document registration while the caller holds its mutation permit.
    pub fn commit_document(
        &self,
        task_id: &str,
        base_version: &str,
        validate: impl FnOnce(&SecretaryTask) -> io::Result<()>,
    ) -> io::Result<SecretaryReceipt> {
        let directory = self.directory(task_id)?;
        let candidate = self.workspace(task_id)?.join("document.md");
        self.update(task_id, |task| {
            if let Some(receipt) = task.receipt.as_ref() {
                return Ok(receipt.clone());
            }
            if !task.phase.executing() || task.base_version != base_version {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "stale secretary commit",
                ));
            }
            validate(task)?;
            let file = task
                .document_file
                .as_ref()
                .ok_or_else(|| io::Error::other("not a document task"))?;
            let bytes = read_regular_file(&candidate)?;
            if std::str::from_utf8(&bytes).is_err() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "candidate must be UTF-8 Markdown",
                ));
            }
            if digest(&read_regular_file(file)?) != task.base_version {
                return Ok(conflict(task));
            }
            // Keep a host-owned immutable submission before replacing the original.
            // Both the content and the source range are now recoverable after a crash.
            fs::atomic_write(&directory.join("prepared.md"), &bytes)?;
            task.prepared_version = digest(&bytes);
            fs::write_json(&directory.join("task.json"), task)?;
            fs::atomic_write_preserving_mode(file, &bytes)?;
            Ok(document_receipt(task))
        })
    }

    /// Called at daemon startup, never from polling. Recover only a submitted
    /// immutable artifact; never rerun an uncertain provider prompt.
    pub fn recover(
        &self,
        task_id: &str,
        validate: impl FnOnce(&SecretaryTask) -> io::Result<()>,
    ) -> io::Result<()> {
        let dir = self.directory(task_id)?;
        self.update(task_id, |task| {
            if task.receipt.is_none() && !task.prepared_version.is_empty() {
                if let Err(error) = validate(task) {
                    task.phase = SecretaryTaskPhase::Unconfirmed;
                    task.diagnostic = format!("Submitted artifact retained; target could not be reconciled: {error}");
                    return Ok(());
                }
                let file = task.document_file.as_ref().ok_or_else(|| io::Error::other("missing commit target"))?;
                let current = digest(&read_regular_file(file)?);
                if current == task.prepared_version {
                    document_receipt(task);
                } else if current == task.base_version {
                    let bytes = read_regular_file(&dir.join("prepared.md"))?;
                    if digest(&bytes) != task.prepared_version {
                        return Err(io::Error::other("secretary submission checksum mismatch"));
                    }
                    fs::atomic_write_preserving_mode(file, &bytes)?;
                    document_receipt(task);
                } else {
                    conflict(task);
                }
            }
            if task.phase.executing() {
                task.phase = task.receipt.as_ref().map_or(SecretaryTaskPhase::Unconfirmed, |r| r.status);
                task.diagnostic = "Previous execution ended without a confirmed cleanup receipt; no prompt was replayed.".into();
            }
            // Process-ledger reconciliation remains the prerequisite for reusing capacity.
            Ok(())
        })
    }
}

fn queued_order(task: &SecretaryTask) -> (String, String) {
    // A recovered ASR batch precedes newer source batches, preserving document
    // chronology. User requests/clarifications retain acceptance-time FIFO.
    let at = if task.target.kind == cccc_contracts::voice_secretary::SecretaryTaskKind::Document
        && task.target.request_id.is_empty()
    {
        task.inputs
            .first()
            .and_then(|input| input["created_at"].as_str())
            .unwrap_or(&task.created_at)
    } else {
        &task.created_at
    };
    (at.to_owned(), task.task_id.clone())
}

fn remove_if_present(path: &Path, directory: bool) -> io::Result<()> {
    let result = if directory {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
    match result {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn read_regular_file(path: &Path) -> io::Result<Vec<u8>> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_DOCUMENT_BYTES
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected a regular document of at most 4 MiB",
        ));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_DOCUMENT_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "document exceeds 4 MiB",
        ));
    }
    Ok(bytes)
}

fn can_retire_artifacts(task: &SecretaryTask) -> bool {
    !task.artifacts_retired && business_settled(task)
}

fn business_settled(task: &SecretaryTask) -> bool {
    task.phase == SecretaryTaskPhase::Done
        && task.cleanup_confirmed
        && task
            .receipt
            .as_ref()
            .is_some_and(|r| r.status == SecretaryTaskPhase::Done)
        && !task.projected_at.is_empty()
        && task.projection_error.is_empty()
}

fn resolved_by(old: &SecretaryTask, resolved: &SecretaryTask) -> bool {
    !old.artifacts_retired
        && old.cleanup_confirmed
        && !old.phase.executing()
        && old.phase != SecretaryTaskPhase::Queued
        && !old.superseded_by.is_empty()
        && old.target == resolved.target
        && old.inputs.iter().all(|input| resolved.covers(input))
}

fn document_receipt(task: &mut SecretaryTask) -> SecretaryReceipt {
    let receipt = SecretaryReceipt {
        status: SecretaryTaskPhase::Done,
        output: serde_json::json!({"document_path":task.target.document_path}),
        document_version: task.prepared_version.clone(),
        committed_at: utc_now(),
    };
    task.receipt = Some(receipt.clone());
    receipt
}

fn conflict(task: &mut SecretaryTask) -> SecretaryReceipt {
    let receipt = SecretaryReceipt {
        status: SecretaryTaskPhase::Conflict,
        output: serde_json::json!({"document_path":task.target.document_path}),
        document_version: String::new(),
        committed_at: utc_now(),
    };
    task.receipt = Some(receipt.clone());
    task.diagnostic = "The document changed after this task read it. The candidate was retained; the original was not replaced.".into();
    receipt
}

fn private_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
#[path = "voice_secretary/tests.rs"]
mod tests;
