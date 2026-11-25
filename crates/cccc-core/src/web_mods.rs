//! Web mods: instance-wide packages under `<home>/web_mods/<id>/` that restyle (and,
//! once approved, extend) the Web UI. This module reads packages, keeps the enabled
//! switch in `state/web_mods.json`, and resolves mod files without escaping a mod's
//! directory. Scripts are listed but never served as runnable here; approval is a
//! later step.
use crate::HomeLayout;
use crate::fs::atomic_write;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

pub const MODS_DIR: &str = "web_mods";
pub const STATE_FILE: &str = "state/web_mods.json";
/// The extension API version this build implements.
pub const API_VERSION: u64 = 1;
pub const MAX_FILE_BYTES: u64 = 256 * 1024;
pub const MAX_MOD_BYTES: u64 = 2 * 1024 * 1024;
const MAX_ID_LEN: usize = 64;

#[derive(Debug, Clone, Deserialize)]
struct Manifest {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    api: u64,
    #[serde(default)]
    description: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    style: Option<String>,
    #[serde(default)]
    script: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ModFile {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ModStatus {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api: u64,
    pub description: String,
    pub author: String,
    pub enabled: bool,
    pub style: Option<ModFile>,
    /// Present when the package has a script. Scripts do not load in this version.
    pub script: Option<ModFile>,
    /// Why the package cannot load, if it cannot.
    pub error: Option<String>,
}

pub fn mods_dir(home: &HomeLayout) -> PathBuf {
    home.root().join(MODS_DIR)
}

pub fn state_path(home: &HomeLayout) -> PathBuf {
    home.root().join(STATE_FILE)
}

/// Lowercase letters, digits and dashes; starts with a letter or digit.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_LEN
        && !id.starts_with('-')
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn load_state(home: &HomeLayout) -> io::Result<Map<String, Value>> {
    match fs::read(state_path(home)) {
        Ok(bytes) => {
            let value: Value = serde_json::from_slice(&bytes)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            Ok(value
                .get("mods")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Map::new()),
        Err(error) => Err(error),
    }
}

fn enabled_in(state: &Map<String, Value>, id: &str) -> bool {
    state
        .get(id)
        .and_then(|entry| entry.get("enabled"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// Every package under `web_mods/`, sorted by id. A package that cannot load is
/// listed with its error so it can be fixed, and is never treated as enabled.
pub fn list(home: &HomeLayout) -> io::Result<Vec<ModStatus>> {
    let state = load_state(home)?;
    let dir = mods_dir(home);
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut mods = Vec::new();
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let Some(id) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !valid_id(&id) {
            continue;
        }
        mods.push(read_package(&entry.path(), &id, enabled_in(&state, &id)));
    }
    mods.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(mods)
}

fn failed(id: &str, error: impl Into<String>) -> ModStatus {
    ModStatus {
        id: id.into(),
        name: id.into(),
        version: String::new(),
        api: 0,
        description: String::new(),
        author: String::new(),
        // A package that cannot load never counts as enabled.
        enabled: false,
        style: None,
        script: None,
        error: Some(error.into()),
    }
}

fn read_package(dir: &Path, id: &str, enabled: bool) -> ModStatus {
    let manifest: Manifest = match fs::read(dir.join("mod.json"))
        .map_err(|error| format!("mod.json: {error}"))
        .and_then(|bytes| {
            serde_json::from_slice(&bytes).map_err(|error| format!("mod.json: {error}"))
        }) {
        Ok(manifest) => manifest,
        Err(error) => return failed(id, error),
    };
    if manifest.id != id {
        return failed(id, "mod.json id does not match its directory");
    }
    if manifest.api != API_VERSION {
        return failed(
            id,
            format!(
                "written for extension API {}, this build supports {API_VERSION}",
                manifest.api
            ),
        );
    }
    if let Err(error) = package_size(dir) {
        return failed(id, error.to_string());
    }
    let file = |relative: &Option<String>| -> Result<Option<ModFile>, String> {
        let Some(relative) = relative.as_deref().filter(|value| !value.trim().is_empty()) else {
            return Ok(None);
        };
        let path = resolve_within(dir, relative).map_err(|error| format!("{relative}: {error}"))?;
        let bytes = fs::read(&path).map_err(|error| format!("{relative}: {error}"))?;
        Ok(Some(ModFile {
            path: relative.trim().to_owned(),
            sha256: format!("{:x}", Sha256::digest(&bytes)),
        }))
    };
    let (style, script) = match (file(&manifest.style), file(&manifest.script)) {
        (Ok(style), Ok(script)) => (style, script),
        (Err(error), _) | (_, Err(error)) => return failed(id, error),
    };
    ModStatus {
        id: id.into(),
        name: if manifest.name.trim().is_empty() {
            id.into()
        } else {
            manifest.name
        },
        version: manifest.version,
        api: manifest.api,
        description: manifest.description,
        author: manifest.author,
        enabled,
        style,
        script,
        error: None,
    }
}

fn package_size(dir: &Path) -> io::Result<()> {
    fn walk(path: &Path, total: &mut u64) -> io::Result<()> {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                return Err(io::Error::other("mod packages may not contain links"));
            }
            if kind.is_dir() {
                walk(&entry.path(), total)?;
                continue;
            }
            let size = entry.metadata()?.len();
            if size > MAX_FILE_BYTES {
                return Err(io::Error::other(format!(
                    "{} exceeds {} KiB",
                    entry.file_name().to_string_lossy(),
                    MAX_FILE_BYTES / 1024
                )));
            }
            *total += size;
        }
        Ok(())
    }
    let mut total = 0;
    walk(dir, &mut total)?;
    if total > MAX_MOD_BYTES {
        return Err(io::Error::other(format!(
            "package exceeds {} MiB",
            MAX_MOD_BYTES / 1024 / 1024
        )));
    }
    Ok(())
}

/// A file inside `dir`, given a relative path of plain components.
fn resolve_within(dir: &Path, relative: &str) -> io::Result<PathBuf> {
    let relative = Path::new(relative.trim());
    if relative.as_os_str().is_empty()
        || !relative
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path must stay inside the mod",
        ));
    }
    let base = dir.canonicalize()?;
    let path = base.join(relative).canonicalize()?;
    if !path.starts_with(&base) || !path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "mod file not found",
        ));
    }
    Ok(path)
}

/// A file of mod `id` for serving. Only files of a loadable package are served.
pub fn resolve_file(home: &HomeLayout, id: &str, relative: &str) -> io::Result<PathBuf> {
    if !valid_id(id) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid mod id",
        ));
    }
    let dir = mods_dir(home).join(id);
    let status = read_package(&dir, id, false);
    if let Some(error) = status.error {
        return Err(io::Error::new(io::ErrorKind::InvalidData, error));
    }
    let path = resolve_within(&dir, relative)?;
    if fs::metadata(&path)?.len() > MAX_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "mod file too large",
        ));
    }
    Ok(path)
}

/// Switch mod `id` on or off. The package must exist and load.
pub fn set_enabled(home: &HomeLayout, id: &str, enabled: bool) -> io::Result<ModStatus> {
    if !valid_id(id) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid mod id",
        ));
    }
    let dir = mods_dir(home).join(id);
    if !dir.is_dir() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "mod not found"));
    }
    let status = read_package(&dir, id, enabled);
    if enabled && let Some(error) = &status.error {
        return Err(io::Error::new(io::ErrorKind::InvalidData, error.clone()));
    }
    let mut state = load_state(home)?;
    state.insert(
        id.into(),
        json!({"enabled": enabled, "updated_at": cccc_contracts::utc_now()}),
    );
    let document = json!({"v": 1, "mods": state});
    let mut bytes = serde_json::to_vec_pretty(&document).map_err(io::Error::other)?;
    bytes.push(b'\n');
    atomic_write(&state_path(home), &bytes)?;
    Ok(status)
}

/// Changes whenever any listed mod, its enabled switch or a served file changes.
pub fn revision(mods: &[ModStatus]) -> String {
    let mut hasher = Sha256::new();
    for status in mods {
        hasher.update(serde_json::to_vec(status).unwrap_or_default());
        hasher.update(b"\n");
    }
    format!("{:x}", hasher.finalize())[..16].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> (tempfile::TempDir, HomeLayout) {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        fs::create_dir_all(home.root()).expect("home dir");
        (temp, home)
    }

    fn write_mod(home: &HomeLayout, id: &str, manifest: Value, files: &[(&str, &str)]) {
        let dir = mods_dir(home).join(id);
        fs::create_dir_all(&dir).expect("mod dir");
        fs::write(dir.join("mod.json"), manifest.to_string()).expect("manifest");
        for (name, body) in files {
            let path = dir.join(name);
            fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
            fs::write(path, body).expect("file");
        }
    }

    #[test]
    fn lists_packages_disabled_until_switched_on_with_style_hashes() {
        let (_temp, home) = home();
        assert!(list(&home).expect("empty").is_empty());
        write_mod(
            &home,
            "glass",
            json!({"id":"glass","name":"Glass","version":"1","api":1,"style":"style.css"}),
            &[("style.css", "body{}")],
        );
        let mods = list(&home).expect("list");
        assert_eq!(mods.len(), 1);
        assert!(!mods[0].enabled);
        assert_eq!(mods[0].style.as_ref().expect("style").path, "style.css");
        let before = revision(&mods);

        set_enabled(&home, "glass", true).expect("enable");
        let mods = list(&home).expect("list");
        assert!(mods[0].enabled);
        assert_ne!(revision(&mods), before);

        fs::write(mods_dir(&home).join("glass/style.css"), "body{color:red}").expect("edit");
        let edited = list(&home).expect("list");
        assert_ne!(revision(&edited), revision(&mods));
    }

    #[test]
    fn a_package_that_cannot_load_is_listed_with_its_error_and_never_enabled() {
        let (_temp, home) = home();
        write_mod(&home, "wrong-id", json!({"id":"other","api":1}), &[]);
        write_mod(&home, "future", json!({"id":"future","api":2}), &[]);
        write_mod(
            &home,
            "missing",
            json!({"id":"missing","api":1,"style":"nope.css"}),
            &[],
        );
        let mods = list(&home).expect("list");
        assert_eq!(mods.len(), 3);
        assert!(
            mods.iter()
                .all(|status| status.error.is_some() && !status.enabled)
        );
        assert!(set_enabled(&home, "future", true).is_err());
        // A directory that is not a valid id is ignored.
        fs::create_dir_all(mods_dir(&home).join("Bad Id")).expect("dir");
        assert_eq!(list(&home).expect("list").len(), 3);
    }

    #[test]
    fn files_resolve_only_inside_a_loadable_package() {
        let (_temp, home) = home();
        write_mod(
            &home,
            "glass",
            json!({"id":"glass","api":1,"style":"style.css"}),
            &[("style.css", "body{}"), ("assets/a.png", "png")],
        );
        fs::write(home.root().join("secret.txt"), "secret").expect("secret");
        assert!(resolve_file(&home, "glass", "style.css").is_ok());
        assert!(resolve_file(&home, "glass", "assets/a.png").is_ok());
        for escape in [
            "../../secret.txt",
            "/etc/hosts",
            "assets/../../../secret.txt",
            "",
        ] {
            assert!(resolve_file(&home, "glass", escape).is_err(), "{escape}");
        }
        assert!(resolve_file(&home, "../glass", "style.css").is_err());
        // An oversized style stops the package from loading at all.
        fs::write(
            mods_dir(&home).join("glass/style.css"),
            vec![b'a'; (MAX_FILE_BYTES + 1) as usize],
        )
        .expect("big");
        assert!(resolve_file(&home, "glass", "style.css").is_err());
        assert!(list(&home).expect("list")[0].error.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn a_link_inside_a_package_stops_it_loading() {
        let (_temp, home) = home();
        write_mod(
            &home,
            "glass",
            json!({"id":"glass","api":1,"style":"style.css"}),
            &[("style.css", "")],
        );
        fs::write(home.root().join("secret.txt"), "secret").expect("secret");
        std::os::unix::fs::symlink(
            home.root().join("secret.txt"),
            mods_dir(&home).join("glass/leak.css"),
        )
        .expect("link");
        assert!(list(&home).expect("list")[0].error.is_some());
        assert!(resolve_file(&home, "glass", "leak.css").is_err());
    }

    #[test]
    fn ids_are_lowercase_words_joined_by_dashes() {
        assert!(valid_id("liquid-glass"));
        assert!(valid_id("a1"));
        for bad in ["", "-x", "Glass", "a b", "a/b", "..", &"a".repeat(65)] {
            assert!(!valid_id(bad), "{bad}");
        }
    }
}
