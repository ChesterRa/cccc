use cccc_contracts::utc_now;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::io;
use std::path::PathBuf;
use uuid::Uuid;

use crate::fs::{read_json, write_json};
use crate::{GroupStore, HomeLayout};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub schema: String,
    pub sid: String,
    pub group_id: String,
    pub title: String,
    pub brief: String,
    pub reply_to_event_id: String,
    pub recipient: String,
    pub scope_key: String,
    pub allowed_paths: Vec<String>,
    pub created_at: String,
    pub expires_at: String,
    pub secret_sha256: String,
    #[serde(default)]
    pub revoked_at: String,
    #[serde(default)]
    pub sent_message_ids: BTreeSet<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Created {
    #[serde(flatten)]
    pub session: Session,
    pub secret: String,
}

pub struct CreateSpec {
    pub group_id: String,
    pub title: String,
    pub brief: String,
    pub reply_to_event_id: String,
    pub recipient: String,
    pub scope_key: String,
    pub allowed_paths: Vec<String>,
    pub expires_in_seconds: i64,
}

#[derive(Debug, Clone)]
pub struct Store {
    home: HomeLayout,
}

impl Store {
    pub fn new(home: HomeLayout) -> io::Result<Self> {
        home.initialize().map_err(io::Error::other)?;
        std::fs::create_dir_all(home.root().join("state/nomcp_sessions"))?;
        Ok(Self { home })
    }

    pub fn create(&self, spec: CreateSpec) -> io::Result<Created> {
        let group = GroupStore::new(self.home.clone())?.load(&spec.group_id)?;
        let effective_scope = if spec.scope_key.trim().is_empty() {
            group.active_scope_key
        } else if group
            .scopes
            .iter()
            .any(|scope| scope.scope_key == spec.scope_key)
        {
            spec.scope_key
        } else {
            return Err(io::Error::other("scope is not attached to group"));
        };
        if effective_scope.is_empty() {
            return Err(io::Error::other("group has no active scope"));
        }
        let sid = format!("nomcp_{}", &Uuid::new_v4().simple().to_string()[..16]);
        let secret = format!("nomcps_{}", Uuid::new_v4().simple());
        let now = chrono::Utc::now();
        let session = Session {
            schema: "cccc.nomcp.session.v1".into(),
            sid: sid.clone(),
            group_id: spec.group_id,
            title: spec.title.trim().into(),
            brief: spec.brief.trim().into(),
            reply_to_event_id: spec.reply_to_event_id.trim().into(),
            recipient: spec.recipient.trim().into(),
            scope_key: effective_scope,
            allowed_paths: normalize_paths(spec.allowed_paths)?,
            created_at: utc_now(),
            expires_at: (now
                + chrono::Duration::seconds(spec.expires_in_seconds.clamp(60, 604_800)))
            .to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
            secret_sha256: digest(&secret),
            revoked_at: String::new(),
            sent_message_ids: BTreeSet::new(),
        };
        self.save(&session)?;
        Ok(Created { session, secret })
    }

    pub fn list(&self) -> io::Result<Vec<Session>> {
        let mut sessions: Vec<Session> = Vec::new();
        for entry in std::fs::read_dir(self.dir())?.filter_map(Result::ok) {
            if entry.path().extension().and_then(|value| value.to_str()) == Some("json")
                && let Ok(session) = read_json(entry.path().as_path())
            {
                sessions.push(session);
            }
        }
        sessions.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        Ok(sessions)
    }

    pub fn get(&self, sid: &str) -> io::Result<Option<Session>> {
        validate_sid(sid)?;
        let path = self.path(sid);
        if path.exists() {
            read_json(&path).map(Some)
        } else {
            Ok(None)
        }
    }

    pub fn authorize(&self, sid: &str, secret: &str) -> io::Result<Session> {
        let session = self
            .get(sid)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "session not found"))?;
        if !session.revoked_at.is_empty() {
            return Err(io::Error::other("session revoked"));
        }
        let expires =
            chrono::DateTime::parse_from_rfc3339(&session.expires_at).map_err(io::Error::other)?;
        if expires < chrono::Utc::now() {
            return Err(io::Error::other("session expired"));
        }
        if digest(secret) != session.secret_sha256 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "invalid session secret",
            ));
        }
        Ok(session)
    }

    pub fn revoke(&self, sid: &str) -> io::Result<bool> {
        let Some(mut session) = self.get(sid)? else {
            return Ok(false);
        };
        session.revoked_at = utc_now();
        self.save(&session)?;
        Ok(true)
    }

    pub fn record_message(&self, sid: &str, message_id: &str) -> io::Result<bool> {
        let mut session = self
            .get(sid)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "session not found"))?;
        if !session.sent_message_ids.insert(message_id.into()) {
            return Ok(false);
        }
        self.save(&session)?;
        Ok(true)
    }

    fn save(&self, session: &Session) -> io::Result<()> {
        write_json(&self.path(&session.sid), session)
    }

    fn path(&self, sid: &str) -> PathBuf {
        self.dir().join(format!("{sid}.json"))
    }

    fn dir(&self) -> PathBuf {
        self.home.root().join("state/nomcp_sessions")
    }
}

fn normalize_paths(paths: Vec<String>) -> io::Result<Vec<String>> {
    let mut output = Vec::new();
    for raw in paths {
        let path = raw.trim().trim_matches('/');
        if path.is_empty() {
            continue;
        }
        if std::path::Path::new(path).is_absolute() || path.split('/').any(|part| part == "..") {
            return Err(io::Error::other("allowed_paths must be relative"));
        }
        if !output.iter().any(|item| item == path) {
            output.push(path.into());
        }
    }
    Ok(output)
}

fn validate_sid(sid: &str) -> io::Result<()> {
    if sid.starts_with("nomcp_")
        && sid
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        Ok(())
    } else {
        Err(io::Error::other("invalid session id"))
    }
}

fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
