use cccc_contracts::utc_now;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io;
use std::path::PathBuf;
use uuid::Uuid;

use crate::HomeLayout;
use crate::fs::{read_yaml, write_yaml};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccessToken {
    pub token: String,
    pub user_id: String,
    #[serde(default)]
    pub allowed_groups: Vec<String>,
    #[serde(default)]
    pub is_admin: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl AccessToken {
    #[must_use]
    pub fn token_id(&self) -> String {
        token_id(&self.token)
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct TokenDocument {
    #[serde(default)]
    tokens: BTreeMap<String, AccessToken>,
}

#[derive(Debug, Clone)]
pub struct AccessTokenStore {
    home: HomeLayout,
}

impl AccessTokenStore {
    pub fn new(home: HomeLayout) -> io::Result<Self> {
        home.initialize().map_err(io::Error::other)?;
        Ok(Self { home })
    }

    pub fn list(&self) -> io::Result<Vec<AccessToken>> {
        let mut tokens: Vec<_> = self.load()?.tokens.into_values().collect();
        tokens.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        Ok(tokens)
    }

    pub fn lookup(&self, raw: &str) -> io::Result<Option<AccessToken>> {
        Ok(self.load()?.tokens.get(raw.trim()).cloned())
    }

    pub fn create(
        &self,
        user_id: &str,
        allowed_groups: Vec<String>,
        is_admin: bool,
        custom_token: Option<&str>,
    ) -> io::Result<AccessToken> {
        let user_id = user_id.trim();
        if user_id.is_empty() {
            return Err(io::Error::other("user_id is required"));
        }
        self.mutate(|document| {
            let token = custom_token
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| format!("acc_{}", Uuid::new_v4().simple()));
            if document.tokens.contains_key(&token) {
                return Err(io::Error::other("access token already exists"));
            }
            let now = utc_now();
            let entry = AccessToken {
                token: token.clone(),
                user_id: user_id.into(),
                allowed_groups: if is_admin {
                    Vec::new()
                } else {
                    normalize_groups(allowed_groups)
                },
                is_admin,
                created_at: now.clone(),
                updated_at: now,
            };
            document.tokens.insert(token, entry.clone());
            Ok(entry)
        })
    }

    pub fn update(
        &self,
        id: &str,
        allowed_groups: Option<Vec<String>>,
        is_admin: Option<bool>,
    ) -> io::Result<Option<AccessToken>> {
        self.mutate(|document| {
            let Some(entry) = find_by_id_mut(document, id) else {
                return Ok(None);
            };
            let next_admin = is_admin.unwrap_or(entry.is_admin);
            if next_admin {
                entry.allowed_groups.clear();
            } else if let Some(groups) = allowed_groups {
                entry.allowed_groups = normalize_groups(groups);
            }
            entry.is_admin = next_admin;
            entry.updated_at = utc_now();
            Ok(Some(entry.clone()))
        })
    }

    pub fn delete(&self, id: &str) -> io::Result<Option<AccessToken>> {
        self.mutate(|document| {
            let raw = document
                .tokens
                .keys()
                .find(|raw| token_id(raw) == id)
                .cloned();
            Ok(raw.and_then(|raw| document.tokens.remove(&raw)))
        })
    }

    fn load(&self) -> io::Result<TokenDocument> {
        let path = self.path();
        if path.exists() {
            read_yaml(&path)
        } else {
            Ok(TokenDocument::default())
        }
    }

    fn mutate<T>(&self, change: impl FnOnce(&mut TokenDocument) -> io::Result<T>) -> io::Result<T> {
        let lock = self.lock()?;
        lock.lock_exclusive()?;
        let mut document = self.load()?;
        let result = change(&mut document)?;
        write_yaml(&self.path(), &document)?;
        protect(&self.path())?;
        FileExt::unlock(&lock)?;
        Ok(result)
    }

    fn lock(&self) -> io::Result<File> {
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.home.root().join("access_tokens.lock"))
    }

    fn path(&self) -> PathBuf {
        self.home.root().join("access_tokens.yaml")
    }
}

#[must_use]
pub fn token_id(raw: &str) -> String {
    format!("{:x}", Sha256::digest(raw.as_bytes()))[..16].into()
}

fn find_by_id_mut<'a>(document: &'a mut TokenDocument, id: &str) -> Option<&'a mut AccessToken> {
    document
        .tokens
        .iter_mut()
        .find(|(raw, _)| token_id(raw) == id)
        .map(|(_, entry)| entry)
}

fn normalize_groups(groups: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    groups
        .into_iter()
        .map(|group| group.trim().to_owned())
        .filter(|group| !group.is_empty() && seen.insert(group.clone()))
        .collect()
}

#[cfg(unix)]
fn protect(path: &std::path::Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn protect(_path: &std::path::Path) -> io::Result<()> {
    Ok(())
}
