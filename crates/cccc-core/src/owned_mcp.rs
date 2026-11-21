//! Registration of a managed member's main MCP binding with its session owner.
//! The owner, rather than the replaceable daemon, retains these observations.
//! Binding identifiers associate a launch; they never grant tool authority.

use crate::{GroupDoc, GroupStore, HomeLayout};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

pub const BINDING_FILE_ENV: &str = "CCCC_MCP_BINDING_FILE";
/// New MCP processes must still register with a retained older owner. Keep
/// these version-one request/result meanings stable and ignore added fields.
pub const REGISTRATION_API: u32 = 1;
const MAX_BINDING_BYTES: usize = 4096;

/// Native sessions retain their original MCP descriptor across exact resume.
/// Its locator must outlive any one owner or launch, without being reused by a
/// recreated member or an imported Group with a new resource generation.
pub fn binding_file(
    home: &HomeLayout,
    group: &GroupDoc,
    actor_id: &str,
    actor_created_at: &str,
) -> io::Result<PathBuf> {
    let state = GroupStore::new(home.clone())?.state_dir(&group.group_id)?;
    let identity =
        serde_json::to_vec(&(&group.generation, actor_id, actor_created_at)).map_err(invalid)?;
    let name = format!("{:x}.json", Sha256::digest(identity));
    Ok(state.join("mcp-bindings").join(name))
}

/// Removal happens after the Actor removal commits; Stop retains the locator
/// so an exact resume can use the same descriptor with newly published data.
pub fn remove_binding_file(
    home: &HomeLayout,
    group: &GroupDoc,
    actor_id: &str,
    actor_created_at: &str,
) -> io::Result<()> {
    match std::fs::remove_file(binding_file(home, group, actor_id, actor_created_at)?) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub owner_id: String,
    pub instance: String,
    pub launch_id: String,
}

impl Binding {
    pub fn validate(&self) -> io::Result<()> {
        for value in [&self.owner_id, &self.instance, &self.launch_id] {
            if !valid_id(value) {
                return Err(invalid("invalid owned MCP binding identifier"));
            }
        }
        Ok(())
    }

    pub fn parse(value: &str) -> io::Result<Self> {
        if value.len() > MAX_BINDING_BYTES {
            return Err(invalid("owned MCP binding exceeds its size limit"));
        }
        let binding: Self = serde_json::from_str(value).map_err(invalid)?;
        binding.validate()?;
        Ok(binding)
    }

    pub fn encoded(&self) -> io::Result<String> {
        self.validate()?;
        serde_json::to_string(self).map_err(invalid)
    }

    /// Read once, before initialize. A missing or damaged controlled binding
    /// is a registration failure, never permission to become an unbound MCP.
    pub fn read_file(path: &Path) -> io::Result<Self> {
        let read = || {
            if !path.is_absolute() || !std::fs::symlink_metadata(path)?.is_file() {
                return Err(invalid("expected an absolute regular binding file"));
            }
            let mut value = String::new();
            std::fs::File::open(path)?
                .take(MAX_BINDING_BYTES as u64 + 1)
                .read_to_string(&mut value)?;
            Self::parse(&value)
        };
        read().map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("cannot read CCCC MCP binding {}: {error}", path.display()),
            )
        })
    }

    pub fn socket(&self, home: &HomeLayout) -> io::Result<PathBuf> {
        self.validate()?;
        Ok(home
            .daemon_dir()
            .join("session-owners")
            .join(&self.owner_id)
            .join("control.sock"))
    }
}

/// A report describes the MCP image actually answering initialize. The
/// receiving socket supplies native process identity; no PID is self-reported.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registration {
    pub registration: u32,
    pub instance: String,
    pub launch_id: String,
    pub version: String,
    pub source_id: String,
    pub mcp_contract: u32,
}

impl Registration {
    pub fn current(binding: &Binding) -> io::Result<Self> {
        binding.validate()?;
        let report = Self {
            registration: REGISTRATION_API,
            instance: binding.instance.clone(),
            launch_id: binding.launch_id.clone(),
            version: env!("CARGO_PKG_VERSION").into(),
            source_id: crate::build_info::source_id().unwrap_or_default().into(),
            mcp_contract: cccc_contracts::MCP_DAEMON_CONTRACT,
        };
        report.validate()?;
        Ok(report)
    }

    pub fn validate(&self) -> io::Result<()> {
        if self.registration != REGISTRATION_API
            || !valid_id(&self.instance)
            || !valid_id(&self.launch_id)
            || self.version.is_empty()
            || self.version.len() > 128
            || !self
                .version
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b".-+".contains(&byte))
            || self.source_id.len() != 64
            || !self.source_id.bytes().all(|byte| byte.is_ascii_hexdigit())
            || self.mcp_contract == 0
        {
            return Err(invalid("incomplete or incompatible owned MCP registration"));
        }
        Ok(())
    }
}

/// These are the registration subset of the owner's private protocol. Its
/// ordinary lifecycle/control requests remain in the owner implementation.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum RegistrationRequest {
    McpRegister { report: Registration },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum RegistrationResponse {
    McpRegistered {
        registration: u32,
        instance: String,
        launch_id: String,
    },
    Failed {
        error: RegistrationFailure,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RegistrationFailure {
    pub code: String,
    pub message: String,
}

fn valid_id(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| !id.is_nil() && id.simple().to_string() == value)
}

fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_addresses_are_derived_from_canonical_ids_not_caller_paths() {
        let binding = Binding {
            owner_id: uuid::Uuid::new_v4().simple().to_string(),
            instance: uuid::Uuid::new_v4().simple().to_string(),
            launch_id: uuid::Uuid::new_v4().simple().to_string(),
        };
        let temp = tempfile::tempdir().expect("isolated home");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let encoded = binding.encoded().expect("binding JSON");
        let mut additive: serde_json::Value = serde_json::from_str(&encoded).expect("JSON");
        additive["future_field"] = serde_json::json!({"ignored":true});
        let parsed = Binding::parse(&additive.to_string()).expect("API 1 permits additions");
        assert_eq!(
            parsed.socket(&home).expect("derived socket"),
            home.daemon_dir()
                .join("session-owners")
                .join(&binding.owner_id)
                .join("control.sock")
        );
        for bad in [
            "../../other",
            "00000000000000000000000000000000",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        ] {
            let mut invalid = binding.clone();
            invalid.owner_id = bad.into();
            assert!(invalid.socket(&home).is_err());
        }
        assert!(Binding::parse(&" ".repeat(MAX_BINDING_BYTES + 1)).is_err());
    }

    #[test]
    fn locators_survive_renaming_but_not_recreation_or_group_import() {
        let temp = tempfile::tempdir().expect("isolated home");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let mut group: GroupDoc = serde_json::from_value(serde_json::json!({
            "v":1,"group_id":"g_locator","generation":"resource-one",
            "title":"Before rename","created_at":"created","updated_at":"created"
        }))
        .expect("group fixture");
        let file = binding_file(&home, &group, "peer", "first creation").expect("locator");
        group.title = "After rename".into();
        assert_eq!(
            binding_file(&home, &group, "peer", "first creation").expect("renamed"),
            file
        );
        assert_ne!(
            binding_file(&home, &group, "peer", "second creation").expect("recreated"),
            file
        );
        group.generation = "resource-two".into();
        assert_ne!(
            binding_file(&home, &group, "peer", "first creation").expect("imported"),
            file
        );
        group.group_id = "../elsewhere".into();
        assert!(binding_file(&home, &group, "peer", "created").is_err());
    }

    #[test]
    fn controlled_binding_files_are_private_bounded_and_never_optional() {
        let temp = tempfile::tempdir().expect("isolated state");
        let file = temp.path().join("binding.json");
        assert_eq!(
            Binding::read_file(&file)
                .expect_err("missing is not unbound")
                .kind(),
            io::ErrorKind::NotFound
        );
        assert!(
            Binding::read_file(temp.path()).is_err(),
            "a directory is not a binding"
        );
        std::fs::write(&file, "not JSON").expect("damaged fixture");
        assert!(Binding::read_file(&file).is_err());
        std::fs::write(&file, " ".repeat(MAX_BINDING_BYTES + 1)).expect("oversized fixture");
        assert!(Binding::read_file(&file).is_err());
        let binding = Binding {
            owner_id: uuid::Uuid::new_v4().simple().to_string(),
            instance: uuid::Uuid::new_v4().simple().to_string(),
            launch_id: uuid::Uuid::new_v4().simple().to_string(),
        };
        crate::fs::write_secret_json(&file, &binding).expect("atomic publication");
        assert_eq!(
            Binding::read_file(&file).expect("complete binding"),
            binding
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&file)
                    .expect("permissions")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
            let link = temp.path().join("alias");
            std::os::unix::fs::symlink(&file, &link).expect("symlink fixture");
            assert!(Binding::read_file(&link).is_err());
        }
    }
}
