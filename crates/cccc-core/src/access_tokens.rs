use cccc_contracts::utc_now;
use fs2::FileExt;
use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize, Serializer};
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
    #[serde(default)]
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

#[derive(Debug, Default, Serialize)]
struct TokenDocument {
    #[serde(default, serialize_with = "serialize_tokens")]
    tokens: BTreeMap<String, AccessToken>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum TokenDocumentFormat {
    Wrapped {
        tokens: BTreeMap<String, AccessToken>,
    },
    Flat(BTreeMap<String, AccessToken>),
}

impl<'de> Deserialize<'de> for TokenDocument {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let tokens = match TokenDocumentFormat::deserialize(deserializer)? {
            TokenDocumentFormat::Wrapped { tokens } | TokenDocumentFormat::Flat(tokens) => tokens,
        };
        Ok(Self { tokens })
    }
}

#[derive(Serialize)]
struct StoredAccessToken<'a> {
    user_id: &'a str,
    allowed_groups: &'a [String],
    is_admin: bool,
    created_at: &'a str,
    updated_at: &'a str,
}

fn serialize_tokens<S>(
    tokens: &BTreeMap<String, AccessToken>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut map = serializer.serialize_map(Some(tokens.len()))?;
    for (raw, entry) in tokens {
        map.serialize_entry(
            raw,
            &StoredAccessToken {
                user_id: &entry.user_id,
                allowed_groups: &entry.allowed_groups,
                is_admin: entry.is_admin,
                created_at: &entry.created_at,
                updated_at: &entry.updated_at,
            },
        )?;
    }
    map.end()
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
            if token.chars().any(char::is_control) {
                return Err(io::Error::other(
                    "access token cannot contain control characters",
                ));
            }
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
            let mut document: TokenDocument = read_yaml(&path)?;
            for (raw, entry) in &mut document.tokens {
                if entry.token.is_empty() {
                    entry.token.clone_from(raw);
                }
            }
            Ok(document)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_python_token_map_without_rewriting_it() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let store = AccessTokenStore::new(home).expect("store");
        let path = temp.path().join("access_tokens.yaml");
        let fixture = concat!(
            "tokens:\n",
            "  legacy-token:\n",
            "    user_id: legacy-user\n",
            "    allowed_groups: []\n",
            "    is_admin: true\n",
            "    created_at: '2026-01-01T00:00:00Z'\n",
            "    updated_at: '2026-01-01T00:00:00Z'\n",
        );
        std::fs::write(&path, fixture).expect("fixture");

        let token = store
            .lookup("legacy-token")
            .expect("lookup")
            .expect("token");
        assert_eq!(token.token, "legacy-token");
        assert_eq!(std::fs::read_to_string(&path).expect("unchanged"), fixture);

        store
            .update(&token.token_id(), None, Some(false))
            .expect("update")
            .expect("updated token");
        let stored: serde_yaml::Value =
            serde_yaml::from_reader(File::open(path).expect("stored file")).expect("stored yaml");
        assert!(stored["tokens"]["legacy-token"]["token"].is_null());
    }

    #[test]
    fn loads_legacy_flat_token_map() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let store = AccessTokenStore::new(home).expect("store");
        std::fs::write(
            temp.path().join("access_tokens.yaml"),
            concat!(
                "legacy-flat-token:\n",
                "  user_id: legacy-user\n",
                "  allowed_groups: []\n",
                "  is_admin: true\n",
                "  created_at: '2026-01-01T00:00:00Z'\n",
                "  updated_at: '2026-01-01T00:00:00Z'\n",
            ),
        )
        .expect("fixture");

        let token = store
            .lookup("legacy-flat-token")
            .expect("lookup")
            .expect("token");
        assert_eq!(token.user_id, "legacy-user");
        assert!(token.is_admin);
    }

    #[test]
    fn rejects_control_characters_in_custom_tokens() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path()).expect("home");
        let store = AccessTokenStore::new(home).expect("store");
        assert!(
            store
                .create("admin", Vec::new(), true, Some("unsafe\ntoken"))
                .is_err()
        );
    }
}
