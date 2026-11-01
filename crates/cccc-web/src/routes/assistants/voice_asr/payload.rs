//! Where an installed voice model's files live, and which versions are in use.
//!
//! Each install unpacks into its own `payload-<id>` directory beside the install state, and the
//! state's `payload_dir` names the current one. State without it is the legacy layout, with the
//! files directly in the model directory. A reader pins the version it uses for as long as it
//! uses it; cleanup removes only versions that are neither current nor pinned.

use super::{INSTALL_STATE, VoiceError, model_dir};
use cccc_core::HomeLayout;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, PoisonError};

pub(super) const PAYLOAD_PREFIX: &str = "payload-";

/// A version directory name: `payload-` followed by letters, digits, `-` or `_`, so it is always
/// one path component inside the model directory.
pub(super) fn valid_payload_dir(name: &str) -> bool {
    name.strip_prefix(PAYLOAD_PREFIX).is_some_and(|rest| {
        !rest.is_empty()
            && rest
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    })
}

/// A read of a model's install state, telling a missing file from one that cannot be used.
pub(super) enum StateRead {
    Missing,
    Unusable(String),
    Present(Value),
}

pub(super) fn read_state(model_root: &Path) -> StateRead {
    let path = model_root.join(INSTALL_STATE);
    match std::fs::read(&path) {
        // Only a path that does not exist means no state. A dangling link may still be the only
        // record of the current version, and writing a state would replace it.
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            match std::fs::symlink_metadata(&path) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => StateRead::Missing,
                _ => StateRead::Unusable("the install state is a link to a missing file".into()),
            }
        }
        Err(error) => StateRead::Unusable(error.to_string()),
        Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
            Ok(state) if state.is_object() => StateRead::Present(state),
            Ok(_) => StateRead::Unusable("install state is not a JSON object".into()),
            Err(error) => StateRead::Unusable(error.to_string()),
        },
    }
}

/// The directory a state names: its `payload_dir`, or the model directory for the legacy layout.
pub(super) fn version_root(model_root: &Path, state: &Value) -> Result<PathBuf, String> {
    match state.get("payload_dir") {
        None | Some(Value::Null) => Ok(model_root.to_path_buf()),
        Some(Value::String(name)) if valid_payload_dir(name) => Ok(model_root.join(name)),
        Some(other) => Err(format!("invalid payload_dir: {other}")),
    }
}

/// One read of a model's install state and the directory of its current version, both from that
/// read, so status, manifest and file paths always describe the same version.
pub(super) struct ModelSnapshot {
    pub(super) state: Value,
    /// The current version's directory; `None` when the state cannot be used.
    pub(super) root: Option<PathBuf>,
    /// Why the state cannot be used: unreadable, malformed or naming an invalid directory.
    pub(super) unusable: Option<String>,
}

pub(super) fn snapshot(home: &HomeLayout, model_id: &str) -> ModelSnapshot {
    let model_root = model_dir(home, model_id);
    match read_state(&model_root) {
        StateRead::Missing => ModelSnapshot {
            state: json!({"status":"not_installed"}),
            root: Some(model_root),
            unusable: None,
        },
        StateRead::Unusable(reason) => ModelSnapshot {
            state: json!({}),
            root: None,
            unusable: Some(reason),
        },
        StateRead::Present(state) => match version_root(&model_root, &state) {
            Ok(root) => ModelSnapshot {
                state,
                root: Some(root),
                unusable: None,
            },
            Err(reason) => ModelSnapshot {
                state,
                root: None,
                unusable: Some(reason),
            },
        },
    }
}

// Pinned version directories and how many readers hold each. The lock guards only map updates,
// never I/O: a pin can be dropped on a Tokio worker when a task is cancelled, and must not wait
// for cleanup reading the disk.
static PINS: LazyLock<Mutex<HashMap<PathBuf, usize>>> = LazyLock::new(Default::default);

/// A version directory held by a reader; cleanup leaves it in place until the pin is dropped.
pub(crate) struct PayloadPin {
    root: PathBuf,
}

impl PayloadPin {
    fn new(root: PathBuf) -> Self {
        *PINS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(root.clone())
            .or_default() += 1;
        Self { root }
    }
}

impl Drop for PayloadPin {
    fn drop(&mut self) {
        let mut pins = PINS.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(count) = pins.get_mut(&self.root) {
            *count -= 1;
            if *count == 0 {
                pins.remove(&self.root);
            }
        }
    }
}

/// Which of the given directories no reader holds.
pub(super) fn unpinned(roots: Vec<PathBuf>) -> Vec<PathBuf> {
    let pins = PINS.lock().unwrap_or_else(PoisonError::into_inner);
    roots
        .into_iter()
        .filter(|root| !pins.contains_key(root))
        .collect()
}

/// A native object built from a model's files, with the pin on their version. Fields drop in
/// declaration order, so the object is released before its version may be removed.
pub(crate) struct Pinned<T> {
    value: T,
    _pin: PayloadPin,
}

impl<T> Pinned<T> {
    pub(super) fn new(value: T, pin: PayloadPin) -> Self {
        Self { value, _pin: pin }
    }
}

impl<T> Deref for Pinned<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.value
    }
}

/// A model's current version, with a pin on its directory when the state names one.
pub(super) struct CurrentVersion {
    pub(super) snapshot: ModelSnapshot,
    pub(super) pin: Option<PayloadPin>,
}

/// Reads the state, pins the version it names, then reads it again: a version still current after
/// pinning is used, as cleanup removes only unpinned versions that are not current, under the
/// install lock that every switch also takes. A version that changed in between is released and
/// the next one tried.
pub(super) fn current_version(
    home: &HomeLayout,
    model_id: &str,
) -> Result<CurrentVersion, VoiceError> {
    current_version_with(|| snapshot(home, model_id))
}

pub(super) fn current_version_with(
    mut read: impl FnMut() -> ModelSnapshot,
) -> Result<CurrentVersion, VoiceError> {
    let mut seen = read();
    for _ in 0..3 {
        let Some(root) = seen.root.clone() else {
            return Ok(CurrentVersion {
                snapshot: seen,
                pin: None,
            });
        };
        let pin = PayloadPin::new(root.clone());
        let again = read();
        if again.root.as_ref() == Some(&root) {
            return Ok(CurrentVersion {
                snapshot: again,
                pin: Some(pin),
            });
        }
        seen = again;
    }
    Err(VoiceError::new(
        "voice_model_install_busy",
        "the voice model changed while it was opened; try again",
    ))
}

#[cfg(test)]
pub(super) fn pin_count(root: &Path) -> usize {
    PINS.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(root)
        .copied()
        .unwrap_or(0)
}
