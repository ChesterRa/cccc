use cccc_contracts::utc_now;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
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
    #[serde(default)]
    pub updated_at: String,
    pub expires_at: String,
    #[serde(default)]
    pub token_hash: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub secret_sha256: String,
    #[serde(default)]
    pub token_preview: String,
    #[serde(default)]
    pub repo_root: String,
    #[serde(default)]
    pub revoked_at: String,
    #[serde(default)]
    pub sent_message_ids: BTreeSet<String>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Session {
    fn credential_digest(&self) -> &str {
        if self.token_hash.is_empty() {
            &self.secret_sha256
        } else {
            &self.token_hash
        }
    }

    fn canonicalize(&mut self) {
        if self.token_hash.is_empty() {
            self.token_hash = std::mem::take(&mut self.secret_sha256);
        } else {
            self.secret_sha256.clear();
        }
        if let Some(Value::Object(messages)) = self.extra.get("sent_messages") {
            self.sent_message_ids.extend(messages.keys().cloned());
        }
    }
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

pub struct AdvisoryPermit {
    pub session: Session,
    path: PathBuf,
    _lock: File,
}

impl AdvisoryPermit {
    pub fn record_message(mut self, message_id: &str) -> io::Result<bool> {
        if !self.session.sent_message_ids.insert(message_id.into()) {
            return Ok(false);
        }
        self.session.canonicalize();
        self.session.updated_at = utc_now();
        write_json(&self.path, &self.session)?;
        Ok(true)
    }
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
            group.active_scope_key.clone()
        } else if group
            .scopes
            .iter()
            .any(|scope| scope.scope_key == spec.scope_key)
        {
            spec.scope_key.clone()
        } else {
            return Err(io::Error::other("scope is not attached to group"));
        };
        if effective_scope.is_empty() {
            return Err(io::Error::other("group has no active scope"));
        }
        let repo_root = group
            .scopes
            .iter()
            .find(|scope| scope.scope_key == effective_scope)
            .map(|scope| scope.url.clone())
            .ok_or_else(|| io::Error::other("session scope is unavailable"))?;
        let sid = format!("nomcp_{}", &Uuid::new_v4().simple().to_string()[..16]);
        let secret = format!("nomcps_{}", Uuid::new_v4().simple());
        let now = chrono::Utc::now();
        let now_text = utc_now();
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
            created_at: now_text.clone(),
            updated_at: now_text,
            expires_at: (now
                + chrono::Duration::seconds(spec.expires_in_seconds.clamp(60, 604_800)))
            .to_rfc3339_opts(chrono::SecondsFormat::Micros, true),
            token_hash: digest(&secret),
            secret_sha256: String::new(),
            token_preview: preview(&secret),
            repo_root,
            revoked_at: String::new(),
            sent_message_ids: BTreeSet::new(),
            extra: BTreeMap::new(),
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
        validate_authorization(&session, secret)?;
        Ok(session)
    }

    pub fn authorize_advisory(&self, sid: &str, secret: &str) -> io::Result<AdvisoryPermit> {
        let lock = self.acquire_session_lock(sid)?;
        let session = self
            .get(sid)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "session not found"))?;
        validate_authorization(&session, secret)?;
        Ok(AdvisoryPermit {
            session,
            path: self.path(sid),
            _lock: lock,
        })
    }

    pub fn revoke(&self, sid: &str) -> io::Result<bool> {
        let _lock = self.acquire_session_lock(sid)?;
        let Some(mut session) = self.get(sid)? else {
            return Ok(false);
        };
        session.canonicalize();
        session.revoked_at = utc_now();
        session.updated_at = session.revoked_at.clone();
        self.save(&session)?;
        Ok(true)
    }

    pub fn record_message(&self, sid: &str, message_id: &str) -> io::Result<bool> {
        let _lock = self.acquire_session_lock(sid)?;
        let mut session = self
            .get(sid)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "session not found"))?;
        if !session.sent_message_ids.insert(message_id.into()) {
            return Ok(false);
        }
        session.canonicalize();
        session.updated_at = utc_now();
        self.save(&session)?;
        Ok(true)
    }

    fn acquire_session_lock(&self, sid: &str) -> io::Result<File> {
        validate_sid(sid)?;
        let path = self.dir().join(format!("{sid}.lock"));
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)?;
        lock.lock_exclusive()?;
        Ok(lock)
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

fn validate_authorization(session: &Session, secret: &str) -> io::Result<()> {
    if !session.revoked_at.is_empty() {
        return Err(io::Error::other("session revoked"));
    }
    let expires =
        chrono::DateTime::parse_from_rfc3339(&session.expires_at).map_err(io::Error::other)?;
    if expires < chrono::Utc::now() {
        return Err(io::Error::other("session expired"));
    }
    if digest(secret) != session.credential_digest() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "invalid session secret",
        ));
    }
    Ok(())
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

fn preview(secret: &str) -> String {
    if secret.len() <= 12 {
        return "****".into();
    }
    format!("{}...{}", &secret[..7], &secret[secret.len() - 4..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Scope, group_scope};
    use fs2::FileExt;
    use std::fs::OpenOptions;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn record_message_waits_for_the_shared_session_lock() {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("repo");
        std::fs::create_dir_all(&repo).expect("repo");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let groups = GroupStore::new(home.clone()).expect("groups");
        let group = groups.create("nomcp", "").expect("group");
        group_scope::attach(
            &groups,
            &group.group_id,
            Scope {
                scope_key: "scope_repo".into(),
                url: repo.to_string_lossy().into_owned(),
                label: "repo".into(),
                git_remote: String::new(),
            },
        )
        .expect("attach");
        let store = Store::new(home).expect("store");
        let created = store
            .create(CreateSpec {
                group_id: group.group_id,
                title: String::new(),
                brief: String::new(),
                reply_to_event_id: String::new(),
                recipient: "user".into(),
                scope_key: "scope_repo".into(),
                allowed_paths: Vec::new(),
                expires_in_seconds: 600,
            })
            .expect("session");
        let lock_path = store.dir().join(format!("{}.lock", created.session.sid));
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)
            .expect("lock file");
        lock.lock_exclusive().expect("hold canonical lock");
        let sid = created.session.sid;
        let writer = store.clone();
        let (tx, rx) = mpsc::channel();
        let task = std::thread::spawn(move || {
            tx.send(writer.record_message(&sid, "msg-1"))
                .expect("send result");
        });

        let blocked = rx.recv_timeout(Duration::from_millis(100)).is_err();
        FileExt::unlock(&lock).expect("unlock");
        if blocked {
            rx.recv_timeout(Duration::from_secs(2))
                .expect("writer resumed")
                .expect("record message");
        }
        task.join().expect("writer");

        assert!(blocked, "Rust mutation must honor the shared session lock");
    }
}
