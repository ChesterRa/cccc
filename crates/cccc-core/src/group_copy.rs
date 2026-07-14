use cccc_contracts::{GroupState, utc_now};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io::{self, Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::fs::{atomic_write, write_yaml};
use crate::{GroupDoc, GroupStore, Registry, Scope, scope};

const KIND: &str = "cccc.group_copy";
const VERSION: u8 = 1;
pub const MAX_PACKAGE_BYTES: usize = 1024 * 1024 * 1024;
const MAX_UNCOMPRESSED_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_FILE_COUNT: usize = 20_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    pub kind: String,
    pub version: u8,
    pub source_group_id: String,
    pub source_title: String,
    pub exported_at: String,
    pub cccc_version: String,
    pub source_platform: String,
    pub export_mode: String,
    pub workspace_included: bool,
    pub contains_secrets: bool,
    pub content_digest: String,
    pub content: ContentSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContentSummary {
    pub ledger: bool,
    pub context: bool,
    pub blobs: bool,
    pub memory: bool,
    pub assistants: bool,
    pub automation: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PreviewActor {
    pub id: String,
    pub title: String,
    pub runtime: String,
    pub runner: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Preview {
    pub manifest: Manifest,
    pub source_group_id: String,
    pub source_title: String,
    pub actor_count: usize,
    pub actors: Vec<PreviewActor>,
    pub source_workspace_root: String,
    pub workspace_root_exists: bool,
    pub group_id_conflict: bool,
    pub target_default_scope_conflict: bool,
    pub requires_reconnect: RequiresReconnect,
    pub workspace_included: bool,
    pub contains_secrets: bool,
    pub runtime_reset: RuntimeReset,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RequiresReconnect {
    pub chatgpt_web_model: bool,
    pub notebooklm_group_space: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RuntimeReset {
    pub actors_stopped: bool,
    pub group_running: bool,
    pub group_state: String,
    pub browser_sessions_cleared: bool,
    pub runtime_sessions_cleared: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ImportResult {
    pub group_id: String,
    pub source_group_id: String,
    pub group_id_conflict: bool,
    pub workspace_root: String,
    pub active_scope_key: String,
}

pub fn export(store: &GroupStore, group_id: &str) -> io::Result<(Vec<u8>, Manifest, String)> {
    let mut group = store.load(group_id)?;
    scrub_group(&mut group);
    let mut files = collect_files(&store.group_dir(group_id)?)?;
    files.insert(
        "group.yaml".into(),
        serde_yaml::to_string(&group)
            .map_err(io::Error::other)?
            .into_bytes(),
    );
    let digest = content_digest(&files);
    let paths = files.keys().cloned().collect::<Vec<_>>();
    let manifest = Manifest {
        kind: KIND.into(),
        version: VERSION,
        source_group_id: group.group_id.clone(),
        source_title: group.title.clone(),
        exported_at: utc_now(),
        cccc_version: env!("CARGO_PKG_VERSION").into(),
        source_platform: std::env::consts::OS.into(),
        export_mode: "group_state_only".into(),
        workspace_included: false,
        contains_secrets: false,
        content_digest: digest,
        content: ContentSummary {
            ledger: paths.iter().any(|path| path == "ledger.jsonl"),
            context: paths.iter().any(|path| path.starts_with("context/")),
            blobs: paths.iter().any(|path| path.starts_with("state/blobs/")),
            memory: paths.iter().any(|path| path.starts_with("state/memory")),
            assistants: false,
            automation: true,
        },
    };
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    writer
        .start_file("manifest.json", options)
        .map_err(io::Error::other)?;
    writer.write_all(&serde_json::to_vec_pretty(&manifest).map_err(io::Error::other)?)?;
    for (relative, data) in &files {
        writer
            .start_file(format!("group/{relative}"), options)
            .map_err(io::Error::other)?;
        writer.write_all(data)?;
    }
    let bytes = writer.finish().map_err(io::Error::other)?.into_inner();
    if bytes.len() > MAX_PACKAGE_BYTES {
        return Err(io::Error::other("group copy package exceeds 1 GiB"));
    }
    let filename = format!(
        "cccc-group--{}--{}--{}.zip",
        slug(&group.title),
        group.group_id,
        manifest
            .exported_at
            .replace([':', '-'], "")
            .chars()
            .take(15)
            .collect::<String>()
    );
    Ok((bytes, manifest, filename))
}

pub fn preview(store: &GroupStore, bytes: &[u8]) -> io::Result<Preview> {
    let package = read_package(bytes)?;
    preview_package(store, package.manifest, &package.group)
}

pub fn import(
    store: &GroupStore,
    bytes: &[u8],
    workspace_root: &str,
    title: &str,
) -> io::Result<ImportResult> {
    let package = read_package(bytes)?;
    let source_group_id = package.group.group_id.clone();
    let conflict = store.group_dir(&source_group_id)?.exists();
    let final_group_id = if conflict {
        format!("g_{}", &Uuid::new_v4().simple().to_string()[..12])
    } else {
        source_group_id.clone()
    };
    let mut group = package.group;
    group.group_id.clone_from(&final_group_id);
    if !title.trim().is_empty() {
        group.title = title.trim().into();
    }
    group.running = false;
    group.state = GroupState::Idle;
    let old_scope = group.active_scope_key.clone();
    if !workspace_root.trim().is_empty() {
        let root = Path::new(workspace_root.trim()).canonicalize()?;
        if root == store.home().root() {
            return Err(io::Error::other(
                "workspace_root must be a project directory, not CCCC_HOME",
            ));
        }
        let mut detected = scope::detect(&root)?;
        if conflict
            || Registry::load(store.home())?
                .defaults
                .get(&detected.scope_key)
                .is_some_and(|owner| owner != &final_group_id)
        {
            detected.scope_key = format!("s_{}", &Uuid::new_v4().simple().to_string()[..12]);
        }
        replace_active_scope(&mut group, detected, &old_scope);
    }
    scrub_group(&mut group);
    let imported = store.import(group.clone())?;
    let target = store.group_dir(&final_group_id)?;
    let result = (|| {
        for (relative, data) in package.files {
            if relative == "group.yaml" || excluded(&relative, false) {
                continue;
            }
            let path = target.join(safe_relative(&relative)?);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            atomic_write(&path, &data)?;
        }
        rewrite_ledger(
            &target.join("ledger.jsonl"),
            &source_group_id,
            &final_group_id,
            &old_scope,
            &imported.active_scope_key,
        )?;
        write_yaml(&target.join("group.yaml"), &imported)?;
        Ok(ImportResult {
            group_id: final_group_id.clone(),
            source_group_id,
            group_id_conflict: conflict,
            workspace_root: active_workspace(&imported),
            active_scope_key: imported.active_scope_key,
        })
    })();
    if result.is_err() {
        let _ = store.delete(&final_group_id);
    }
    result
}

struct Package {
    manifest: Manifest,
    files: HashMap<String, Vec<u8>>,
    group: GroupDoc,
}

fn read_package(bytes: &[u8]) -> io::Result<Package> {
    if bytes.is_empty() || bytes.len() > MAX_PACKAGE_BYTES {
        return Err(io::Error::other("invalid group copy package size"));
    }
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(io::Error::other)?;
    if archive.len() > MAX_FILE_COUNT {
        return Err(io::Error::other("group copy package has too many entries"));
    }
    let mut names = BTreeSet::new();
    let mut folded = BTreeSet::new();
    let mut total = 0_u64;
    let mut manifest = None;
    let mut files = HashMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(io::Error::other)?;
        if entry.is_dir() {
            continue;
        }
        let name = safe_zip_name(entry.name())?;
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(io::Error::other(
                "symbolic links are not allowed in group copies",
            ));
        }
        if !names.insert(name.clone()) || !folded.insert(name.to_ascii_lowercase()) {
            return Err(io::Error::other("duplicate group copy entry"));
        }
        total = total.saturating_add(entry.size());
        if total > MAX_UNCOMPRESSED_BYTES {
            return Err(io::Error::other("group copy expands beyond 2 GiB"));
        }
        let mut data = Vec::with_capacity(entry.size().min(8 * 1024 * 1024) as usize);
        entry.read_to_end(&mut data)?;
        if name == "manifest.json" {
            manifest = Some(serde_json::from_slice::<Manifest>(&data).map_err(io::Error::other)?);
        } else if let Some(relative) = name.strip_prefix("group/") {
            files.insert(relative.into(), data);
        }
    }
    let manifest = manifest.ok_or_else(|| io::Error::other("manifest.json missing"))?;
    validate_manifest(&manifest)?;
    if manifest.content_digest != content_digest(&files) {
        return Err(io::Error::other("group copy content digest mismatch"));
    }
    let group: GroupDoc = serde_yaml::from_slice(
        files
            .get("group.yaml")
            .ok_or_else(|| io::Error::other("group/group.yaml missing"))?,
    )
    .map_err(io::Error::other)?;
    Ok(Package {
        manifest,
        files,
        group,
    })
}

fn validate_manifest(manifest: &Manifest) -> io::Result<()> {
    if manifest.kind != KIND
        || manifest.version != VERSION
        || manifest.export_mode != "group_state_only"
        || manifest.workspace_included
        || manifest.contains_secrets
    {
        Err(io::Error::other("unsupported group copy manifest"))
    } else {
        Ok(())
    }
}

fn preview_package(
    store: &GroupStore,
    manifest: Manifest,
    group: &GroupDoc,
) -> io::Result<Preview> {
    let actors = group
        .actors
        .iter()
        .filter(|actor| actor.internal_kind.is_none())
        .map(|actor| PreviewActor {
            id: actor.id.clone(),
            title: actor.title.clone(),
            runtime: serde_json::to_value(actor.runtime)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_default(),
            runner: serde_json::to_value(actor.runner)
                .ok()
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_default(),
            enabled: actor.enabled,
        })
        .collect::<Vec<_>>();
    let workspace = active_workspace(group);
    let registry = Registry::load(store.home())?;
    Ok(Preview {
        manifest,
        source_group_id: group.group_id.clone(),
        source_title: group.title.clone(),
        actor_count: actors.len(),
        requires_reconnect: RequiresReconnect {
            chatgpt_web_model: actors.iter().any(|actor| actor.runtime == "web_model"),
            notebooklm_group_space: serde_json::to_string(group)
                .unwrap_or_default()
                .to_ascii_lowercase()
                .contains("notebooklm"),
        },
        actors,
        source_workspace_root: workspace.clone(),
        workspace_root_exists: Path::new(&workspace).exists(),
        group_id_conflict: store.group_dir(&group.group_id)?.exists(),
        target_default_scope_conflict: registry
            .defaults
            .get(&group.active_scope_key)
            .is_some_and(|owner| owner != &group.group_id),
        workspace_included: false,
        contains_secrets: false,
        runtime_reset: RuntimeReset {
            actors_stopped: true,
            group_running: false,
            group_state: "idle".into(),
            browser_sessions_cleared: true,
            runtime_sessions_cleared: true,
        },
    })
}

fn collect_files(root: &Path) -> io::Result<HashMap<String, Vec<u8>>> {
    let mut output = HashMap::new();
    collect_dir(root, root, &mut output)?;
    Ok(output)
}

fn collect_dir(
    root: &Path,
    directory: &Path,
    output: &mut HashMap<String, Vec<u8>>,
) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .map_err(io::Error::other)?
            .to_string_lossy()
            .replace('\\', "/");
        let kind = entry.file_type()?;
        if kind.is_symlink() || excluded(&relative, kind.is_dir()) {
            continue;
        }
        if kind.is_dir() {
            collect_dir(root, &path, output)?;
        } else if kind.is_file() {
            output.insert(relative, fs::read(path)?);
        }
    }
    Ok(())
}

fn excluded(relative: &str, is_dir: bool) -> bool {
    let lower = relative.to_ascii_lowercase();
    let parts = lower.split('/').collect::<Vec<_>>();
    let sensitive = [
        "secret",
        "secrets",
        "token",
        "tokens",
        "credential",
        "credentials",
        "auth",
        "password",
        "env_private",
    ];
    let runtime = [
        "browser",
        "browsers",
        "cdp",
        "chrome",
        "chrome_profile",
        "headless",
        "playwright",
        "projections",
        "pty",
        "runners",
        "runtime_sessions",
        "web_model",
    ];
    parts
        .iter()
        .any(|part| sensitive.iter().any(|word| part.contains(word)))
        || is_dir && parts.iter().any(|part| runtime.contains(part))
        || lower.ends_with(".pid")
        || lower.ends_with(".sock")
        || lower.ends_with(".lock")
        || lower.ends_with(".sqlite")
        || lower.ends_with(".sqlite3")
        || lower == "state/assistants.json"
}

fn scrub_group(group: &mut GroupDoc) {
    group.running = false;
    for actor in &mut group.actors {
        actor.env.retain(|key, _| {
            let key = key.to_ascii_lowercase();
            ![
                "secret",
                "token",
                "password",
                "credential",
                "cookie",
                "auth",
            ]
            .iter()
            .any(|word| key.contains(word))
        });
    }
}

fn replace_active_scope(group: &mut GroupDoc, scope: Scope, old_key: &str) {
    let new_key = scope.scope_key.clone();
    if let Some(existing) = group
        .scopes
        .iter_mut()
        .find(|item| item.scope_key == old_key)
    {
        *existing = scope;
    } else {
        group.scopes.insert(0, scope);
    }
    group.active_scope_key.clone_from(&new_key);
    for actor in &mut group.actors {
        if actor.default_scope_key == old_key {
            actor.default_scope_key.clone_from(&new_key);
        }
    }
}

fn rewrite_ledger(
    path: &Path,
    old_group: &str,
    new_group: &str,
    old_scope: &str,
    new_scope: &str,
) -> io::Result<()> {
    if !path.exists() || old_group == new_group && old_scope == new_scope {
        return Ok(());
    }
    let raw = fs::read_to_string(path)?;
    let mut output = String::new();
    for line in raw.lines() {
        let Ok(mut value) = serde_json::from_str::<serde_json::Value>(line) else {
            output.push_str(line);
            output.push('\n');
            continue;
        };
        if value.get("group_id").and_then(|value| value.as_str()) == Some(old_group) {
            value["group_id"] = serde_json::Value::String(new_group.into());
        }
        if !old_scope.is_empty()
            && value.get("scope_key").and_then(|value| value.as_str()) == Some(old_scope)
        {
            value["scope_key"] = serde_json::Value::String(new_scope.into());
        }
        output.push_str(&serde_json::to_string(&value).map_err(io::Error::other)?);
        output.push('\n');
    }
    atomic_write(path, output.as_bytes())
}

fn safe_zip_name(name: &str) -> io::Result<String> {
    if name.is_empty()
        || name.contains('\\')
        || name.starts_with('/')
        || name.bytes().any(|byte| byte < 32)
    {
        return Err(io::Error::other("invalid group copy entry path"));
    }
    let path = Path::new(name);
    if path.components().any(|part| {
        matches!(
            part,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(io::Error::other("group copy path traversal rejected"));
    }
    Ok(name.into())
}

fn safe_relative(name: &str) -> io::Result<PathBuf> {
    safe_zip_name(name).map(PathBuf::from)
}

fn content_digest(files: &HashMap<String, Vec<u8>>) -> String {
    let mut names = files.keys().collect::<Vec<_>>();
    names.sort();
    let mut digest = Sha256::new();
    for name in names {
        digest.update(name.as_bytes());
        digest.update([0]);
        digest.update(Sha256::digest(&files[name]));
        digest.update([b'\n']);
    }
    format!("{:x}", digest.finalize())
}

fn active_workspace(group: &GroupDoc) -> String {
    group
        .scopes
        .iter()
        .find(|scope| scope.scope_key == group.active_scope_key)
        .or_else(|| group.scopes.first())
        .map(|scope| scope.url.clone())
        .unwrap_or_default()
}

fn slug(value: &str) -> String {
    let mut slug = value
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "group".into()
    } else {
        slug.chars().take(80).collect()
    }
}
