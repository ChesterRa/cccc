//! Durable evidence for Unix owned-process recovery.
//!
//! The object envelope makes old readers fail closed rather than applying the
//! former seconds-level heuristic to records carrying stronger evidence.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(super) struct LinuxOrigin {
    pub(super) boot_id: String,
    pub(super) pid_namespace: u64,
    /// `None` means a checked procfs lacked the time-namespace facility.
    /// An unreadable facility is not represented as `None`.
    pub(super) time_namespace: Option<u64>,
}

impl LinuxOrigin {
    pub(super) fn valid(&self) -> bool {
        uuid::Uuid::parse_str(&self.boot_id)
            .is_ok_and(|id| !id.is_nil() && id.to_string() == self.boot_id)
            && self.pid_namespace != 0
            && self.time_namespace != Some(0)
    }
}

/// Linux keeps its existing flat encoding. An explicit platform marker is
/// required for Darwin, so a missing or future marker cannot guess an origin.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum Origin {
    Linux(LinuxOrigin),
    Darwin { boot_id: String },
}

impl Origin {
    pub(super) fn same_platform(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Self::Linux(_), Self::Linux(_)) | (Self::Darwin { .. }, Self::Darwin { .. })
        )
    }

    pub(super) fn boot_id(&self) -> &str {
        match self {
            Self::Linux(origin) => &origin.boot_id,
            Self::Darwin { boot_id } => boot_id,
        }
    }

    pub(super) fn valid(&self) -> bool {
        match self {
            Self::Linux(origin) => origin.valid(),
            Self::Darwin { boot_id } => uuid::Uuid::parse_str(boot_id)
                .is_ok_and(|id| !id.is_nil() && id.to_string() == *boot_id),
        }
    }

    pub(super) fn linux(&self) -> Option<&LinuxOrigin> {
        match self {
            Self::Linux(origin) => Some(origin),
            Self::Darwin { .. } => None,
        }
    }

    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn linux_mut(&mut self) -> &mut LinuxOrigin {
        match self {
            Self::Linux(origin) => origin,
            Self::Darwin { .. } => panic!("expected a Linux fixture origin"),
        }
    }
}

impl Serialize for Origin {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Linux(origin) => origin.serialize(serializer),
            Self::Darwin { boot_id } => {
                use serde::ser::SerializeStruct;
                let mut value = serializer.serialize_struct("DarwinOrigin", 2)?;
                value.serialize_field("platform", "darwin")?;
                value.serialize_field("boot_id", boot_id)?;
                value.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for Origin {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value.get("platform") {
            None => serde_json::from_value(value)
                .map(Self::Linux)
                .map_err(serde::de::Error::custom),
            Some(serde_json::Value::String(platform)) if platform == "darwin" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct DarwinOrigin {
                    platform: String,
                    boot_id: String,
                }
                let origin: DarwinOrigin =
                    serde_json::from_value(value).map_err(serde::de::Error::custom)?;
                if origin.platform != "darwin" {
                    return Err(serde::de::Error::custom("invalid Darwin origin"));
                }
                Ok(Self::Darwin {
                    boot_id: origin.boot_id,
                })
            }
            Some(_) => Err(serde::de::Error::custom("unknown process origin platform")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(super) struct CapturedIdentity {
    pub(super) origin: Origin,
    pub(super) start_ticks: u64,
    pub(super) strength: IdentityStrength,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DarwinIdentity {
    pub(super) origin: Origin,
    pub(super) pid: u32,
    pub(super) uid: u32,
    pub(super) started_seconds: u64,
    pub(super) started_microseconds: u32,
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
    DarwinCaptured(DarwinIdentity),
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
                                && identity.origin.linux().is_some()
                                && group.origin.as_ref() == Some(&identity.origin)
                                && !matches!(
                                    identity.strength,
                                    IdentityStrength::Pidfs { inode: 0 }
                                )
                        }
                        GroupEvidence::DarwinCaptured(identity) => {
                            matches!(identity.origin, Origin::Darwin { .. })
                                && identity.origin.valid()
                                && group.origin.as_ref() == Some(&identity.origin)
                                && i32::try_from(identity.pid).ok() == Some(group.pgid)
                                && identity.started_seconds != 0
                                && identity.started_microseconds < 1_000_000
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
    fn a_linux_guard_ledger_keeps_its_original_bytes() {
        let original = br#"{"version":2,"records":[{"type":"process_group","evidence":{"id":"12345678-1234-4234-8234-123456789abc","pgid":42,"spawned_at":123,"observed_at":124,"origin":{"boot_id":"12345678-1234-4234-8234-123456789abc","pid_namespace":7,"time_namespace":null},"evidence":{"kind":"captured","identity":{"origin":{"boot_id":"12345678-1234-4234-8234-123456789abc","pid_namespace":7,"time_namespace":null},"start_ticks":17,"strength":{"kind":"pidfs","inode":19}}}}}]}"#;
        let ledger = Ledger::from_bytes(original).expect("existing Linux ledger");
        assert_eq!(ledger.to_disk_bytes().expect("same encoding"), original);
    }

    #[test]
    fn process_origins_require_an_explicit_known_platform() {
        let boot = "12345678-1234-4234-8234-123456789abc";
        let darwin = Origin::Darwin {
            boot_id: boot.into(),
        };
        let value = serde_json::to_value(&darwin).expect("Darwin origin");
        assert_eq!(
            value,
            serde_json::json!({"platform":"darwin", "boot_id":boot})
        );
        assert_eq!(
            serde_json::from_value::<Origin>(value).expect("explicit platform"),
            darwin
        );
        for value in [
            serde_json::json!({"boot_id":boot}),
            serde_json::json!({"platform":"future", "boot_id":boot, "pid_namespace":7}),
            serde_json::json!({"platform":null, "boot_id":boot, "pid_namespace":7}),
            serde_json::json!({"platform":"darwin", "boot_id":boot, "pid_namespace":0}),
        ] {
            assert!(serde_json::from_value::<Origin>(value).is_err());
        }
    }

    #[test]
    fn darwin_group_evidence_cannot_borrow_a_linux_origin_or_another_group() {
        let origin = Origin::Darwin {
            boot_id: "12345678-1234-4234-8234-123456789abc".into(),
        };
        let mut group = GroupRecord {
            id: uuid::Uuid::new_v4(),
            source_id: None,
            pgid: 42,
            spawned_at: 123,
            observed_at: 124,
            origin: Some(origin.clone()),
            evidence: GroupEvidence::DarwinCaptured(DarwinIdentity {
                origin,
                pid: 42,
                uid: 501,
                started_seconds: 123,
                started_microseconds: 999_999,
            }),
        };
        assert!(Record::ProcessGroup(group.clone()).valid());
        let mut foreign = group.clone();
        let linux = Origin::Linux(LinuxOrigin {
            boot_id: "12345678-1234-4234-8234-123456789abc".into(),
            pid_namespace: 7,
            time_namespace: None,
        });
        foreign.origin = Some(linux.clone());
        let GroupEvidence::DarwinCaptured(identity) = &mut foreign.evidence else {
            unreachable!()
        };
        identity.origin = linux;
        assert!(!Record::ProcessGroup(foreign).valid());
        let GroupEvidence::DarwinCaptured(identity) = &mut group.evidence else {
            unreachable!()
        };
        identity.started_microseconds = 1_000_000;
        assert!(!Record::ProcessGroup(group.clone()).valid());
        let GroupEvidence::DarwinCaptured(identity) = &mut group.evidence else {
            unreachable!()
        };
        identity.started_microseconds = 0;
        group.pgid = 43;
        assert!(!Record::ProcessGroup(group).valid());
    }

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
