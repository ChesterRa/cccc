//! Durable facts for the global assistant. A registered input is not proof
//! that a Runtime received or completed it.
use cccc_contracts::chet::{GroupInput, GroupInputSender};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
use uuid::Uuid;

const HEADER: &[u8] = b"{\"version\":1}\n";
const INITIALIZED: &[u8] = b"CCCC_CHET_HISTORY_V1\n";
const MAX_LINE: u64 = 1024 * 1024;
const MAX_PAGE_BYTES: u64 = 1024 * 1024;

/// A label captured with the input, not an authorization or a workspace path.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GroupContext {
    pub group_id: String,
    pub group_title: String,
    pub scope_key: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextRequest {
    pub text: String,
    pub context: Option<GroupContext>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    pub generation: String,
    pub turn_id: String,
    pub status: String,
    pub text: String,
    pub error: String,
}

/// A Runtime receipt confirms admission of one input, not completion.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub generation: String,
    pub turn_id: String,
}

/// A request observed from Realtime Voice. It is not the user's verbatim
/// transcript and is not, by itself, an Analyst admission receipt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceDelegation {
    pub call_id: String,
    pub delegation_id: String,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptRole {
    User,
    Assistant,
}

/// Only text actually observed by the call owner may be recorded. An
/// unfinished or incomplete turn must remain distinguishable from a final.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transcript {
    pub call_id: String,
    pub turn_id: String,
    /// Order in which the browser first observed this turn within the call.
    pub seq: u64,
    pub role: TranscriptRole,
    pub text: String,
    pub final_update: bool,
    pub complete: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Record {
    Input {
        id: Uuid,
        created_at: String,
        request: TextRequest,
    },
    VoiceDelegation {
        id: Uuid,
        created_at: String,
        delegation: VoiceDelegation,
    },
    /// A source reference, not a copy of the message or proof that a Runtime
    /// has received it.
    GroupInput {
        id: Uuid,
        created_at: String,
        input: GroupInput,
    },
    /// The host may have submitted this Group input after this durable claim.
    /// A restart must not automatically submit it a second time.
    GroupDispatch {
        id: Uuid,
        created_at: String,
        input_id: Uuid,
    },
    Receipt {
        id: Uuid,
        created_at: String,
        input_id: Uuid,
        receipt: Receipt,
    },
    Result {
        id: Uuid,
        created_at: String,
        input_ids: Vec<Uuid>,
        outcome: Outcome,
    },
    /// The host did not submit this input to a Runtime. This is distinct from
    /// both an unconfirmed delivery and a Runtime-reported turn failure.
    NotStarted {
        id: Uuid,
        created_at: String,
        input_id: Uuid,
        reason: StartFailure,
    },
    Transcript {
        id: Uuid,
        created_at: String,
        transcript: Transcript,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartFailure {
    AssistantUnavailable,
    Cancelled,
    SourceUnavailable,
}

impl Record {
    fn id(&self) -> Uuid {
        match self {
            Self::Input { id, .. }
            | Self::VoiceDelegation { id, .. }
            | Self::GroupInput { id, .. }
            | Self::GroupDispatch { id, .. }
            | Self::Receipt { id, .. }
            | Self::Result { id, .. }
            | Self::NotStarted { id, .. }
            | Self::Transcript { id, .. } => *id,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Registration {
    Inserted,
    Existing,
}

#[derive(Debug)]
pub struct Page {
    pub records: Vec<Record>,
    /// Byte boundary supplied by this store, represented as a string at the API.
    pub before: Option<u64>,
    pub head: u64,
}

#[derive(Debug)]
pub struct ForwardPage {
    pub records: Vec<Record>,
    pub next: u64,
    pub has_more: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid history request: {0}")]
    Invalid(&'static str),
    #[error("history ID was already used for a different record")]
    IdConflict,
    #[error("a result already exists for an input")]
    OutcomeConflict,
    #[error("a different Runtime receipt already exists for an input")]
    ReceiptConflict,
    #[error("a result refers to an unknown input")]
    UnknownInput,
    #[error("history is malformed or incomplete; no record was skipped")]
    Corrupt,
    #[error("history storage failed before writing: {0}")]
    Rejected(#[source] io::Error),
    #[error("history write outcome is unknown; do not dispatch or replay: {0}")]
    Unknown(#[source] io::Error),
}

pub struct Store {
    home: PathBuf,
    root: PathBuf,
}

impl Store {
    pub fn new(home: impl Into<PathBuf>) -> Self {
        let home = home.into();
        let root = home.join("state").join("chet");
        Self { home, root }
    }

    fn journal(&self) -> PathBuf {
        self.root.join("history.jsonl")
    }

    fn lock_path(&self) -> PathBuf {
        self.root.join("history.lock")
    }

    fn writer(&self) -> Result<File, Error> {
        ensure_child(&self.home, "state").map_err(Error::Rejected)?;
        ensure_child(&self.home.join("state"), "chet").map_err(Error::Rejected)?;
        let mut options = OpenOptions::new();
        options.create(true).truncate(false).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(self.lock_path()).map_err(Error::Rejected)?;
        file.lock_exclusive().map_err(Error::Rejected)?;
        Ok(file)
    }

    fn initialize(&self, lock: &mut File) -> Result<(), Error> {
        let initialized = marker(lock)?;
        let mut file = match OpenOptions::new()
            .read(true)
            .write(true)
            .open(self.journal())
        {
            Ok(mut file) if initialized => {
                check_header(&mut file)?;
                file
            }
            Ok(mut file) => {
                // Before the initialization marker is complete no record may
                // have been appended. Only repair a prefix of the header.
                let length = file.metadata().map_err(Error::Rejected)?.len();
                if length > HEADER.len() as u64 {
                    return Err(Error::Corrupt);
                }
                let mut bytes = vec![0; length as usize];
                file.read_exact(&mut bytes).map_err(Error::Rejected)?;
                if !HEADER.starts_with(&bytes) {
                    return Err(Error::Corrupt);
                }
                file.write_all(&HEADER[bytes.len()..])
                    .and_then(|()| file.sync_all())
                    .and_then(|()| sync_dir(&self.root))
                    .map_err(Error::Unknown)?;
                file
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound && initialized => {
                return Err(Error::Corrupt);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let mut options = OpenOptions::new();
                options.create_new(true).read(true).write(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                let mut file = options.open(self.journal()).map_err(Error::Rejected)?;
                file.write_all(HEADER)
                    .and_then(|()| file.sync_all())
                    .and_then(|()| sync_dir(&self.root))
                    .map_err(Error::Unknown)?;
                file
            }
            Err(error) => return Err(Error::Rejected(error)),
        };
        if !initialized {
            lock.seek(SeekFrom::Start(0)).map_err(Error::Rejected)?;
            lock.write_all(INITIALIZED).map_err(Error::Unknown)?;
        }
        // Reconfirm a prior complete marker whose sync acknowledgement was lost.
        lock.sync_all()
            .and_then(|()| sync_dir(&self.root))
            .map_err(Error::Unknown)?;

        // A writer holds the exclusive lock until an append has completed.
        // An unterminated suffix left by an interrupted writer was never an
        // acknowledged record. Validate the preceding records before trimming.
        let end = complete_end(&mut file)?;
        if end < file.metadata().map_err(Error::Rejected)?.len() {
            file.seek(SeekFrom::Start(HEADER.len() as u64))
                .map_err(Error::Rejected)?;
            let mut reader = BufReader::new((&mut file).take(end - HEADER.len() as u64));
            while let Some(line) = read_line(&mut reader)? {
                let _: Record = serde_json::from_slice(&line).map_err(|_| Error::Corrupt)?;
            }
            file.set_len(end)
                .and_then(|()| file.sync_all())
                .map_err(Error::Unknown)?;
        }
        Ok(())
    }

    pub fn register_input(&self, id: Uuid, request: TextRequest) -> Result<Registration, Error> {
        self.register_input_with(id, request, |file, data| {
            file.write_all(data)?;
            file.sync_all()
        })
    }

    fn register_input_with(
        &self,
        id: Uuid,
        request: TextRequest,
        write: impl FnOnce(&mut File, &[u8]) -> io::Result<()>,
    ) -> Result<Registration, Error> {
        if id.is_nil() || request.text.trim().is_empty() || request.text.len() > 64 * 1024 {
            return Err(Error::Invalid("a nonempty input and ID are required"));
        }
        if request.context.as_ref().is_some_and(|context| {
            context.group_id.trim().is_empty()
                || context.group_id.len() > 4096
                || context.group_title.len() > 4096
                || context
                    .scope_key
                    .as_ref()
                    .is_some_and(|key| key.trim().is_empty() || key.len() > 4096)
        }) {
            return Err(Error::Invalid("invalid Group context"));
        }
        let mut lock = self.writer()?;
        self.initialize(&mut lock)?;
        let mut found = false;
        self.scan(|record| {
            if record.id() == id {
                match record {
                    Record::Input { request: prior, .. } if prior == request && !found => {
                        found = true
                    }
                    _ => return Err(Error::IdConflict),
                }
            }
            Ok(())
        })?;
        if found {
            return self.confirm_existing();
        }
        self.append_with(
            &Record::Input {
                id,
                created_at: cccc_contracts::utc_now(),
                request,
            },
            write,
        )?;
        Ok(Registration::Inserted)
    }

    pub fn record_voice_delegation(
        &self,
        id: Uuid,
        delegation: VoiceDelegation,
    ) -> Result<Registration, Error> {
        if id.is_nil()
            || delegation.call_id.trim().is_empty()
            || delegation.call_id.len() > 128
            || delegation.delegation_id.trim().is_empty()
            || delegation.delegation_id.len() > 512
            || delegation.text.trim().is_empty()
            || delegation.text.len() > 32 * 1024
        {
            return Err(Error::Invalid("invalid Voice delegation"));
        }
        let mut lock = self.writer()?;
        self.initialize(&mut lock)?;
        let mut existing = false;
        self.scan(|record| {
            if record.id() == id {
                match record {
                    Record::VoiceDelegation {
                        delegation: prior, ..
                    } if prior == delegation && !existing => existing = true,
                    _ => return Err(Error::IdConflict),
                }
            }
            Ok(())
        })?;
        if existing {
            return self.confirm_existing();
        }
        self.append(&Record::VoiceDelegation {
            id,
            created_at: cccc_contracts::utc_now(),
            delegation,
        })?;
        Ok(Registration::Inserted)
    }

    /// Register a Group source at most once. The source fields are references
    /// supplied by the owner; this store does not authorize or inspect the
    /// Group ledger. The caller must validate them before admission.
    ///
    /// An `Existing` result is evidence of a prior record, not permission to
    /// replay model work. A caller that lost confirmation must reconcile any
    /// separate admission state before deciding what to do.
    pub fn register_group_input(&self, input: GroupInput) -> Result<(Uuid, Registration), Error> {
        self.register_group_input_with(input, |file, data| {
            file.write_all(data)?;
            file.sync_all()
        })
    }

    /// Claim one Group input before attempting Runtime admission. An uncertain
    /// append is reconciled by retrying this claim, never by resubmitting work.
    pub fn claim_group_input(&self, input_id: Uuid) -> Result<Registration, Error> {
        self.claim_group_input_with(input_id, |file, data| {
            file.write_all(data)?;
            file.sync_all()
        })
    }

    fn claim_group_input_with(
        &self,
        input_id: Uuid,
        write: impl FnOnce(&mut File, &[u8]) -> io::Result<()>,
    ) -> Result<Registration, Error> {
        if input_id.is_nil() {
            return Err(Error::Invalid("input ID is required"));
        }
        let mut lock = self.writer()?;
        self.initialize(&mut lock)?;
        let id = Uuid::new_v4();
        let mut found = false;
        let mut claimed = false;
        self.scan(|record| {
            if record.id() == id {
                return Err(Error::IdConflict);
            }
            match record {
                Record::GroupInput { id, .. } if id == input_id => found = true,
                Record::GroupDispatch {
                    input_id: prior, ..
                }
                | Record::Receipt {
                    input_id: prior, ..
                }
                | Record::NotStarted {
                    input_id: prior, ..
                } if prior == input_id => claimed = true,
                Record::Result { input_ids, .. } if input_ids.contains(&input_id) => claimed = true,
                _ => {}
            }
            Ok(())
        })?;
        if !found {
            return Err(Error::UnknownInput);
        }
        if claimed {
            return self.confirm_existing();
        }
        self.append_with(
            &Record::GroupDispatch {
                id,
                created_at: cccc_contracts::utc_now(),
                input_id,
            },
            write,
        )?;
        Ok(Registration::Inserted)
    }

    fn register_group_input_with(
        &self,
        input: GroupInput,
        write: impl FnOnce(&mut File, &[u8]) -> io::Result<()>,
    ) -> Result<(Uuid, Registration), Error> {
        let valid = |value: &str| !value.is_empty() && value.trim() == value && value.len() <= 4096;
        if !valid(&input.group_id)
            || !valid(&input.group_generation)
            || input.group_generation == "legacy:"
            || !valid(&input.event_id)
            || input
                .reply_to
                .as_ref()
                .is_some_and(|parent| !valid(parent) || parent == &input.event_id)
            || match &input.sender {
                GroupInputSender::User => false,
                GroupInputSender::Actor {
                    actor_id,
                    actor_generation,
                } => !valid(actor_id) || !valid(actor_generation) || actor_generation == "legacy:",
            }
        {
            return Err(Error::Invalid("invalid Group input source"));
        }
        let mut lock = self.writer()?;
        self.initialize(&mut lock)?;
        let id = Uuid::new_v4();
        let mut collision = false;
        let mut existing = None;
        self.scan(|record| {
            collision |= record.id() == id;
            if let Record::GroupInput {
                id, input: prior, ..
            } = record
                && prior.group_id == input.group_id
                && prior.group_generation == input.group_generation
                && prior.event_id == input.event_id
            {
                if prior != input {
                    return Err(Error::IdConflict);
                }
                if existing.replace(id).is_some() {
                    return Err(Error::Corrupt);
                }
            }
            Ok(())
        })?;
        if let Some(id) = existing {
            return Ok((id, self.confirm_existing()?));
        }
        if collision {
            return Err(Error::IdConflict);
        }
        self.append_with(
            &Record::GroupInput {
                id,
                created_at: cccc_contracts::utc_now(),
                input,
            },
            write,
        )?;
        Ok((id, Registration::Inserted))
    }

    pub fn record_result(
        &self,
        id: Uuid,
        input_ids: Vec<Uuid>,
        outcome: Outcome,
    ) -> Result<Registration, Error> {
        let inputs: HashSet<_> = input_ids.iter().copied().collect();
        if id.is_nil() || inputs.is_empty() || inputs.len() > 32 || inputs.len() != input_ids.len()
        {
            return Err(Error::Invalid("distinct input IDs are required"));
        }
        if outcome.generation.trim().is_empty()
            || outcome.turn_id.trim().is_empty()
            || outcome.status.trim().is_empty()
            || outcome.text.len() > 32 * 1024
            || outcome.error.len() > 4 * 1024
        {
            return Err(Error::Invalid("invalid result"));
        }
        let mut lock = self.writer()?;
        self.initialize(&mut lock)?;
        let mut found = HashSet::new();
        let mut existing = false;
        self.scan(|record| {
            match record {
                Record::Result {
                    id: prior_id,
                    input_ids: prior_inputs,
                    outcome: prior,
                    ..
                } if prior_id == id => {
                    if prior_inputs == input_ids && prior == outcome && !existing {
                        existing = true;
                    } else {
                        return Err(Error::IdConflict);
                    }
                }
                other if other.id() == id => return Err(Error::IdConflict),
                Record::Input { id, .. } if inputs.contains(&id) => {
                    found.insert(id);
                }
                Record::VoiceDelegation { id, .. } if inputs.contains(&id) => {
                    found.insert(id);
                }
                Record::GroupInput { id, .. } if inputs.contains(&id) => {
                    found.insert(id);
                }
                Record::Result { input_ids, .. }
                    if input_ids.iter().any(|id| inputs.contains(id)) =>
                {
                    return Err(Error::OutcomeConflict);
                }
                Record::NotStarted { input_id, .. } if inputs.contains(&input_id) => {
                    return Err(Error::OutcomeConflict);
                }
                _ => {}
            }
            Ok(())
        })?;
        if found != inputs {
            return Err(Error::UnknownInput);
        }
        if existing {
            return self.confirm_existing();
        }
        self.append(&Record::Result {
            id,
            created_at: cccc_contracts::utc_now(),
            input_ids,
            outcome,
        })?;
        Ok(Registration::Inserted)
    }

    pub fn record_receipt(
        &self,
        id: Uuid,
        input_id: Uuid,
        receipt: Receipt,
    ) -> Result<Registration, Error> {
        if id.is_nil()
            || input_id.is_nil()
            || receipt.generation.trim().is_empty()
            || receipt.turn_id.trim().is_empty()
            || receipt.generation.len() > 256
            || receipt.turn_id.len() > 256
        {
            return Err(Error::Invalid("invalid Runtime receipt"));
        }
        let mut lock = self.writer()?;
        self.initialize(&mut lock)?;
        let mut found = false;
        let mut existing = false;
        self.scan(|record| {
            match record {
                Record::Receipt {
                    id: prior_id,
                    input_id: prior_input,
                    receipt: prior,
                    ..
                } if prior_id == id => {
                    if prior_input == input_id && prior == receipt && !existing {
                        existing = true;
                    } else {
                        return Err(Error::IdConflict);
                    }
                }
                other if other.id() == id => return Err(Error::IdConflict),
                Record::Input { id, .. } if id == input_id => found = true,
                Record::VoiceDelegation { id, .. } if id == input_id => found = true,
                Record::GroupInput { id, .. } if id == input_id => found = true,
                Record::Receipt {
                    input_id: prior, ..
                } if prior == input_id => {
                    return Err(Error::ReceiptConflict);
                }
                Record::NotStarted {
                    input_id: prior, ..
                } if prior == input_id => {
                    return Err(Error::OutcomeConflict);
                }
                _ => {}
            }
            Ok(())
        })?;
        if !found {
            return Err(Error::UnknownInput);
        }
        if existing {
            return self.confirm_existing();
        }
        self.append(&Record::Receipt {
            id,
            created_at: cccc_contracts::utc_now(),
            input_id,
            receipt,
        })?;
        Ok(Registration::Inserted)
    }

    pub fn record_not_started(
        &self,
        id: Uuid,
        input_id: Uuid,
        reason: StartFailure,
    ) -> Result<Registration, Error> {
        if id.is_nil() || input_id.is_nil() {
            return Err(Error::Invalid("input and event IDs are required"));
        }
        let mut lock = self.writer()?;
        self.initialize(&mut lock)?;
        let mut found = false;
        let mut group_input = false;
        let mut existing = false;
        self.scan(|record| {
            match record {
                Record::NotStarted {
                    id: prior_id,
                    input_id: prior_input,
                    reason: prior_reason,
                    ..
                } if prior_id == id => {
                    if prior_input == input_id && prior_reason == reason && !existing {
                        existing = true;
                    } else {
                        return Err(Error::IdConflict);
                    }
                }
                other if other.id() == id => return Err(Error::IdConflict),
                Record::Input { id, .. } if id == input_id => found = true,
                Record::VoiceDelegation { id, .. } if id == input_id => found = true,
                Record::GroupInput { id, .. } if id == input_id => {
                    found = true;
                    group_input = true;
                }
                Record::GroupDispatch {
                    input_id: prior, ..
                } if prior == input_id && reason == StartFailure::SourceUnavailable => {
                    // Another owner may have claimed this input after a
                    // revalidation snapshot. Only a pre-claim source failure
                    // is known not to have reached the Runtime.
                    return Err(Error::OutcomeConflict);
                }
                Record::NotStarted {
                    input_id: prior, ..
                }
                | Record::Receipt {
                    input_id: prior, ..
                } if prior == input_id => {
                    return Err(Error::OutcomeConflict);
                }
                Record::Result { input_ids, .. } if input_ids.contains(&input_id) => {
                    return Err(Error::OutcomeConflict);
                }
                _ => {}
            }
            Ok(())
        })?;
        if !found {
            return Err(Error::UnknownInput);
        }
        if reason == StartFailure::SourceUnavailable && !group_input {
            return Err(Error::Invalid("source failure requires a Group input"));
        }
        if existing {
            return self.confirm_existing();
        }
        self.append(&Record::NotStarted {
            id,
            created_at: cccc_contracts::utc_now(),
            input_id,
            reason,
        })?;
        Ok(Registration::Inserted)
    }

    pub fn record_transcript(
        &self,
        id: Uuid,
        transcript: Transcript,
    ) -> Result<Registration, Error> {
        if id.is_nil()
            || transcript.call_id.trim().is_empty()
            || transcript.turn_id.trim().is_empty()
            || transcript.call_id.len() > 128
            || transcript.turn_id.len() > 256
            || transcript.seq == 0
            || transcript.text.len() > 32 * 1024
            || (transcript.complete && !transcript.final_update)
        {
            return Err(Error::Invalid("invalid transcript update"));
        }
        let mut lock = self.writer()?;
        self.initialize(&mut lock)?;
        let mut existing = false;
        self.scan(|record| {
            match record {
                Record::Transcript {
                    id: prior_id,
                    transcript: prior,
                    ..
                } if prior_id == id => {
                    if prior == transcript && !existing {
                        existing = true;
                    } else {
                        return Err(Error::IdConflict);
                    }
                }
                Record::Transcript {
                    transcript: prior, ..
                } if prior.call_id == transcript.call_id
                    && (prior.turn_id == transcript.turn_id || prior.seq == transcript.seq) =>
                {
                    return Err(Error::IdConflict);
                }
                other if other.id() == id => return Err(Error::IdConflict),
                _ => {}
            }
            Ok(())
        })?;
        if existing {
            return self.confirm_existing();
        }
        self.append(&Record::Transcript {
            id,
            created_at: cccc_contracts::utc_now(),
            transcript,
        })?;
        Ok(Registration::Inserted)
    }

    /// Read a bounded suffix. Opening an unused history does not create files.
    /// Only visited records are validated; older pages are checked when read.
    pub fn page(&self, before: Option<u64>, limit: usize) -> Result<Page, Error> {
        if !(1..=100).contains(&limit) {
            return Err(Error::Invalid("page limit must be 1..=100"));
        }
        let Some((_lock, mut file)) = self.reader()? else {
            return Ok(Page {
                records: vec![],
                before: None,
                head: 0,
            });
        };
        let floor = HEADER.len() as u64;
        let length = complete_end(&mut file)?;
        let mut end = before.unwrap_or(length);
        if end < floor || end > length {
            return Err(Error::Invalid("invalid history cursor"));
        }
        if before.is_some() {
            file.seek(SeekFrom::Start(end - 1))
                .map_err(Error::Rejected)?;
            let mut boundary = [0];
            file.read_exact(&mut boundary).map_err(Error::Rejected)?;
            if boundary[0] != b'\n' {
                return Err(Error::Invalid("history cursor is not a record boundary"));
            }
        }
        let mut rows = Vec::new();
        let mut bytes = 0;
        let mut more = false;
        while end > floor {
            let record_end = end;
            let (start, record) = read_previous(&mut file, floor, end)?;
            end = start;
            let record_bytes = record_end - start;
            if rows.len() == limit || (!rows.is_empty() && bytes + record_bytes > MAX_PAGE_BYTES) {
                more = true;
                break;
            }
            bytes += record_bytes;
            rows.push((start, record));
        }
        let before = if more {
            rows.last().map(|row| row.0)
        } else {
            None
        };
        rows.reverse();
        Ok(Page {
            records: rows.into_iter().map(|(_, record)| record).collect(),
            before,
            head: length,
        })
    }

    /// Read new facts after a previously returned byte boundary. The cursor
    /// advances only through records actually returned to the caller.
    pub fn after(&self, after: u64, limit: usize) -> Result<ForwardPage, Error> {
        if !(1..=100).contains(&limit) {
            return Err(Error::Invalid("page limit must be 1..=100"));
        }
        let Some((_lock, mut file)) = self.reader()? else {
            return if after == 0 {
                Ok(ForwardPage {
                    records: vec![],
                    next: 0,
                    has_more: false,
                })
            } else {
                Err(Error::Invalid("invalid history cursor"))
            };
        };
        let floor = HEADER.len() as u64;
        let length = complete_end(&mut file)?;
        let start = if after == 0 { floor } else { after };
        if start < floor || start > length {
            return Err(Error::Invalid("invalid history cursor"));
        }
        file.seek(SeekFrom::Start(start - 1))
            .map_err(Error::Rejected)?;
        let mut boundary = [0];
        file.read_exact(&mut boundary).map_err(Error::Rejected)?;
        if boundary[0] != b'\n' {
            return Err(Error::Invalid("history cursor is not a record boundary"));
        }
        file.seek(SeekFrom::Start(start)).map_err(Error::Rejected)?;
        let mut reader = BufReader::new(file.take(length - start));
        let mut records = Vec::new();
        let mut next = start;
        let mut bytes = 0_u64;
        while records.len() < limit && next < length {
            let Some(line) = read_line(&mut reader)? else {
                return Err(Error::Corrupt);
            };
            let line_len = line.len() as u64;
            if !records.is_empty() && bytes + line_len > MAX_PAGE_BYTES {
                break;
            }
            records.push(serde_json::from_slice(&line).map_err(|_| Error::Corrupt)?);
            next += line_len;
            bytes += line_len;
        }
        Ok(ForwardPage {
            records,
            next,
            has_more: next < length,
        })
    }

    /// Resolve Group inputs referenced by a page of facts with one history
    /// scan. Other input kinds are intentionally omitted.
    pub fn find_group_inputs(&self, ids: &HashSet<Uuid>) -> Result<Vec<Record>, Error> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let Some((_lock, mut file)) = self.reader()? else {
            return Ok(Vec::new());
        };
        let end = complete_end(&mut file)?;
        file.seek(SeekFrom::Start(HEADER.len() as u64))
            .map_err(Error::Rejected)?;
        let mut reader = BufReader::new(file.take(end - HEADER.len() as u64));
        let mut seen = HashSet::new();
        let mut records = Vec::new();
        while let Some(line) = read_line(&mut reader)? {
            let record: Record = serde_json::from_slice(&line).map_err(|_| Error::Corrupt)?;
            if ids.contains(&record.id()) {
                if !seen.insert(record.id()) {
                    return Err(Error::Corrupt);
                }
                if matches!(record, Record::GroupInput { .. }) {
                    records.push(record);
                }
            }
        }
        Ok(records)
    }

    /// Reconcile an uncertain submission by its original ID without replaying it.
    pub fn find_input(&self, id: Uuid) -> Result<Option<Vec<Record>>, Error> {
        if id.is_nil() {
            return Err(Error::Invalid("input ID is required"));
        }
        let Some((_lock, mut file)) = self.reader()? else {
            return Ok(None);
        };
        let end = complete_end(&mut file)?;
        file.seek(SeekFrom::Start(HEADER.len() as u64))
            .map_err(Error::Rejected)?;
        let mut reader = BufReader::new(file.take(end - HEADER.len() as u64));
        let mut found = false;
        let mut records = Vec::new();
        while let Some(line) = read_line(&mut reader)? {
            let record: Record = serde_json::from_slice(&line).map_err(|_| Error::Corrupt)?;
            let related = match &record {
                Record::Input { id: input, .. } if *input == id => {
                    if found {
                        return Err(Error::Corrupt);
                    }
                    found = true;
                    true
                }
                Record::VoiceDelegation { id: input, .. } if *input == id => {
                    if found {
                        return Err(Error::Corrupt);
                    }
                    found = true;
                    true
                }
                Record::GroupInput { id: input, .. } if *input == id => {
                    if found {
                        return Err(Error::Corrupt);
                    }
                    found = true;
                    true
                }
                Record::GroupDispatch { input_id, .. } => *input_id == id,
                Record::Receipt { input_id, .. } => *input_id == id,
                Record::NotStarted { input_id, .. } => *input_id == id,
                Record::Result { input_ids, .. } => input_ids.contains(&id),
                _ => false,
            };
            if related {
                records.push(record);
            }
        }
        if found {
            Ok(Some(records))
        } else if records.is_empty() {
            Ok(None)
        } else {
            Err(Error::Corrupt)
        }
    }

    fn reader(&self) -> Result<Option<(File, File)>, Error> {
        let mut lock = match OpenOptions::new().read(true).open(self.lock_path()) {
            Ok(lock) => lock,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                match (
                    fs::symlink_metadata(self.lock_path()),
                    fs::symlink_metadata(self.journal()),
                ) {
                    (Err(lock_error), Err(journal_error))
                        if lock_error.kind() == io::ErrorKind::NotFound
                            && journal_error.kind() == io::ErrorKind::NotFound =>
                    {
                        return Ok(None);
                    }
                    (Err(error), _) if error.kind() != io::ErrorKind::NotFound => {
                        return Err(Error::Rejected(error));
                    }
                    (_, Err(error)) if error.kind() != io::ErrorKind::NotFound => {
                        return Err(Error::Rejected(error));
                    }
                    _ => return Err(Error::Corrupt),
                }
            }
            Err(error) => return Err(Error::Rejected(error)),
        };
        FileExt::lock_shared(&lock).map_err(Error::Rejected)?;
        let initialized = marker(&mut lock)?;
        let mut file = match File::open(self.journal()) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound && !initialized => {
                return Ok(None);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Err(Error::Corrupt),
            Err(error) => return Err(Error::Rejected(error)),
        };
        if !initialized {
            let length = file.metadata().map_err(Error::Rejected)?.len();
            if length > HEADER.len() as u64 {
                return Err(Error::Corrupt);
            }
            let mut bytes = vec![0; length as usize];
            file.read_exact(&mut bytes).map_err(Error::Rejected)?;
            return if HEADER.starts_with(&bytes) {
                Ok(None)
            } else {
                Err(Error::Corrupt)
            };
        }
        check_header(&mut file)?;
        Ok(Some((lock, file)))
    }

    fn scan(&self, mut visit: impl FnMut(Record) -> Result<(), Error>) -> Result<(), Error> {
        let mut file = File::open(self.journal()).map_err(Error::Rejected)?;
        check_header(&mut file)?;
        let mut reader = BufReader::new(file);
        while let Some(line) = read_line(&mut reader)? {
            visit(serde_json::from_slice(&line).map_err(|_| Error::Corrupt)?)?;
        }
        Ok(())
    }

    fn append(&self, record: &Record) -> Result<(), Error> {
        self.append_with(record, |file, data| {
            file.write_all(data)?;
            file.sync_all()
        })
    }

    fn append_with(
        &self,
        record: &Record,
        write: impl FnOnce(&mut File, &[u8]) -> io::Result<()>,
    ) -> Result<(), Error> {
        let mut data = serde_json::to_vec(record).map_err(|_| Error::Invalid("cannot encode"))?;
        data.push(b'\n');
        if data.len() as u64 > MAX_LINE {
            return Err(Error::Invalid("history record is too large"));
        }
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(self.journal())
            .map_err(Error::Rejected)?;
        // The caller holds the exclusive history lock across this write.
        // A regular writable handle also permits rollback on Windows.
        let start = file.seek(SeekFrom::End(0)).map_err(Error::Rejected)?;
        if let Err(error) = write(&mut file, &data) {
            // A complete line may already be durable despite a failed sync.
            // Never remove it or authorize a replay. A proven partial write can
            // be rolled back while the exclusive writer lock is still held.
            let rollback = (|| -> io::Result<bool> {
                let length = file.metadata()?.len();
                if length < start || length >= start + data.len() as u64 {
                    return Ok(false);
                }
                let count = (length - start) as usize;
                file.seek(SeekFrom::Start(start))?;
                let mut suffix = vec![0; count];
                file.read_exact(&mut suffix)?;
                if suffix != data[..count] {
                    return Ok(false);
                }
                file.set_len(start)?;
                file.sync_all()?;
                sync_dir(&self.root)?;
                Ok(true)
            })();
            return Err(if matches!(rollback, Ok(true)) {
                Error::Rejected(error)
            } else {
                Error::Unknown(error)
            });
        }
        sync_dir(&self.root).map_err(Error::Unknown)
    }

    fn confirm_existing(&self) -> Result<Registration, Error> {
        OpenOptions::new()
            .append(true)
            .open(self.journal())
            .and_then(|file| file.sync_all())
            .and_then(|()| sync_dir(&self.root))
            .map_err(Error::Unknown)?;
        Ok(Registration::Existing)
    }
}

fn ensure_child(parent: &Path, name: &str) -> io::Result<()> {
    let path = parent.join(name);
    match fs::create_dir(&path) {
        Ok(()) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
            }
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists && path.is_dir() => {}
        Err(error) => return Err(error),
    }
    sync_dir(parent)
}

fn marker(lock: &mut File) -> Result<bool, Error> {
    match lock.metadata().map_err(Error::Rejected)?.len() {
        0 => Ok(false),
        len if len == INITIALIZED.len() as u64 => {
            lock.seek(SeekFrom::Start(0)).map_err(Error::Rejected)?;
            let mut bytes = vec![0; INITIALIZED.len()];
            lock.read_exact(&mut bytes).map_err(Error::Rejected)?;
            if bytes == INITIALIZED {
                Ok(true)
            } else {
                Err(Error::Corrupt)
            }
        }
        len if len < INITIALIZED.len() as u64 => {
            lock.seek(SeekFrom::Start(0)).map_err(Error::Rejected)?;
            let mut bytes = vec![0; len as usize];
            lock.read_exact(&mut bytes).map_err(Error::Rejected)?;
            if INITIALIZED.starts_with(&bytes) {
                Ok(false)
            } else {
                Err(Error::Corrupt)
            }
        }
        _ => Err(Error::Corrupt),
    }
}

/// Byte boundary of the last complete record. A reader holds a shared lock,
/// so an incomplete suffix cannot belong to a concurrently active writer.
fn complete_end(file: &mut File) -> Result<u64, Error> {
    let length = file.metadata().map_err(Error::Rejected)?.len();
    let floor = HEADER.len() as u64;
    if length < floor {
        return Err(Error::Corrupt);
    }
    file.seek(SeekFrom::Start(length - 1))
        .map_err(Error::Rejected)?;
    let mut last = [0];
    file.read_exact(&mut last).map_err(Error::Rejected)?;
    if last[0] == b'\n' {
        return Ok(length);
    }
    let start = length.saturating_sub(MAX_LINE).max(floor - 1);
    file.seek(SeekFrom::Start(start)).map_err(Error::Rejected)?;
    let mut tail = vec![0; (length - start) as usize];
    file.read_exact(&mut tail).map_err(Error::Rejected)?;
    tail.iter()
        .rposition(|byte| *byte == b'\n')
        .map(|index| start + index as u64 + 1)
        .filter(|end| *end >= floor)
        .ok_or(Error::Corrupt)
}

fn check_header(file: &mut File) -> Result<(), Error> {
    let mut header = vec![0; HEADER.len()];
    file.read_exact(&mut header).map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            Error::Corrupt
        } else {
            Error::Rejected(error)
        }
    })?;
    if header == HEADER {
        Ok(())
    } else {
        Err(Error::Corrupt)
    }
}

fn read_line(reader: &mut impl BufRead) -> Result<Option<Vec<u8>>, Error> {
    let mut line = Vec::new();
    let count = reader
        .by_ref()
        .take(MAX_LINE + 1)
        .read_until(b'\n', &mut line)
        .map_err(Error::Rejected)?;
    if count == 0 {
        return Ok(None);
    }
    if count as u64 > MAX_LINE || line.last() != Some(&b'\n') {
        return Err(Error::Corrupt);
    }
    Ok(Some(line))
}

fn read_previous(file: &mut File, floor: u64, end: u64) -> Result<(u64, Record), Error> {
    const CHUNK: u64 = 64 * 1024;
    file.seek(SeekFrom::Start(end - 1))
        .map_err(Error::Rejected)?;
    let mut delimiter = [0];
    file.read_exact(&mut delimiter).map_err(Error::Rejected)?;
    if delimiter[0] != b'\n' {
        return Err(Error::Corrupt);
    }
    let mut position = end - 1;
    let mut suffix = Vec::new();
    let start = loop {
        if position <= floor {
            break floor;
        }
        let chunk_start = position.saturating_sub(CHUNK).max(floor);
        let mut chunk = vec![0; (position - chunk_start) as usize];
        file.seek(SeekFrom::Start(chunk_start))
            .map_err(Error::Rejected)?;
        file.read_exact(&mut chunk).map_err(Error::Rejected)?;
        if let Some(index) = chunk.iter().rposition(|byte| *byte == b'\n') {
            let mut line = chunk[index + 1..].to_vec();
            line.extend_from_slice(&suffix);
            suffix = line;
            break chunk_start + index as u64 + 1;
        }
        chunk.extend_from_slice(&suffix);
        suffix = chunk;
        if suffix.len() as u64 >= MAX_LINE {
            return Err(Error::Corrupt);
        }
        position = chunk_start;
    };
    if suffix.len() as u64 >= MAX_LINE {
        return Err(Error::Corrupt);
    }
    let record = serde_json::from_slice(&suffix).map_err(|_| Error::Corrupt)?;
    Ok((start, record))
}

#[cfg(unix)]
fn sync_dir(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_dir(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, Store) {
        let temp = tempfile::tempdir().expect("temporary home");
        let store = Store::new(temp.path());
        (temp, store)
    }

    fn request(text: &str) -> TextRequest {
        TextRequest {
            text: text.into(),
            context: Some(GroupContext {
                group_id: "g_fixture".into(),
                group_title: "Fixture Group".into(),
                scope_key: None,
            }),
        }
    }

    fn group_input() -> GroupInput {
        GroupInput {
            group_id: "g_fixture".into(),
            group_generation: "group-generation".into(),
            event_id: "member-reply".into(),
            reply_to: Some("chet-request".into()),
            sender: GroupInputSender::Actor {
                actor_id: "peer".into(),
                actor_generation: "actor-generation".into(),
            },
        }
    }

    #[test]
    fn group_input_is_one_durable_input_per_group_generation_and_event() {
        let (temp, store) = fixture();
        let source = group_input();
        let (id, registration) = store
            .register_group_input(source.clone())
            .expect("register");
        assert!(!id.is_nil());
        assert_eq!(registration, Registration::Inserted);
        let reopened = Store::new(temp.path());
        assert_eq!(
            reopened
                .register_group_input(source.clone())
                .expect("retry"),
            (id, Registration::Existing)
        );
        let mut conflicting = source.clone();
        conflicting.sender = GroupInputSender::Actor {
            actor_id: "peer".into(),
            actor_generation: "replacement-actor".into(),
        };
        assert!(matches!(
            reopened.register_group_input(conflicting),
            Err(Error::IdConflict)
        ));
        let mut replacement = source;
        replacement.group_generation = "replacement-group".into();
        let (other, registration) = reopened
            .register_group_input(replacement)
            .expect("different Group generation");
        assert_ne!(other, id);
        assert_eq!(registration, Registration::Inserted);
        assert_eq!(reopened.page(None, 20).expect("history").records.len(), 2);
    }

    #[test]
    fn group_input_accepts_receipt_and_result_and_remains_reconcilable() {
        let (_temp, store) = fixture();
        let source = group_input();
        let (id, _) = store.register_group_input(source.clone()).expect("input");
        store
            .record_receipt(
                Uuid::new_v4(),
                id,
                Receipt {
                    generation: "runtime".into(),
                    turn_id: "turn".into(),
                },
            )
            .expect("receipt");
        store
            .record_result(
                Uuid::new_v4(),
                vec![id],
                Outcome {
                    generation: "runtime".into(),
                    turn_id: "turn".into(),
                    status: "completed".into(),
                    text: "handled".into(),
                    error: String::new(),
                },
            )
            .expect("result");
        let facts = store.find_input(id).expect("lookup").expect("known input");
        assert!(
            matches!(facts.first(), Some(Record::GroupInput { input, .. }) if input == &source)
        );
        assert!(matches!(facts.last(), Some(Record::Result { .. })));
        assert!(matches!(
            store.record_not_started(Uuid::new_v4(), id, StartFailure::Cancelled),
            Err(Error::OutcomeConflict)
        ));
    }

    #[test]
    fn source_unavailable_and_a_group_claim_cannot_both_be_recorded() {
        let (temp, store) = fixture();
        let text_input = Uuid::new_v4();
        store
            .register_input(text_input, request("hello"))
            .expect("text input");
        assert!(matches!(
            store.record_not_started(Uuid::new_v4(), text_input, StartFailure::SourceUnavailable),
            Err(Error::Invalid(_))
        ));
        let (claimed, _) = store.register_group_input(group_input()).expect("input");
        assert_eq!(
            store.claim_group_input(claimed).expect("claim"),
            Registration::Inserted
        );
        assert!(matches!(
            store.record_not_started(Uuid::new_v4(), claimed, StartFailure::SourceUnavailable),
            Err(Error::OutcomeConflict)
        ));
        assert_eq!(
            store
                .record_not_started(Uuid::new_v4(), claimed, StartFailure::AssistantUnavailable)
                .expect("known post-claim admission failure"),
            Registration::Inserted
        );
        let mut stopped_source = group_input();
        stopped_source.event_id = "stopped-before-claim".into();
        let (stopped, _) = store.register_group_input(stopped_source).expect("input");
        let failure_id = Uuid::new_v4();
        assert_eq!(
            store
                .record_not_started(failure_id, stopped, StartFailure::SourceUnavailable)
                .expect("source retired"),
            Registration::Inserted
        );
        assert_eq!(
            store
                .claim_group_input(stopped)
                .expect("claim after source failure"),
            Registration::Existing
        );
        assert_eq!(
            store
                .record_not_started(failure_id, stopped, StartFailure::SourceUnavailable)
                .expect("idempotent source failure"),
            Registration::Existing
        );
        for index in 0..8 {
            let mut source = group_input();
            source.event_id = format!("event-{index}");
            let (input, _) = store.register_group_input(source).expect("input");
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
            let claim_barrier = barrier.clone();
            let claim_path = temp.path().to_owned();
            let claim = std::thread::spawn(move || {
                claim_barrier.wait();
                Store::new(claim_path).claim_group_input(input)
            });
            barrier.wait();
            let failure =
                store.record_not_started(Uuid::new_v4(), input, StartFailure::SourceUnavailable);
            let claim = claim.join().expect("claim thread");
            assert!(
                matches!(
                    (claim, failure),
                    (Ok(Registration::Inserted), Err(Error::OutcomeConflict))
                        | (Ok(Registration::Existing), Ok(Registration::Inserted))
                ),
                "either the claim or the pre-claim failure wins, never both"
            );
            let facts = store.find_input(input).expect("history").expect("input");
            let claims = facts
                .iter()
                .filter(|fact| matches!(fact, Record::GroupDispatch { .. }))
                .count();
            let failures = facts
                .iter()
                .filter(|fact| matches!(fact, Record::NotStarted { .. }))
                .count();
            assert_eq!(claims + failures, 1);
        }
    }

    #[test]
    fn group_input_complete_unknown_write_reconciles_as_existing() {
        let (_temp, store) = fixture();
        let source = group_input();
        let failure = store.register_group_input_with(source.clone(), |file, bytes| {
            file.write_all(bytes)?;
            Err(io::Error::other("simulated lost confirmation"))
        });
        assert!(matches!(failure, Err(Error::Unknown(_))));
        let (id, registration) = store.register_group_input(source).expect("reconcile");
        assert_eq!(registration, Registration::Existing);
        assert!(!id.is_nil());
        assert_eq!(store.page(None, 20).expect("history").records.len(), 1);
    }

    #[test]
    fn group_input_rejects_missing_or_ambiguous_source_identity() {
        let (_temp, store) = fixture();
        for source in [
            GroupInput {
                group_generation: "".into(),
                ..group_input()
            },
            GroupInput {
                sender: GroupInputSender::Actor {
                    actor_id: "peer".into(),
                    actor_generation: "legacy:".into(),
                },
                ..group_input()
            },
            GroupInput {
                event_id: "chet-request".into(),
                ..group_input()
            },
            GroupInput {
                sender: GroupInputSender::Actor {
                    actor_id: " peer ".into(),
                    actor_generation: "actor-generation".into(),
                },
                ..group_input()
            },
        ] {
            assert!(matches!(
                store.register_group_input(source),
                Err(Error::Invalid(_))
            ));
        }
        assert!(
            store
                .page(None, 20)
                .expect("empty history")
                .records
                .is_empty()
        );
    }

    #[test]
    fn group_input_accepts_user_and_proactive_member_sources() {
        let (_temp, store) = fixture();
        let member = GroupInput {
            reply_to: None,
            ..group_input()
        };
        let (member_id, status) = store.register_group_input(member).expect("member input");
        assert_eq!(status, Registration::Inserted);
        let user = GroupInput {
            event_id: "user-message".into(),
            sender: GroupInputSender::User,
            reply_to: None,
            ..group_input()
        };
        let (user_id, status) = store.register_group_input(user).expect("user input");
        assert_eq!(status, Registration::Inserted);
        assert_ne!(member_id, user_id);
        assert_eq!(store.page(None, 20).expect("history").records.len(), 2);
    }

    #[test]
    fn group_input_accepts_legacy_generations_with_creation_times() {
        let (_temp, store) = fixture();
        let source = GroupInput {
            group_generation: "legacy:2026-10-01T00:00:00Z".into(),
            sender: GroupInputSender::Actor {
                actor_id: "peer".into(),
                actor_generation: "legacy:2026-10-02T00:00:00Z".into(),
            },
            ..group_input()
        };
        let (_, status) = store.register_group_input(source).expect("legacy source");
        assert_eq!(status, Registration::Inserted);
    }

    #[test]
    fn concurrent_group_input_registration_returns_one_id() {
        let (temp, store) = fixture();
        let home = temp.path().to_path_buf();
        let workers: Vec<_> = (0..4)
            .map(|_| {
                let home = home.clone();
                std::thread::spawn(move || Store::new(home).register_group_input(group_input()))
            })
            .collect();
        let results: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().expect("worker").expect("registration"))
            .collect();
        assert!(results.iter().all(|(id, _)| *id == results[0].0));
        assert_eq!(
            results
                .iter()
                .filter(|(_, status)| *status == Registration::Inserted)
                .count(),
            1
        );
        assert_eq!(store.page(None, 20).expect("history").records.len(), 1);
    }

    #[test]
    fn group_dispatch_claim_is_durable_and_single_owner() {
        let dir = tempfile::tempdir().expect("home");
        let store = Store::new(dir.path());
        let (input_id, _) = store.register_group_input(group_input()).expect("input");
        let threads = (0..8)
            .map(|_| {
                let home = dir.path().to_path_buf();
                std::thread::spawn(move || {
                    Store::new(home).claim_group_input(input_id).expect("claim")
                })
            })
            .collect::<Vec<_>>();
        let results = threads
            .into_iter()
            .map(|thread| thread.join().expect("thread"))
            .collect::<Vec<_>>();
        assert_eq!(
            results
                .iter()
                .filter(|value| **value == Registration::Inserted)
                .count(),
            1
        );
        assert_eq!(
            Store::new(dir.path())
                .claim_group_input(input_id)
                .expect("restart"),
            Registration::Existing
        );
        let facts = store.find_input(input_id).expect("find").expect("input");
        assert_eq!(
            facts
                .iter()
                .filter(|item| matches!(item, Record::GroupDispatch { .. }))
                .count(),
            1
        );
        assert!(matches!(
            store.claim_group_input(Uuid::new_v4()),
            Err(Error::UnknownInput)
        ));
    }

    #[test]
    fn uncertain_group_dispatch_claim_is_reconciled_without_replay() {
        let (_temp, store) = fixture();
        let (input_id, _) = store.register_group_input(group_input()).expect("input");
        let failure = store.claim_group_input_with(input_id, |file, bytes| {
            file.write_all(bytes)?;
            Err(io::Error::other("simulated lost confirmation"))
        });
        assert!(matches!(failure, Err(Error::Unknown(_))));
        assert_eq!(
            store.claim_group_input(input_id).expect("reconcile"),
            Registration::Existing
        );
    }

    #[test]
    fn reading_an_unused_history_does_not_create_it() {
        let (temp, store) = fixture();
        let page = store.page(None, 20).expect("empty history");
        assert!(page.records.is_empty());
        assert!(page.before.is_none());
        assert_eq!(page.head, 0);
        assert_eq!(store.after(page.head, 20).expect("empty updates").next, 0);
        assert!(!temp.path().join("state/chet").exists());
    }

    #[test]
    fn repeated_input_must_match_and_does_not_register_twice() {
        let (_temp, store) = fixture();
        let id = Uuid::new_v4();
        let input = request("keep the exact input");
        assert_eq!(
            store.register_input(id, input.clone()).expect("first"),
            Registration::Inserted
        );
        assert_eq!(
            store.register_input(id, input.clone()).expect("retry"),
            Registration::Existing
        );
        assert!(matches!(
            store.register_input(id, request("different")),
            Err(Error::IdConflict)
        ));
        let mut moved = input;
        moved.context.as_mut().expect("context").scope_key = Some("other".into());
        assert!(matches!(
            store.register_input(id, moved),
            Err(Error::IdConflict)
        ));
        assert_eq!(store.page(None, 20).expect("history").records.len(), 1);
    }

    #[test]
    fn unknown_write_cannot_be_retried_as_new_work() {
        let (_temp, store) = fixture();
        let id = Uuid::new_v4();
        let input = request("one operation");
        let first = store.register_input_with(id, input.clone(), |file, bytes| {
            file.write_all(bytes)?;
            Err(io::Error::other("injected lost write acknowledgement"))
        });
        assert!(matches!(first, Err(Error::Unknown(_))));
        assert_eq!(
            store.register_input(id, input).expect("retry"),
            Registration::Existing
        );
        assert_eq!(store.page(None, 20).expect("history").records.len(), 1);
    }

    #[test]
    fn partial_append_is_rolled_back_but_a_complete_unknown_write_is_preserved() {
        let (_temp, store) = fixture();
        let prior = Uuid::new_v4();
        store
            .register_input(prior, request("prior"))
            .expect("prior");
        let before = fs::read(store.journal()).expect("journal");
        let id = Uuid::new_v4();
        let failure = store.register_input_with(id, request("new"), |file, data| {
            file.write_all(&data[..data.len() / 2])?;
            Err(io::Error::other("injected partial write"))
        });
        assert!(matches!(failure, Err(Error::Rejected(_))));
        assert_eq!(fs::read(store.journal()).expect("journal"), before);
        assert_eq!(
            store.register_input(id, request("new")).expect("retry"),
            Registration::Inserted
        );
        assert_eq!(store.page(None, 10).expect("history").records.len(), 2);
    }

    #[test]
    fn interrupted_tail_leaves_history_readable_and_is_repaired_on_next_write() {
        let (_temp, store) = fixture();
        let prior = Uuid::new_v4();
        store
            .register_input(prior, request("prior"))
            .expect("prior");
        let head = store.page(None, 10).expect("page").head;
        OpenOptions::new()
            .append(true)
            .open(store.journal())
            .expect("journal")
            .write_all(b"{\"kind\":\"result\",\"incomplete\"")
            .expect("interrupted write");
        let damaged = fs::read(store.journal()).expect("damaged");
        assert_eq!(
            store.page(None, 10).expect("page after interruption").head,
            head
        );
        assert_eq!(store.after(0, 10).expect("updates").records.len(), 1);
        assert_eq!(
            store.find_input(prior).expect("find").expect("prior").len(),
            1
        );
        assert_eq!(
            fs::read(store.journal()).expect("read is nonmutating"),
            damaged
        );
        assert!(matches!(
            store.page(Some(head - 1), 10),
            Err(Error::Invalid(_))
        ));
        let next = Uuid::new_v4();
        store
            .register_input(next, request("next"))
            .expect("recover and append");
        assert_eq!(
            store
                .after(head, 10)
                .expect("updates after repair")
                .records
                .len(),
            1
        );
        let prior_facts = store.find_input(prior).expect("find").expect("prior");
        assert_eq!(prior_facts.len(), 1, "no result may be fabricated");
    }

    #[test]
    fn initialization_recovers_only_its_own_incomplete_header_and_marker() {
        for (journal, marker_bytes) in [
            (&HEADER[..0], &INITIALIZED[..0]),
            (&HEADER[..5], &INITIALIZED[..0]),
            (HEADER, &INITIALIZED[..5]),
        ] {
            let (_temp, store) = fixture();
            fs::create_dir_all(&store.root).expect("root");
            fs::write(store.journal(), journal).expect("journal prefix");
            fs::write(store.lock_path(), marker_bytes).expect("marker prefix");
            assert!(
                store
                    .page(None, 10)
                    .expect("read incomplete init")
                    .records
                    .is_empty()
            );
            store
                .register_input(Uuid::new_v4(), request("initial"))
                .expect("recover init");
            assert_eq!(store.page(None, 10).expect("history").records.len(), 1);
        }
        let (_temp, store) = fixture();
        fs::create_dir_all(&store.root).expect("root");
        fs::write(store.journal(), b"not the header").expect("invalid");
        fs::write(store.lock_path(), b"CCCC_").expect("marker prefix");
        assert!(matches!(
            store.register_input(Uuid::new_v4(), request("x")),
            Err(Error::Corrupt)
        ));
        assert_eq!(
            fs::read(store.journal()).expect("preserved"),
            b"not the header"
        );
    }

    #[test]
    fn interrupted_suffix_does_not_hide_corrupt_complete_records() {
        let (_temp, store) = fixture();
        store
            .register_input(Uuid::new_v4(), request("prior"))
            .expect("prior");
        let mut file = OpenOptions::new()
            .append(true)
            .open(store.journal())
            .expect("journal");
        file.write_all(b"not-json\n{\"partial\"").expect("damage");
        let before = fs::read(store.journal()).expect("before");
        assert!(matches!(store.page(None, 10), Err(Error::Corrupt)));
        assert!(matches!(
            store.register_input(Uuid::new_v4(), request("new")),
            Err(Error::Corrupt)
        ));
        assert_eq!(fs::read(store.journal()).expect("preserved"), before);
    }

    #[test]
    fn concurrent_same_id_registers_once() {
        let (temp, store) = fixture();
        let id = Uuid::new_v4();
        let home = temp.path().to_path_buf();
        let workers: Vec<_> = (0..4)
            .map(|_| {
                let home = home.clone();
                std::thread::spawn(move || {
                    Store::new(home).register_input(id, request("one operation"))
                })
            })
            .collect();
        let results: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().expect("worker").expect("registration"))
            .collect();
        assert_eq!(
            results
                .iter()
                .filter(|result| **result == Registration::Inserted)
                .count(),
            1
        );
        assert_eq!(
            results
                .iter()
                .filter(|result| **result == Registration::Existing)
                .count(),
            3
        );
        assert_eq!(store.page(None, 20).expect("history").records.len(), 1);
    }

    #[test]
    fn runtime_receipt_is_correlated_and_not_a_completion() {
        let (_temp, store) = fixture();
        let input = Uuid::new_v4();
        let receipt_id = Uuid::new_v4();
        let receipt = Receipt {
            generation: "fixture".into(),
            turn_id: "turn".into(),
        };
        assert!(matches!(
            store.record_receipt(receipt_id, input, receipt.clone()),
            Err(Error::UnknownInput)
        ));
        store
            .register_input(input, request("question"))
            .expect("input");
        assert_eq!(
            store
                .record_receipt(receipt_id, input, receipt.clone())
                .expect("receipt"),
            Registration::Inserted
        );
        assert_eq!(
            store
                .record_receipt(receipt_id, input, receipt.clone())
                .expect("retry"),
            Registration::Existing
        );
        assert!(matches!(
            store.record_receipt(Uuid::new_v4(), input, receipt),
            Err(Error::ReceiptConflict)
        ));
        let records = store.page(None, 20).expect("history").records;
        assert_eq!(records.len(), 2);
        assert!(
            matches!(records.last(), Some(Record::Receipt { input_id, .. }) if *input_id == input)
        );
        assert!(
            !records
                .iter()
                .any(|record| matches!(record, Record::Result { .. }))
        );
    }

    #[test]
    fn known_pre_admission_failure_is_distinct_and_cannot_replace_a_receipt() {
        let (_temp, store) = fixture();
        let input = Uuid::new_v4();
        let failure = Uuid::new_v4();
        store
            .register_input(input, request("hello"))
            .expect("input");
        assert_eq!(
            store
                .record_not_started(failure, input, StartFailure::AssistantUnavailable)
                .expect("known host failure"),
            Registration::Inserted
        );
        assert_eq!(
            store
                .record_not_started(failure, input, StartFailure::AssistantUnavailable)
                .expect("idempotent retry"),
            Registration::Existing
        );
        let records = store.find_input(input).expect("lookup").expect("input");
        assert!(
            matches!(records.last(), Some(Record::NotStarted { input_id, .. }) if *input_id == input)
        );
        let receipt = Receipt {
            generation: "g".into(),
            turn_id: "t".into(),
        };
        assert!(matches!(
            store.record_receipt(Uuid::new_v4(), input, receipt.clone()),
            Err(Error::OutcomeConflict)
        ));

        let admitted = Uuid::new_v4();
        store
            .register_input(admitted, request("admitted"))
            .expect("input");
        store
            .record_receipt(Uuid::new_v4(), admitted, receipt)
            .expect("receipt");
        assert!(matches!(
            store.record_not_started(Uuid::new_v4(), admitted, StartFailure::AssistantUnavailable),
            Err(Error::OutcomeConflict)
        ));
    }

    #[test]
    fn results_and_transcripts_are_distinct_durable_facts() {
        let (_temp, store) = fixture();
        let input = Uuid::new_v4();
        let result = Uuid::new_v4();
        let outcome = Outcome {
            generation: "fixture".into(),
            turn_id: "turn".into(),
            status: "completed".into(),
            text: "answer".into(),
            error: String::new(),
        };
        assert!(matches!(
            store.record_result(result, vec![input], outcome.clone()),
            Err(Error::UnknownInput)
        ));
        store
            .register_input(input, request("question"))
            .expect("input");
        assert!(matches!(
            store.record_result(input, vec![input], outcome.clone()),
            Err(Error::IdConflict)
        ));
        assert_eq!(
            store
                .record_result(result, vec![input], outcome.clone())
                .expect("result"),
            Registration::Inserted
        );
        assert_eq!(
            store
                .record_result(result, vec![input], outcome.clone())
                .expect("result retry"),
            Registration::Existing
        );
        assert!(matches!(
            store.record_result(Uuid::new_v4(), vec![input], outcome),
            Err(Error::OutcomeConflict)
        ));
        let transcript = Transcript {
            call_id: "call".into(),
            turn_id: "speech".into(),
            seq: 1,
            role: TranscriptRole::User,
            text: "partial".into(),
            final_update: false,
            complete: false,
        };
        store
            .record_transcript(Uuid::new_v4(), transcript.clone())
            .expect("partial transcript");
        let page = store.page(None, 20).expect("history");
        assert_eq!(page.records.len(), 3);
        assert!(
            matches!(page.records.last(), Some(Record::Transcript { transcript: saved, .. }) if saved == &transcript && !saved.complete)
        );
    }

    #[test]
    fn voice_request_is_distinct_from_spoken_text_and_accepts_runtime_facts() {
        let (_temp, store) = fixture();
        let id = Uuid::new_v4();
        let delegation = VoiceDelegation {
            call_id: "call".into(),
            delegation_id: "provider-id".into(),
            text: "inspect the project".into(),
        };
        assert_eq!(
            store
                .record_voice_delegation(id, delegation.clone())
                .expect("request"),
            Registration::Inserted
        );
        assert_eq!(
            store
                .record_voice_delegation(id, delegation)
                .expect("retry"),
            Registration::Existing
        );
        store
            .record_receipt(
                Uuid::new_v4(),
                id,
                Receipt {
                    generation: "g".into(),
                    turn_id: "t".into(),
                },
            )
            .expect("receipt");
        store
            .record_result(
                Uuid::new_v4(),
                vec![id],
                Outcome {
                    generation: "g".into(),
                    turn_id: "t".into(),
                    status: "completed".into(),
                    text: "result".into(),
                    error: String::new(),
                },
            )
            .expect("result");
        let facts = store.find_input(id).expect("lookup").expect("request");
        assert!(matches!(
            facts.first(),
            Some(Record::VoiceDelegation { .. })
        ));
        assert!(matches!(facts.last(), Some(Record::Result { .. })));
    }

    #[test]
    fn transcript_retains_in_call_order_and_marks_incomplete_observations() {
        let (_temp, store) = fixture();
        let record = Transcript {
            call_id: "call".into(),
            turn_id: "turn-a".into(),
            seq: 2,
            role: TranscriptRole::User,
            text: "observed only".into(),
            final_update: false,
            complete: false,
        };
        let id = Uuid::new_v4();
        assert_eq!(
            store
                .record_transcript(id, record.clone())
                .expect("incomplete"),
            Registration::Inserted
        );
        assert_eq!(
            store
                .record_transcript(id, record.clone())
                .expect("same receipt"),
            Registration::Existing
        );
        let mut collision = record.clone();
        collision.turn_id = "turn-b".into();
        assert!(matches!(
            store.record_transcript(Uuid::new_v4(), collision),
            Err(Error::IdConflict)
        ));
        let mut wrong = record;
        wrong.complete = true;
        assert!(matches!(
            store.record_transcript(Uuid::new_v4(), wrong),
            Err(Error::Invalid(_))
        ));
    }

    #[test]
    fn pages_return_history_in_order_without_skipping_damage() {
        let (_temp, store) = fixture();
        let ids: Vec<_> = (0..3).map(|_| Uuid::new_v4()).collect();
        for (index, id) in ids.iter().enumerate() {
            store
                .register_input(*id, request(&format!("input {index}")))
                .expect("input");
        }
        let latest = store.page(None, 2).expect("latest page");
        assert_eq!(
            latest.records.iter().map(Record::id).collect::<Vec<_>>(),
            ids[1..]
        );
        let earlier = store.page(latest.before, 2).expect("earlier page");
        assert_eq!(
            earlier.records.iter().map(Record::id).collect::<Vec<_>>(),
            ids[..1]
        );
        assert!(earlier.before.is_none());
        let mut file = OpenOptions::new()
            .append(true)
            .open(store.journal())
            .expect("journal");
        file.write_all(b"{\"kind\":\"input\"}\n")
            .expect("inject malformed complete record");
        assert!(matches!(store.page(None, 20), Err(Error::Corrupt)));
        assert!(matches!(
            store.register_input(Uuid::new_v4(), request("next")),
            Err(Error::Corrupt)
        ));
    }

    #[test]
    fn initial_head_and_forward_pages_do_not_miss_new_records() {
        let (_temp, store) = fixture();
        let first = Uuid::new_v4();
        store
            .register_input(first, request("first"))
            .expect("first");
        let initial = store.page(None, 20).expect("initial page");
        let second = Uuid::new_v4();
        let third = Uuid::new_v4();
        store
            .register_input(second, request("second"))
            .expect("second");
        store
            .register_input(third, request("third"))
            .expect("third");
        let update = store.after(initial.head, 1).expect("first update");
        assert_eq!(
            update.records.iter().map(Record::id).collect::<Vec<_>>(),
            vec![second]
        );
        assert!(update.has_more);
        let next = store.after(update.next, 1).expect("second update");
        assert_eq!(
            next.records.iter().map(Record::id).collect::<Vec<_>>(),
            vec![third]
        );
        assert!(!next.has_more);
        let found = store
            .find_input(second)
            .expect("lookup")
            .expect("record exists");
        assert_eq!(
            found.iter().map(Record::id).collect::<Vec<_>>(),
            vec![second]
        );
        assert!(
            store
                .find_input(Uuid::new_v4())
                .expect("lookup missing")
                .is_none()
        );
    }

    #[test]
    fn a_missing_initialized_journal_is_not_an_empty_history() {
        let (_temp, store) = fixture();
        store
            .register_input(Uuid::new_v4(), request("saved"))
            .expect("input");
        fs::remove_file(store.journal()).expect("simulate missing journal");
        assert!(matches!(store.page(None, 20), Err(Error::Corrupt)));
        assert!(matches!(
            store.register_input(Uuid::new_v4(), request("next")),
            Err(Error::Corrupt)
        ));
    }
}
