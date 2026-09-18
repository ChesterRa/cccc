use std::borrow::Cow;
use std::sync::OnceLock;

use sha2::{Digest, Sha256};

use crate::WebAssets;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebAssetsInfo {
    pub assets_id: String,
    pub entry_script: String,
}

/// Inspect the same asset source as the HTTP handler. Embedded data is immutable;
/// disk-backed debug assets can change without restarting or recompiling Rust.
/// Unavailable assets produce no identity, never a stale compile-time fallback.
pub fn web_assets_info() -> Option<WebAssetsInfo> {
    let index = WebAssets::get("index.html")?;
    let inspect = || {
        inspect_assets(
            &index.data,
            WebAssets::iter().map(|name| name.into_owned()).collect(),
            |name| WebAssets::get(name).map(|file| file.data),
        )
    };
    if matches!(index.data, Cow::Borrowed(_)) {
        static INFO: OnceLock<Option<WebAssetsInfo>> = OnceLock::new();
        INFO.get_or_init(inspect).clone()
    } else {
        inspect()
    }
}

fn inspect_assets(
    index: &[u8],
    mut names: Vec<String>,
    read: impl Fn(&str) -> Option<Cow<'static, [u8]>>,
) -> Option<WebAssetsInfo> {
    let html = String::from_utf8_lossy(index);
    let entry_script = html
        .split("<script")
        .skip(1)
        .find_map(|script| {
            let tag = script.split('>').next()?;
            if !tag.contains("type=\"module\"") {
                return None;
            }
            tag.split("src=\"").nth(1)?.split('"').next()
        })
        .unwrap_or("")
        .to_owned();
    names.sort();
    let mut hash = Sha256::new();
    for name in names {
        // Use the same index bytes for the fingerprint and the entry identity.
        let bytes = if name == "index.html" {
            Cow::Borrowed(index)
        } else {
            read(&name)?
        };
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(&bytes);
    }
    Some(WebAssetsInfo {
        assets_id: format!("{:x}", hash.finalize()),
        entry_script,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn disk_assets_track_rebuilds_and_public_file_changes() {
        let temp = tempfile::tempdir().expect("asset fixture");
        let root = temp.path();
        let inspect = || {
            let index = fs::read(root.join("index.html")).ok()?;
            let names = fs::read_dir(root)
                .expect("asset directory")
                .map(|entry| {
                    entry
                        .expect("asset")
                        .file_name()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
            inspect_assets(&index, names, |name| {
                fs::read(root.join(name)).ok().map(Cow::Owned)
            })
        };
        let entry =
            |name: &str| format!(r#"<script type="module" src="/ui/assets/{name}.js"></script>"#);
        fs::write(root.join("index.html"), entry("first")).expect("index");
        fs::write(root.join("logo.svg"), "first logo").expect("logo");
        let first = inspect().expect("first identity");
        assert_eq!(first.entry_script, "/ui/assets/first.js");

        fs::write(root.join("index.html"), entry("second")).expect("rebuilt index");
        let second = inspect().expect("rebuilt identity");
        assert_eq!(second.entry_script, "/ui/assets/second.js");
        assert_ne!(first.assets_id, second.assets_id);

        fs::write(root.join("logo.svg"), "second logo").expect("changed public file");
        let third = inspect().expect("changed public identity");
        assert_eq!(second.entry_script, third.entry_script);
        assert_ne!(second.assets_id, third.assets_id);
        fs::remove_file(root.join("index.html")).expect("rebuild gap");
        assert!(inspect().is_none());
    }

    #[test]
    fn missing_asset_never_produces_a_partial_fingerprint() {
        assert!(inspect_assets(b"index", vec!["missing.js".into()], |_| None).is_none());
    }
}
