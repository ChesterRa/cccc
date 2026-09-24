//! Weixin only accepts a reply that carries the context token from the user's latest message,
//! and the SDK keeps those tokens in memory. Persist them so a restart does not silently drop
//! every reply until the user writes again. The file keeps the Python bridge's flat
//! `{user_id: token}` format, so tokens it saved are restored too.

use cccc_core::{GroupStore, HomeLayout};
use serde_json::Value;
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

const FILE: &str = "im_weixin_context_tokens.json";

fn path(home: &HomeLayout, group_id: &str) -> io::Result<PathBuf> {
    Ok(GroupStore::new(home.clone())?
        .state_dir(group_id)?
        .join(FILE))
}

pub(super) fn load(home: &HomeLayout, group_id: &str) -> BTreeMap<String, String> {
    match path(home, group_id).and_then(|path| read(&path)) {
        Ok(tokens) => tokens,
        Err(error) => {
            tracing::warn!(%error, %group_id, "failed to restore Weixin context tokens");
            BTreeMap::new()
        }
    }
}

pub(super) fn remember(
    home: &HomeLayout,
    group_id: &str,
    user_id: &str,
    token: &str,
) -> io::Result<()> {
    let path = path(home, group_id)?;
    let directory = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(directory)?;
    cccc_core::fs::with_exclusive_lock(&directory.join(format!("{FILE}.lock")), || {
        // An unreadable file holds nothing usable; rewriting it resumes persistence.
        let mut tokens = read(&path).unwrap_or_else(|error| {
            tracing::warn!(%error, %group_id, "replacing unreadable Weixin context tokens");
            BTreeMap::new()
        });
        if tokens.get(user_id).map(String::as_str) == Some(token) {
            return Ok(());
        }
        tokens.insert(user_id.to_owned(), token.to_owned());
        cccc_core::fs::write_secret_json(&path, &tokens)
    })
}

fn read(path: &Path) -> io::Result<BTreeMap<String, String>> {
    let raw = match std::fs::read(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(error),
    };
    let Value::Object(entries) = serde_json::from_slice(&raw).map_err(io::Error::other)? else {
        return Err(io::Error::other(
            "Weixin context tokens must be a JSON object",
        ));
    };
    Ok(entries
        .into_iter()
        .filter_map(|(user_id, token)| match token {
            Value::String(token) if !token.is_empty() => Some((user_id, token)),
            _ => None,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, HomeLayout, String) {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let group = GroupStore::new(home.clone())
            .expect("store")
            .create("weixin", "")
            .expect("group");
        (temp, home, group.group_id)
    }

    #[test]
    fn a_remembered_token_survives_a_restart() {
        let (_temp, home, group_id) = setup();
        remember(&home, &group_id, "alice@im.wechat", "token-1").expect("remember");
        remember(&home, &group_id, "bob@im.wechat", "token-2").expect("remember");
        remember(&home, &group_id, "alice@im.wechat", "token-3").expect("refresh");

        assert_eq!(
            load(&home, &group_id),
            BTreeMap::from([
                ("alice@im.wechat".to_owned(), "token-3".to_owned()),
                ("bob@im.wechat".to_owned(), "token-2".to_owned()),
            ])
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(path(&home, &group_id).expect("path"))
                .expect("metadata")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn tokens_saved_by_the_python_bridge_are_restored() {
        let (_temp, home, group_id) = setup();
        let path = path(&home, &group_id).expect("path");
        std::fs::create_dir_all(path.parent().expect("state dir")).expect("state dir");
        std::fs::write(
            &path,
            br#"{"o9cq80-user@im.wechat":"AARzJWAFAAAB","broken":null,"empty":""}"#,
        )
        .expect("legacy file");

        assert_eq!(
            load(&home, &group_id),
            BTreeMap::from([(
                "o9cq80-user@im.wechat".to_owned(),
                "AARzJWAFAAAB".to_owned()
            )])
        );
    }

    #[test]
    fn a_missing_or_corrupt_file_restores_nothing() {
        let (_temp, home, group_id) = setup();
        assert!(load(&home, &group_id).is_empty());
        let path = path(&home, &group_id).expect("path");
        std::fs::create_dir_all(path.parent().expect("state dir")).expect("state dir");
        std::fs::write(&path, b"[not an object").expect("corrupt file");
        assert!(load(&home, &group_id).is_empty());
        // A corrupt file must not block recording the next token.
        remember(&home, &group_id, "alice@im.wechat", "token").expect("remember");
        assert_eq!(
            load(&home, &group_id),
            BTreeMap::from([("alice@im.wechat".to_owned(), "token".to_owned())])
        );
    }
}
