//! Keep unresolved attempts bounded; retain exact completed receipts indefinitely.
//! Random handoff IDs have no trustworthy expiry, so deleting a receipt would
//! turn an old request into a new native submission. Only active slots are reclaimed.
use cccc_contracts::session_node::SessionNodeDeliveryOutcome as Outcome;
use cccc_core::{HomeLayout, fs};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_UNRESOLVED: usize = 8192;
// Bound migration work per admission instead of rewriting thousands of receipts
// while holding the cross-process lock on the first request after an upgrade.
const MIGRATION_BATCH: usize = 128;
const MAX_RECEIPT_BYTES: u64 = 1024 * 1024;

#[derive(Serialize, Deserialize)]
struct Attempt {
    fingerprint: String,
    outcome: Outcome,
}

// Old readers cannot see archived receipts. This envelope makes them fail
// closed instead of admitting a previously completed identity after downgrade.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    version: u32,
    attempts: BTreeMap<String, Attempt>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StoredJournal {
    Current(Journal),
    Legacy(BTreeMap<String, Attempt>),
}

fn completed(outcome: &Outcome) -> bool {
    matches!(
        outcome,
        Outcome::Accepted | Outcome::Rejected { .. } | Outcome::Unavailable
    )
}

fn fingerprint(payload: &str) -> String {
    format!("{:x}", Sha256::digest(payload.as_bytes()))
}

fn receipt_path(root: &Path, id: &str) -> PathBuf {
    let hash = fingerprint(id);
    root.join("delivery_receipts")
        .join(&hash[..2])
        .join(format!("{hash}.json"))
}

fn read_receipt(root: &Path, id: &str) -> io::Result<Option<Attempt>> {
    let file = match std::fs::File::open(receipt_path(root, id)) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(MAX_RECEIPT_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_RECEIPT_BYTES {
        return Err(io::Error::other("Remote delivery receipt exceeds one MiB"));
    }
    let receipt: Attempt = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    if !completed(&receipt.outcome)
        || receipt.fingerprint.len() != 64
        || !receipt
            .fingerprint
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(io::Error::other("Remote delivery receipt is invalid"));
    }
    Ok(Some(receipt))
}

fn check_fingerprint(attempt: &Attempt, expected: &str) -> io::Result<()> {
    if attempt.fingerprint != expected {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Remote delivery identifier was reused with a different payload",
        ));
    }
    Ok(())
}

fn archive(root: &Path, id: &str, attempt: &Attempt) -> io::Result<()> {
    if let Some(existing) = read_receipt(root, id)? {
        check_fingerprint(&existing, &attempt.fingerprint)?;
        if serde_json::to_value(&existing.outcome).map_err(io::Error::other)?
            != serde_json::to_value(&attempt.outcome).map_err(io::Error::other)?
        {
            return Err(io::Error::other("Remote delivery receipt outcome changed"));
        }
        return sync_receipt_parents(root, id);
    }
    let path = receipt_path(root, id);
    let shard = path.parent().expect("receipt shard");
    std::fs::create_dir_all(shard)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for directory in [shard, shard.parent().expect("receipt directory")] {
            std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    fs::write_secret_json(&path, attempt)?;
    sync_receipt_parents(root, id)
}

fn sync_receipt_parents(root: &Path, id: &str) -> io::Result<()> {
    // Include the shard on retries: atomic_write can return a directory-sync
    // error after rename. Never remove the active fence until all directory
    // entries of the receipt have been persisted.
    #[cfg(unix)]
    {
        let path = receipt_path(root, id);
        let shard = path.parent().expect("receipt shard");
        for directory in [shard, shard.parent().expect("receipt directory"), root] {
            std::fs::File::open(directory)?.sync_all()?;
        }
    }
    #[cfg(not(unix))]
    let _ = (root, id); // Match core atomic_write's platform persistence guarantees.
    Ok(())
}

fn mutate<T>(
    home: &HomeLayout,
    action: impl FnOnce(&Path, &mut BTreeMap<String, Attempt>) -> io::Result<T>,
) -> io::Result<T> {
    let root = home.root().join("state/session_nodes");
    std::fs::create_dir_all(&root)?;
    fs::with_exclusive_lock(&root.join("deliveries.lock"), || {
        let path = root.join("deliveries.json");
        let mut state = if path.exists() {
            match fs::read_json(&path)? {
                StoredJournal::Current(journal) if journal.version == 2 => journal.attempts,
                StoredJournal::Current(_) => {
                    return Err(io::Error::other(
                        "Unsupported remote delivery journal version",
                    ));
                }
                StoredJournal::Legacy(attempts) => attempts,
            }
        } else {
            BTreeMap::new()
        };
        let result = action(&root, &mut state)?;
        fs::write_secret_json(
            &path,
            &Journal {
                version: 2,
                attempts: state,
            },
        )?;
        Ok(result)
    })
}

pub(super) fn begin(home: &HomeLayout, id: &str, payload: &str) -> io::Result<Option<Outcome>> {
    let expected = fingerprint(payload);
    mutate(home, |root, state| {
        if let Some(receipt) = read_receipt(root, id)? {
            check_fingerprint(&receipt, &expected)?;
            if let Some(active) = state.get(id) {
                check_fingerprint(active, &expected)?;
                sync_receipt_parents(root, id)?;
            }
            // A crash after receipt persistence but before active-slot removal
            // leaves both. The durable final receipt safely settles this slot.
            state.remove(id);
            return Ok(Some(receipt.outcome));
        }
        if let Some(attempt) = state.get(id) {
            check_fingerprint(attempt, &expected)?;
            return Ok(Some(attempt.outcome.clone()));
        }
        let completed_ids: Vec<_> = state
            .iter()
            .filter(|(_, attempt)| completed(&attempt.outcome))
            .take(MIGRATION_BATCH)
            .map(|(id, _)| id.clone())
            .collect();
        for old_id in completed_ids {
            archive(root, &old_id, &state[&old_id])?;
            state.remove(&old_id);
        }
        if state.len() >= MAX_UNRESOLVED {
            return Err(io::Error::other(
                "Remote delivery journal is full of unresolved attempts; no message was submitted",
            ));
        }
        state.insert(id.into(), Attempt { fingerprint: expected, outcome: Outcome::Ambiguous { reason: "A previous attempt has no confirmed native outcome; automatic replay is blocked".into() } });
        Ok(None)
    })
}

pub(super) fn finish(
    home: &HomeLayout,
    id: &str,
    payload: &str,
    outcome: &Outcome,
) -> io::Result<()> {
    let expected = fingerprint(payload);
    mutate(home, |root, state| {
        let attempt = state
            .get_mut(id)
            .filter(|attempt| attempt.fingerprint == expected)
            .ok_or_else(|| io::Error::other("Remote delivery journal changed"))?;
        attempt.outcome = outcome.clone();
        if completed(outcome) {
            archive(root, id, attempt)?;
            state.remove(id);
        }
        Ok(())
    })
}

#[cfg(test)]
#[path = "journal_tests.rs"]
mod tests;
