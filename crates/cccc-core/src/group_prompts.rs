use std::io;
use std::path::PathBuf;

use crate::{GroupStore, fs};

pub const PREAMBLE_FILENAME: &str = "CCCC_PREAMBLE.md";
pub const DEFAULT_PREAMBLE_BODY: &str = "Startup:\n- On cold start or resume, use MCP tool `cccc_bootstrap`.\n- Call `cccc_help` only when you need a CCCC-specific route or a missing capability.";
pub const MAX_PROMPT_BYTES: usize = 512 * 1024;

pub struct PromptFile {
    pub path: PathBuf,
    pub found: bool,
    pub content: Option<String>,
}

pub fn read_preamble(store: &GroupStore, group_id: &str) -> io::Result<PromptFile> {
    let path = preamble_path(store, group_id)?;
    if !path.is_file() {
        return Ok(PromptFile {
            path,
            found: false,
            content: None,
        });
    }
    let content = std::fs::read(&path).ok().map(|mut bytes| {
        bytes.truncate(MAX_PROMPT_BYTES);
        String::from_utf8_lossy(&bytes).into_owned()
    });
    Ok(PromptFile {
        path,
        found: true,
        content,
    })
}

pub fn write_preamble(store: &GroupStore, group_id: &str, content: &str) -> io::Result<()> {
    if content.len() > MAX_PROMPT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("prompt content exceeds {MAX_PROMPT_BYTES} UTF-8 bytes"),
        ));
    }
    let path = preamble_path(store, group_id)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    fs::atomic_write(&path, content.as_bytes())
}

pub fn delete_preamble(store: &GroupStore, group_id: &str) -> io::Result<()> {
    let path = preamble_path(store, group_id)?;
    if !path.is_file() {
        return Ok(());
    }
    std::fs::remove_file(path)
}

fn preamble_path(store: &GroupStore, group_id: &str) -> io::Result<PathBuf> {
    store
        .group_dir(group_id)
        .map(|root| root.join("prompts").join(PREAMBLE_FILENAME))
}

#[cfg(test)]
mod tests {
    use super::{MAX_PROMPT_BYTES, read_preamble, write_preamble};
    use crate::{GroupStore, HomeLayout};

    #[test]
    fn preamble_write_rejects_oversized_utf8_without_replacing_content() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let group = store.create("test", "").expect("group");
        write_preamble(&store, &group.group_id, "existing").expect("initial preamble");

        let error = write_preamble(&store, &group.group_id, &"界".repeat(MAX_PROMPT_BYTES))
            .expect_err("oversized UTF-8 preamble");

        assert!(error.to_string().contains("524288 UTF-8 bytes"));
        assert_eq!(
            read_preamble(&store, &group.group_id)
                .expect("read preamble")
                .content
                .as_deref(),
            Some("existing")
        );
    }
}
