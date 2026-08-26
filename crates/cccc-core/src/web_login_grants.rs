use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io;
use url::Origin;
use uuid::Uuid;

use crate::HomeLayout;
use crate::fs::{read_json, with_exclusive_lock, write_secret_json};

const STORE_FILE: &str = "web_login_grants.json";
const LOCK_FILE: &str = "web_login_grants.lock";
const MAX_GRANTS: usize = 64;
pub const DEFAULT_TTL_SECONDS: i64 = 120;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedGrant {
    pub code: String,
    pub origin: String,
    pub expires_at_epoch: i64,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct GrantDocument {
    #[serde(default)]
    v: u8,
    #[serde(default)]
    grants: BTreeMap<String, GrantRecord>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct GrantRecord {
    token_id: String,
    origin: String,
    created_at_epoch: i64,
    expires_at_epoch: i64,
}

pub fn issue(
    home: &HomeLayout,
    origin: &str,
    token_id: &str,
    ttl_seconds: i64,
) -> io::Result<IssuedGrant> {
    issue_at(home, origin, token_id, ttl_seconds, Utc::now().timestamp())
}

pub fn consume(home: &HomeLayout, code: &str, origin: &str) -> io::Result<Option<String>> {
    consume_at(home, code, origin, Utc::now().timestamp())
}

pub fn normalize_origin(value: &str) -> Option<String> {
    let parsed = url::Url::parse(value.trim()).ok()?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.host_str().is_none()
    {
        return None;
    }
    match parsed.origin() {
        Origin::Tuple(..) => Some(parsed.origin().ascii_serialization()),
        Origin::Opaque(_) => None,
    }
}

fn issue_at(
    home: &HomeLayout,
    origin: &str,
    token_id: &str,
    ttl_seconds: i64,
    now: i64,
) -> io::Result<IssuedGrant> {
    let origin = normalize_origin(origin)
        .ok_or_else(|| io::Error::other("Web login grant origin must be HTTP(S)"))?;
    let token_id = token_id.trim();
    if token_id.len() != 16 || !token_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(io::Error::other("Web login grant token id is invalid"));
    }
    let ttl_seconds = ttl_seconds.clamp(30, 300);
    let expires_at_epoch = now.saturating_add(ttl_seconds);
    let code = format!("wlg_{}", Uuid::new_v4().simple());
    let digest = code_digest(&code);
    with_exclusive_lock(&lock_path(home), || {
        let mut document = load(home)?;
        prune(&mut document, now);
        while document.grants.len() >= MAX_GRANTS {
            let Some(oldest) = document
                .grants
                .iter()
                .min_by_key(|(_, record)| record.created_at_epoch)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            document.grants.remove(&oldest);
        }
        document.grants.insert(
            digest,
            GrantRecord {
                token_id: token_id.to_owned(),
                origin: origin.clone(),
                created_at_epoch: now,
                expires_at_epoch,
            },
        );
        save(home, &document)
    })?;
    Ok(IssuedGrant {
        code,
        origin,
        expires_at_epoch,
    })
}

fn consume_at(home: &HomeLayout, code: &str, origin: &str, now: i64) -> io::Result<Option<String>> {
    let Some(origin) = normalize_origin(origin) else {
        return Ok(None);
    };
    let code = code.trim();
    if !code.starts_with("wlg_") || code.len() != 36 {
        return Ok(None);
    }
    with_exclusive_lock(&lock_path(home), || {
        let mut document = load(home)?;
        let changed = prune(&mut document, now);
        let digest = code_digest(code);
        let token_id = document
            .grants
            .get(&digest)
            .filter(|record| record.origin == origin && record.expires_at_epoch > now)
            .map(|record| record.token_id.clone());
        if token_id.is_some() {
            document.grants.remove(&digest);
        }
        if changed || token_id.is_some() {
            save(home, &document)?;
        }
        Ok(token_id)
    })
}

fn load(home: &HomeLayout) -> io::Result<GrantDocument> {
    let path = store_path(home);
    if !path.exists() {
        return Ok(GrantDocument::default());
    }
    let document: GrantDocument = read_json(&path)?;
    if !matches!(document.v, 0 | 1) {
        return Err(io::Error::other(
            "unsupported Web login grant store version",
        ));
    }
    Ok(document)
}

fn save(home: &HomeLayout, document: &GrantDocument) -> io::Result<()> {
    let mut document = GrantDocument {
        v: 1,
        grants: document.grants.clone(),
    };
    document.grants.retain(|digest, record| {
        digest.len() == 64
            && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
            && record.token_id.len() == 16
            && record.token_id.bytes().all(|byte| byte.is_ascii_hexdigit())
    });
    write_secret_json(&store_path(home), &document)
}

fn prune(document: &mut GrantDocument, now: i64) -> bool {
    let before = document.grants.len();
    document.grants.retain(|_, record| {
        record.expires_at_epoch > now
            && normalize_origin(&record.origin).as_deref() == Some(record.origin.as_str())
    });
    before != document.grants.len()
}

fn code_digest(code: &str) -> String {
    format!("{:x}", Sha256::digest(code.as_bytes()))
}

fn store_path(home: &HomeLayout) -> std::path::PathBuf {
    home.root().join(STORE_FILE)
}

fn lock_path(home: &HomeLayout) -> std::path::PathBuf {
    home.root().join(LOCK_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grant_is_origin_bound_one_time_and_stored_as_a_hash() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let grant = issue_at(
            &home,
            "HTTPS://Reach.Example.test:443/ui/",
            "0123456789abcdef",
            120,
            1_000,
        )
        .expect("issue");
        assert_eq!(grant.origin, "https://reach.example.test");
        let stored = std::fs::read_to_string(store_path(&home)).expect("stored grant");
        assert!(!stored.contains(&grant.code));

        assert_eq!(
            consume_at(&home, &grant.code, "https://other.example.test", 1_001)
                .expect("wrong origin"),
            None
        );
        assert_eq!(
            consume_at(&home, &grant.code, &grant.origin, 1_001).expect("consume"),
            Some("0123456789abcdef".into())
        );
        assert_eq!(
            consume_at(&home, &grant.code, &grant.origin, 1_001).expect("replay"),
            None
        );
    }

    #[test]
    fn expired_grant_is_rejected_and_pruned() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let grant = issue_at(
            &home,
            "https://reach.example.test",
            "0123456789abcdef",
            30,
            1_000,
        )
        .expect("issue");
        assert_eq!(
            consume_at(&home, &grant.code, &grant.origin, 1_031).expect("expired"),
            None
        );
        assert!(load(&home).expect("load").grants.is_empty());
    }
}
