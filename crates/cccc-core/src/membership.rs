use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

use crate::fs;
use crate::home::HomeLayout;

pub const LOGOUT_WARNING: &str = "This device and its public hostname were retired. The next login creates a new device and hostname.";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MembershipState {
    #[serde(default)]
    pub logged_in: bool,
    #[serde(default)]
    pub account_origin: Option<String>,
    #[serde(default)]
    pub device_id: Option<String>,
    #[serde(default)]
    pub device_token: Option<String>,
    #[serde(default)]
    pub hostname: Option<String>,
    #[serde(default)]
    pub tunnel_token: Option<String>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub pending_login: Option<serde_json::Value>,
}

pub fn path(home: &HomeLayout) -> PathBuf {
    home.root().join("secrets").join("membership.json")
}

pub fn lock_path(home: &HomeLayout) -> PathBuf {
    home.root().join("secrets").join("membership.json.lock")
}

fn load_unlocked(path: &Path) -> io::Result<MembershipState> {
    if !path.exists() {
        return Ok(MembershipState::default());
    }
    fs::read_json(path)
}

pub fn load(home: &HomeLayout) -> io::Result<MembershipState> {
    fs::with_exclusive_lock(&lock_path(home), || load_unlocked(&path(home)))
}

pub fn save(home: &HomeLayout, state: &MembershipState) -> io::Result<()> {
    fs::with_exclusive_lock(&lock_path(home), || {
        fs::write_secret_json(&path(home), state)
    })
}

pub fn update<T>(
    home: &HomeLayout,
    change: impl FnOnce(&mut MembershipState) -> io::Result<T>,
) -> io::Result<T> {
    fs::with_exclusive_lock(&lock_path(home), || {
        let mut state = load_unlocked(&path(home))?;
        let result = change(&mut state)?;
        fs::write_secret_json(&path(home), &state)?;
        Ok(result)
    })
}

pub fn clear(home: &HomeLayout) -> io::Result<()> {
    fs::with_exclusive_lock(&lock_path(home), || {
        let path = path(home);
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    })
}

pub const DEFAULT_ACCOUNT_ORIGIN: &str = "https://account.cccc.sh";

pub fn canonical_account_origin(value: &str) -> String {
    let origin = value.trim().trim_end_matches('/');
    match origin.to_ascii_lowercase().as_str() {
        "http://account.cccc.foo" | "https://account.cccc.foo" => DEFAULT_ACCOUNT_ORIGIN.to_owned(),
        _ => origin.to_owned(),
    }
}

pub fn account_origin() -> Option<String> {
    std::env::var("CCCC_ACCOUNT_ORIGIN")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .or_else(|| Some(DEFAULT_ACCOUNT_ORIGIN.to_owned()))
        .map(|value| canonical_account_origin(&value))
}
