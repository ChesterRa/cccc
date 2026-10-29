//! The VAPID key and the subscriptions, kept in one owner-only document so a
//! binding can never outlive the key it was made with. Endpoints and keys
//! stay in this file: the API reports an endpoint only by its hash.

use std::collections::{BTreeMap, HashSet};
use std::io;
use std::path::PathBuf;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use cccc_core::HomeLayout;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::crypto::VapidKey;

const DOCUMENT: &str = "web_push.json";
const LOCK: &str = "web_push.lock";
/// Browsers and devices one owner keeps; the least recently bound goes first.
const MAX_PER_OWNER: usize = 16;

#[derive(Default, Serialize, Deserialize)]
struct Document {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    vapid: Option<StoredKey>,
    #[serde(default)]
    subscriptions: Vec<Subscription>,
}

#[derive(Serialize, Deserialize)]
struct StoredKey {
    /// PKCS#8, base64url.
    pkcs8: String,
    created_at: String,
}

impl Document {
    /// The current key. One that is stored but cannot be read is an error,
    /// not a missing key: replacing it would end every binding silently.
    /// Bindings stored without a key are as unreadable as a damaged key.
    fn key(&self) -> io::Result<Option<VapidKey>> {
        let Some(stored) = &self.vapid else {
            if self.subscriptions.is_empty() {
                return Ok(None);
            }
            return Err(io::Error::other(
                "Web Push subscriptions are stored without their key",
            ));
        };
        URL_SAFE_NO_PAD
            .decode(&stored.pkcs8)
            .ok()
            .and_then(|pkcs8| VapidKey::from_pkcs8(&pkcs8).ok())
            .map(Some)
            .ok_or_else(|| io::Error::other("the stored Web Push key cannot be read"))
    }
}

/// Who a subscription notifies: the loopback administrator, or one access
/// token. A token deleted and added again with the same secret is a new
/// owner, so its earlier bindings do not come back with it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Owner {
    Local,
    Token {
        user_id: String,
        token_id: String,
        created_at: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Subscription {
    pub(crate) endpoint: String,
    /// New when the browser's keys, owner or origin change; binding the same
    /// browser again unchanged keeps it. A send started before a change
    /// reports back with the binding it read, so its result cannot land on
    /// the new one.
    pub(crate) binding: String,
    pub(crate) p256dh: String,
    pub(crate) auth: String,
    /// The VAPID public key the browser subscribed with.
    pub(crate) application_server_key: String,
    pub(crate) owner: Owner,
    /// The page origin it was made from; notification links lead back there.
    pub(crate) origin: String,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    /// The newest event of each Group the owner could see when the binding
    /// was made, or an empty id where nothing was written yet: only later
    /// events are announced to it. A Group without an entry was not visible
    /// or did not exist then; it is announced from where this process starts
    /// following it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) since: BTreeMap<String, String>,
}

/// Whether `origin` is a page origin in the form a binding stores it.
/// Notifications link there, so only such a binding is sent to; anything
/// else is a damaged store.
pub(crate) fn is_page_origin(origin: &str) -> bool {
    cccc_core::web_login_grants::normalize_origin(origin).is_some_and(|normal| normal == origin)
}

/// A browser subscription as the page sends it, already validated.
pub(crate) struct NewSubscription {
    pub(crate) endpoint: String,
    pub(crate) p256dh: String,
    pub(crate) auth: String,
    pub(crate) application_server_key: String,
    pub(crate) origin: String,
}

#[derive(Debug)]
pub(crate) enum BindError {
    /// No key has been made yet, so nothing could have subscribed with it.
    NoKey,
    /// The browser subscribed with a key that is not the current one.
    KeyMismatch,
    Io(io::Error),
}

impl From<io::Error> for BindError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

fn new_binding() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

pub(crate) fn endpoint_hash(endpoint: &str) -> String {
    format!("{:x}", Sha256::digest(endpoint.as_bytes()))
}

#[derive(Clone)]
pub(crate) struct PushStore {
    home: HomeLayout,
}

impl PushStore {
    pub(crate) fn new(home: HomeLayout) -> Self {
        Self { home }
    }

    fn path(&self) -> PathBuf {
        self.home.root().join(DOCUMENT)
    }

    /// The file is replaced atomically, so a read needs no lock.
    fn load(&self) -> io::Result<Document> {
        match std::fs::read(self.path()) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
                // Serde's data errors can quote the invalid value, including
                // an endpoint or key. Keep diagnostics without logging it.
                io::Error::other(format!(
                    "invalid Web Push state ({:?} at line {}, column {})",
                    error.classify(),
                    error.line(),
                    error.column()
                ))
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Document::default()),
            Err(error) => Err(error),
        }
    }

    /// Change the document under the lock; a refused change writes nothing.
    fn mutate<T, E: From<io::Error>>(
        &self,
        change: impl FnOnce(&mut Document) -> Result<T, E>,
    ) -> Result<T, E> {
        cccc_core::fs::with_exclusive_lock(&self.home.root().join(LOCK), || {
            let mut document = self.load()?;
            Ok(match change(&mut document) {
                Ok(result) => {
                    cccc_core::fs::write_secret_json(&self.path(), &document)?;
                    Ok(result)
                }
                Err(error) => Err(error),
            })
        })?
    }

    pub(crate) fn public_key(&self) -> io::Result<Option<String>> {
        Ok(self.load()?.key()?.map(|key| key.public_key().to_owned()))
    }

    /// The public key, made on first use. A key is made only where none was
    /// ever kept; an existing one, readable or not, is never replaced.
    pub(crate) fn ensure_key(&self) -> io::Result<String> {
        if let Some(key) = self.public_key()? {
            return Ok(key);
        }
        self.mutate(|document| {
            if let Some(key) = document.key()? {
                return Ok(key.public_key().to_owned());
            }
            let pkcs8 = VapidKey::generate_pkcs8()
                .map_err(|_| io::Error::other("could not make a Web Push key"))?;
            let key = VapidKey::from_pkcs8(&pkcs8)
                .map_err(|_| io::Error::other("could not read the new Web Push key"))?;
            document.vapid = Some(StoredKey {
                pkcs8: URL_SAFE_NO_PAD.encode(&pkcs8),
                created_at: cccc_contracts::utc_now(),
            });
            Ok(key.public_key().to_owned())
        })
    }

    /// Hashes of the endpoints `owner` holds under the current key, leaving
    /// out any whose page origin is invalid, so the page offers to bind it
    /// again.
    pub(crate) fn owned(&self, owner: &Owner) -> io::Result<Vec<String>> {
        let document = self.load()?;
        let Some(key) = document.key()? else {
            return Ok(Vec::new());
        };
        Ok(document
            .subscriptions
            .iter()
            .filter(|item| {
                item.application_server_key == key.public_key()
                    && item.owner == *owner
                    && is_page_origin(&item.origin)
            })
            .map(|item| endpoint_hash(&item.endpoint))
            .collect())
    }

    /// Bind a browser subscription to `owner`, starting after `since`, the
    /// newest event of each Group now. The endpoint names one browser, so
    /// binding it again moves it to the owner now signed in there.
    pub(crate) fn bind(
        &self,
        new: NewSubscription,
        owner: Owner,
        since: BTreeMap<String, String>,
    ) -> Result<(), BindError> {
        self.mutate(|document| {
            let key = document.key()?.ok_or(BindError::NoKey)?;
            let current = key.public_key();
            if new.application_server_key.trim_end_matches('=') != current {
                return Err(BindError::KeyMismatch);
            }
            let now = cccc_contracts::utc_now();
            let endpoint = new.endpoint.clone();
            document
                .subscriptions
                .retain(|item| item.application_server_key == current);
            if let Some(item) = document
                .subscriptions
                .iter_mut()
                .find(|item| item.endpoint == new.endpoint)
            {
                // The same browser sent again, as after a lost answer, keeps
                // its place; anything else about it changed starts it anew.
                let same = item.owner == owner
                    && item.p256dh == new.p256dh
                    && item.auth == new.auth
                    && item.origin == new.origin;
                if !same {
                    item.binding = new_binding();
                    item.since = since;
                    item.p256dh = new.p256dh;
                    item.auth = new.auth;
                    item.owner = owner.clone();
                    item.origin = new.origin;
                }
                item.updated_at = now;
            } else {
                document.subscriptions.push(Subscription {
                    endpoint: new.endpoint,
                    binding: new_binding(),
                    p256dh: new.p256dh,
                    auth: new.auth,
                    application_server_key: current.to_owned(),
                    owner: owner.clone(),
                    origin: new.origin,
                    created_at: now.clone(),
                    updated_at: now,
                    since,
                });
            }
            // The oldest others make room. The browser just bound stays even
            // when its time sorts first, as after the clock was set back.
            let mut others: Vec<_> = document
                .subscriptions
                .iter()
                .filter(|item| item.owner == owner && item.endpoint != endpoint)
                .map(|item| (item.updated_at.clone(), item.endpoint.clone()))
                .collect();
            if others.len() >= MAX_PER_OWNER {
                others.sort();
                let evicted: HashSet<_> = others[..=others.len() - MAX_PER_OWNER]
                    .iter()
                    .map(|(_, endpoint)| endpoint.clone())
                    .collect();
                document
                    .subscriptions
                    .retain(|item| item.owner != owner || !evicted.contains(&item.endpoint));
            }
            Ok(())
        })
    }

    /// Remove `endpoint` when `owner` holds it. Another owner's binding is
    /// left alone, and the result does not reveal that it exists.
    pub(crate) fn unbind(&self, endpoint: &str, owner: &Owner) -> io::Result<bool> {
        let held = |item: &Subscription| item.endpoint == endpoint && item.owner == *owner;
        if !self.load()?.subscriptions.iter().any(held) {
            return Ok(false);
        }
        self.mutate(|document| {
            let before = document.subscriptions.len();
            document.subscriptions.retain(|item| !held(item));
            Ok::<_, io::Error>(document.subscriptions.len() != before)
        })
    }

    /// The key and the subscriptions made with it, for one round of sending.
    pub(crate) fn snapshot(&self) -> io::Result<Option<(VapidKey, Vec<Subscription>)>> {
        let document = self.load()?;
        let Some(key) = document.key()? else {
            return Ok(None);
        };
        let subscriptions = document
            .subscriptions
            .into_iter()
            .filter(|item| item.application_server_key == key.public_key())
            .collect();
        Ok(Some((key, subscriptions)))
    }

    /// Drop the bindings a push service no longer knows or whose owner is
    /// gone, each only while it is still the binding that was sent to.
    pub(crate) fn forget(&self, bindings: &HashSet<String>) -> io::Result<()> {
        if bindings.is_empty() {
            return Ok(());
        }
        self.mutate(|document| {
            document
                .subscriptions
                .retain(|item| !bindings.contains(&item.binding));
            Ok::<_, io::Error>(())
        })
    }

    /// The binding as it is stored now, if it still exists.
    pub(crate) fn current(&self, binding: &str) -> io::Result<Option<Subscription>> {
        let document = self.load()?;
        let Some(key) = document.key()? else {
            return Ok(None);
        };
        Ok(document.subscriptions.into_iter().find(|item| {
            item.binding == binding && item.application_server_key == key.public_key()
        }))
    }
}

#[cfg(test)]
mod tests;
