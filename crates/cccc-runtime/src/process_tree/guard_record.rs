//! Durable evidence for Unix owned-process recovery.
//!
//! The object envelope makes old readers fail closed rather than applying the
//! former seconds-level heuristic to records carrying stronger evidence.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(super) struct Origin {
    pub(super) boot_id: String,
    pub(super) pid_namespace: u64,
    /// `None` means a checked procfs lacked the time-namespace facility.
    /// An unreadable facility is not represented as `None`.
    pub(super) time_namespace: Option<u64>,
}

impl Origin {
    pub(super) fn valid(&self) -> bool {
        uuid::Uuid::parse_str(&self.boot_id)
            .is_ok_and(|id| !id.is_nil() && id.to_string() == self.boot_id)
            && self.pid_namespace != 0
            && self.time_namespace != Some(0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(super) struct CapturedIdentity {
    pub(super) origin: Origin,
    pub(super) start_ticks: u64,
    pub(super) strength: IdentityStrength,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum IdentityStrength {
    Pidfs { inode: u64 },
    Approximate { reason: UnavailableReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum UnavailableReason {
    PidfdUnavailable,
    PidfdDenied,
    LegacyPidfd,
}

/// Legacy records may use the explicitly accepted upgrade policy. A failed
/// new capture must never be relabelled legacy to get that policy.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "identity", rename_all = "snake_case")]
pub(super) enum GroupEvidence {
    Captured(CapturedIdentity),
    Legacy,
    Unconfirmed,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(super) struct LaunchIntent {
    pub(super) id: uuid::Uuid,
    /// Original ID when an interrupted snapshot had a conflicting ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) source_id: Option<uuid::Uuid>,
    pub(super) created_at: u64,
    pub(super) origin: Option<Origin>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(super) struct GroupRecord {
    pub(super) id: uuid::Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) source_id: Option<uuid::Uuid>,
    pub(super) pgid: i32,
    pub(super) spawned_at: u64,
    pub(super) observed_at: u64,
    /// Scope captured independently before launch; absent is unknown.
    pub(super) origin: Option<Origin>,
    pub(super) evidence: GroupEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", content = "evidence", rename_all = "snake_case")]
pub(super) enum Record {
    LaunchIntent(LaunchIntent),
    ProcessGroup(GroupRecord),
}

impl Record {
    pub(super) fn id(&self) -> uuid::Uuid {
        match self {
            Self::LaunchIntent(intent) => intent.id,
            Self::ProcessGroup(group) => group.id,
        }
    }

    pub(super) fn set_id(&mut self, id: uuid::Uuid) {
        match self {
            Self::LaunchIntent(intent) => intent.id = id,
            Self::ProcessGroup(group) => group.id = id,
        }
    }

    fn source_id(&self) -> Option<uuid::Uuid> {
        match self {
            Self::LaunchIntent(intent) => intent.source_id,
            Self::ProcessGroup(group) => group.source_id,
        }
    }

    pub(super) fn original(&self) -> Self {
        let mut record = self.clone();
        if let Some(id) = record.source_id() {
            record.set_id(id);
            record.set_source_id(None);
        }
        record
    }

    pub(super) fn set_source_id(&mut self, source_id: Option<uuid::Uuid>) {
        match self {
            Self::LaunchIntent(intent) => intent.source_id = source_id,
            Self::ProcessGroup(group) => group.source_id = source_id,
        }
    }

    pub(super) fn valid(&self) -> bool {
        if self.id().is_nil() {
            return false;
        }
        if self
            .source_id()
            .is_some_and(|source| source.is_nil() || source == self.id())
        {
            return false;
        }
        match self {
            Self::LaunchIntent(intent) => {
                intent.created_at != 0 && intent.origin.as_ref().is_none_or(Origin::valid)
            }
            Self::ProcessGroup(group) => {
                group.pgid > 1
                    && group.spawned_at != 0
                    && group.observed_at != 0
                    && group.origin.as_ref().is_none_or(Origin::valid)
                    && match &group.evidence {
                        GroupEvidence::Captured(identity) => {
                            identity.origin.valid()
                                && group.origin.as_ref() == Some(&identity.origin)
                                && !matches!(
                                    identity.strength,
                                    IdentityStrength::Pidfs { inode: 0 }
                                )
                        }
                        GroupEvidence::Legacy => group.origin.is_none(),
                        GroupEvidence::Unconfirmed => true,
                    }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Ledger {
    version: u8,
    pub(super) records: Vec<Record>,
}

impl Ledger {
    pub(super) fn new(records: Vec<Record>) -> Self {
        Self {
            version: 2,
            records,
        }
    }

    pub(super) fn from_bytes(bytes: &[u8]) -> io::Result<Self> {
        let ledger: Self = serde_json::from_slice(bytes).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid guard ledger: {error}"),
            )
        })?;
        ledger.validate()?;
        Ok(ledger)
    }

    /// An empty record set has no versioned evidence; older readers can safely
    /// read the empty array. Nonempty records must remain versioned so an older
    /// reader cannot silently discard an intent or interpret stronger evidence.
    pub(super) fn to_disk_bytes(&self) -> io::Result<Vec<u8>> {
        self.validate()?;
        if self.records.is_empty() {
            Ok(b"[]".to_vec())
        } else {
            serde_json::to_vec(self).map_err(io::Error::other)
        }
    }

    pub(super) fn validate(&self) -> io::Result<()> {
        if self.version != 2 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported guard ledger version",
            ));
        }
        let mut ids = HashSet::new();
        if self
            .records
            .iter()
            .any(|record| !record.valid() || !ids.insert(record.id()))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid or duplicate guard record",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versioned_intent_roundtrips_and_old_vector_reader_rejects_it() {
        let intent = LaunchIntent {
            id: uuid::Uuid::new_v4(),
            source_id: None,
            created_at: 123,
            origin: None,
        };
        let ledger = Ledger::new(vec![Record::LaunchIntent(intent)]);
        let bytes = serde_json::to_vec(&ledger).expect("serialize ledger");
        assert_eq!(Ledger::from_bytes(&bytes).expect("read ledger"), ledger);
        assert!(serde_json::from_slice::<Vec<serde_json::Value>>(&bytes).is_err());
    }

    #[test]
    fn unknown_version_or_record_is_not_silently_discarded() {
        for bytes in [
            br#"{"version":3,"records":[]}"#.as_slice(),
            br#"{"version":2,"records":[{"type":"future","evidence":{}}]}"#.as_slice(),
            br#"{"version":2,"records":null}"#.as_slice(),
        ] {
            assert!(Ledger::from_bytes(bytes).is_err());
        }
    }

    #[test]
    fn invalid_or_duplicate_identity_is_preserved_as_an_error() {
        let intent = LaunchIntent {
            id: uuid::Uuid::new_v4(),
            source_id: None,
            created_at: 123,
            origin: None,
        };
        let duplicate = Ledger::new(vec![
            Record::LaunchIntent(intent.clone()),
            Record::LaunchIntent(intent),
        ]);
        let bytes = serde_json::to_vec(&duplicate).expect("serialize");
        assert!(Ledger::from_bytes(&bytes).is_err());

        let invalid = Ledger::new(vec![Record::ProcessGroup(GroupRecord {
            id: uuid::Uuid::new_v4(),
            source_id: None,
            pgid: 1,
            spawned_at: 123,
            observed_at: 124,
            origin: None,
            evidence: GroupEvidence::Legacy,
        })]);
        let bytes = serde_json::to_vec(&invalid).expect("serialize");
        assert!(Ledger::from_bytes(&bytes).is_err());
    }
}
