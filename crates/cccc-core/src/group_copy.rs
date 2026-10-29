use cccc_contracts::{Event, GroupState, utc_now};
use flate2::Compression;
use flate2::bufread::GzDecoder;
use flate2::write::GzEncoder;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io::{self, BufRead, BufReader, Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use uuid::Uuid;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::{GroupDoc, GroupStore, Registry, Scope, scope};

const KIND: &str = "cccc.group_copy";
const VERSION: u8 = 1;
pub const MAX_PACKAGE_BYTES: usize = 1024 * 1024 * 1024;
const MAX_UNCOMPRESSED_BYTES: u64 = 2 * 1024 * 1024 * 1024;
// IPC requests are limited to 16 MiB; leave room for older or locally written
// events while bounding the extra memory needed to rewrite one history line.
const MAX_REWRITE_LINE_BYTES: usize = 64 * 1024 * 1024;
const MAX_FILE_COUNT: usize = 20_000;
const MAX_COMPRESSION_RATIO: f64 = 200.0;

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
    pub native_session: bool,
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
    pub grok_web_model: bool,
    pub notebooklm_group_space: bool,
    pub native_sessions: bool,
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
    let _operation = operation_guard();
    let mut group = store.load(group_id)?;
    scrub_group(&mut group);
    let mut files = collect_files(&store.group_dir(group_id)?)?;
    files.insert(
        "group.yaml".into(),
        // JSON is valid YAML and keeps ISO timestamps as strings when Python's
        // yaml.safe_load reads a Rust-created package.
        serde_json::to_vec_pretty(&group).map_err(io::Error::other)?,
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
    let _operation = operation_guard();
    let package = read_package(bytes)?;
    preview_package(store, package.manifest, &package.group)
}

pub fn import(
    store: &GroupStore,
    bytes: &[u8],
    workspace_root: &str,
    title: &str,
) -> io::Result<ImportResult> {
    let _operation = operation_guard();
    let package = read_package(bytes)?;
    let source_group_id = package.group.group_id.clone();
    let registry = Registry::load(store.home())?;
    let conflict = store.group_dir(&source_group_id)?.exists()
        || registry.groups.contains_key(&source_group_id);
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
            || registry
                .defaults
                .get(&detected.scope_key)
                .is_some_and(|owner| owner != &final_group_id)
        {
            detected.scope_key = format!("s_{}", &Uuid::new_v4().simple().to_string()[..12]);
        }
        replace_active_scope(&mut group, detected, &old_scope);
    }
    scrub_group(&mut group);
    group.extra.remove("local_session_conversation");
    for actor in &mut group.actors {
        if !actor.external_session_id.is_empty() {
            // A copied binding retains its identity and history, not live access
            // to the same native source in this CCCC_HOME.
            actor.enabled = false;
        }
    }
    sanitize_import_profiles(store, &mut group)?;
    let mut files = package.files;
    let identity_changes = source_group_id != final_group_id || old_scope != group.active_scope_key;
    // Outer ZIP entries have already been expanded into memory. Charge each
    // nested gzip member's decoded bytes to that same total as well.
    let mut decoded_bytes = package.uncompressed_bytes;
    let imported = store.import_prepared(group, |target, imported| {
        let mut event = Event::new("group.create", &final_group_id);
        event.by = "system".into();
        event
            .data
            .insert("title".into(), imported.title.clone().into());
        event
            .data
            .insert("source_group_id".into(), source_group_id.clone().into());
        event.data.insert("imported".into(), true.into());
        // The marker follows the copied history and precedes any future append.
        // The staged Group is not visible while this ledger is constructed.
        let history = files.remove("ledger.jsonl").unwrap_or_default();
        let ledger_path = target.join("ledger.jsonl");
        // import_prepared created this empty file in our private stage. Create
        // the real ledger with private permissions, still respecting umask.
        fs::remove_file(&ledger_path)?;
        let mut ledger = staged_private_file(&ledger_path)?;
        if identity_changes {
            rewrite_history_lines(
                &mut BufReader::new(history.as_slice()),
                &mut ledger,
                &HistoryIdentityRewrite {
                    old_group: &source_group_id,
                    new_group: &final_group_id,
                    old_scope: &old_scope,
                    new_scope: &imported.active_scope_key,
                },
                MAX_REWRITE_LINE_BYTES,
                u64::MAX,
            )?;
        } else {
            ledger.write_all(&history)?;
        }
        if history.last().is_some_and(|last| *last != b'\n') {
            ledger.write_all(b"\n")?;
        }
        serde_json::to_writer(&mut ledger, &event).map_err(io::Error::other)?;
        ledger.write_all(b"\n")?;
        ledger.sync_all()?;
        for (relative, data) in files {
            if relative == "group.yaml" || excluded(&relative, false) {
                continue;
            }
            let path = target.join(safe_relative(&relative)?);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let history_segment = is_history_segment(target, &relative)?;
            if identity_changes && history_segment {
                rewrite_segment(
                    &path,
                    &relative,
                    &data,
                    &HistoryIdentityRewrite {
                        old_group: &source_group_id,
                        new_group: &final_group_id,
                        old_scope: &old_scope,
                        new_scope: &imported.active_scope_key,
                    },
                    &mut decoded_bytes,
                )?;
            } else {
                if history_segment && relative.ends_with(".gz") {
                    count_unchanged_gzip(&data, &mut decoded_bytes)?;
                }
                // The stage is private and is published as a unit. Never
                // replace a file: filesystem aliases (including NTFS 8.3
                // names) can otherwise overwrite an earlier entry or ledger.
                let mut file = staged_private_file(&path)?;
                file.write_all(&data)?;
                file.sync_all()?;
                let parent = path
                    .parent()
                    .ok_or_else(|| io::Error::other("path has no parent"))?;
                crate::fs::sync_dir(parent)?;
            }
        }
        Ok(())
    })?;
    Ok(ImportResult {
        group_id: final_group_id,
        source_group_id,
        group_id_conflict: conflict,
        workspace_root: active_workspace(&imported),
        active_scope_key: imported.active_scope_key,
    })
}

fn operation_guard() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct Package {
    manifest: Manifest,
    files: HashMap<String, Vec<u8>>,
    group: GroupDoc,
    uncompressed_bytes: u64,
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
    let mut total_compressed = 0_u64;
    let mut manifest = None;
    let mut files = HashMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(io::Error::other)?;
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
        if entry.is_dir() {
            continue;
        }
        let declared_size = entry.size();
        if declared_size > MAX_UNCOMPRESSED_BYTES - total {
            return Err(io::Error::other("group copy expands beyond 2 GiB"));
        }
        total_compressed = total_compressed.saturating_add(entry.compressed_size());
        let compressed = entry.compressed_size().max(1);
        if declared_size > 1024 * 1024
            && declared_size as f64 / compressed as f64 > MAX_COMPRESSION_RATIO
        {
            return Err(io::Error::other("suspicious group copy compression ratio"));
        }
        let mut data = Vec::with_capacity(declared_size.min(8 * 1024 * 1024) as usize);
        // ZIP metadata is untrusted: the decoder may yield more than the
        // declared size. Limit the actual read before allocating the output.
        entry
            .by_ref()
            .take(declared_size + 1)
            .read_to_end(&mut data)?;
        if data.len() as u64 != declared_size {
            return Err(io::Error::other(
                "group copy entry size does not match its contents",
            ));
        }
        total += data.len() as u64;
        if name == "manifest.json" {
            manifest = Some(serde_json::from_slice::<Manifest>(&data).map_err(io::Error::other)?);
        } else if let Some(relative) = name.strip_prefix("group/") {
            if ["ledger.jsonl", "group.yaml"]
                .iter()
                .any(|reserved| relative != *reserved && unicase::eq(relative, *reserved))
            {
                return Err(io::Error::other("noncanonical reserved group copy entry"));
            }
            files.insert(relative.into(), data);
        }
    }
    if total_compressed > 0 && total as f64 / total_compressed as f64 > MAX_COMPRESSION_RATIO {
        return Err(io::Error::other("suspicious group copy compression ratio"));
    }
    let manifest = manifest.ok_or_else(|| io::Error::other("manifest.json missing"))?;
    validate_manifest(&manifest)?;
    if !content_digest_matches(&manifest.content_digest, &files) {
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
        uncompressed_bytes: total,
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
            enabled: actor.enabled && actor.external_session_id.is_empty(),
            native_session: !actor.external_session_id.is_empty(),
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
            chatgpt_web_model: actors
                .iter()
                .any(|actor| matches!(actor.runtime.as_str(), "web_model" | "chatgpt_dots")),
            grok_web_model: actors.iter().any(|actor| actor.runtime == "grok_web_model"),
            notebooklm_group_space: serde_json::to_string(group)
                .unwrap_or_default()
                .to_ascii_lowercase()
                .contains("notebooklm"),
            native_sessions: actors.iter().any(|actor| actor.native_session),
        },
        actors,
        source_workspace_root: workspace.clone(),
        workspace_root_exists: Path::new(&workspace).exists(),
        group_id_conflict: store.group_dir(&group.group_id)?.exists()
            || registry.groups.contains_key(&group.group_id),
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
    let mut budget = CollectionBudget::default();
    collect_dir(root, root, &mut output, &mut budget)?;
    Ok(output)
}

#[derive(Default)]
struct CollectionBudget {
    files: usize,
    bytes: u64,
}

fn collect_dir(
    root: &Path,
    directory: &Path,
    output: &mut HashMap<String, Vec<u8>>,
    budget: &mut CollectionBudget,
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
            collect_dir(root, &path, output, budget)?;
        } else if kind.is_file() {
            budget.files = budget.files.saturating_add(1);
            budget.bytes = budget.bytes.saturating_add(entry.metadata()?.len());
            if budget.files > MAX_FILE_COUNT {
                return Err(io::Error::other("group copy has too many files"));
            }
            if budget.bytes > MAX_UNCOMPRESSED_BYTES {
                return Err(io::Error::other("group copy exceeds 2 GiB"));
            }
            output.insert(relative, fs::read(path)?);
        }
    }
    Ok(())
}

fn excluded(relative: &str, _is_dir: bool) -> bool {
    // Sensitive names are ASCII. Unicode case folding also catches aliases on
    // case-insensitive filesystems without rejecting unrelated Unicode names.
    let lower = unicase::UniCase::new(relative).to_folded_case();
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
        "cookie",
        "oauth",
        "session_auth",
    ];
    let runtime = [
        "browser",
        "browsers",
        "cdp",
        "chrome",
        "chrome_profile",
        "claude",
        "codex",
        "devin",
        "grok",
        "headless",
        "hermes",
        "kiro",
        "playwright",
        "projections",
        "pty",
        "runners",
        "runtime_sessions",
        "web_model",
        "grok_web_model",
    ];
    let name = parts.last().copied().unwrap_or_default();
    parts.iter().any(|part| sensitive.contains(part))
        || name.ends_with('~')
        || name.ends_with(".tmp")
        || parts.iter().any(|part| part.starts_with(".deleting-"))
        || [
            ".db", ".db-shm", ".db-wal", ".lock", ".pid", ".sock", ".sqlite", ".sqlite3",
        ]
        .iter()
        .any(|suffix| name.ends_with(suffix))
        || parts.first() == Some(&"runtime")
        || parts.len() >= 2
            && parts[0] == "state"
            && (runtime.contains(&parts[1])
                || matches!(parts[1], "group_space" | "notebooklm" | "notebooklm_auth")
                || parts[1] == "ledger"
                    && parts.len() >= 3
                    && matches!(
                        parts[2],
                        "index.sqlite3"
                            | "ledger.lock"
                            | "manifest.json"
                            | "status.sqlite3"
                            | "tmp"
                    ))
        || lower == "state/preamble_sent.json"
        || lower == "state/unread_index.json"
        || lower == "state/assistants.json"
        || lower == "state/env_private.json"
        || lower == "state/im_weixin_context_tokens.json"
}

fn scrub_group(group: &mut GroupDoc) {
    group.running = false;
    group.extra.remove("im_bridge");
    group.extra.remove("im");
    for actor in &mut group.actors {
        actor.env.clear();
    }
}

fn sanitize_import_profiles(store: &GroupStore, group: &mut GroupDoc) -> io::Result<()> {
    let profiles = crate::profiles::ProfileStore::new(store.home().clone())?;
    for actor in &mut group.actors {
        if !actor.profile_id.is_empty()
            && profiles
                .get_ref(
                    &actor.profile_id,
                    &actor.profile_scope,
                    &actor.profile_owner,
                )?
                .is_none()
        {
            actor.profile_id.clear();
            actor.profile_scope = "global".into();
            actor.profile_owner.clear();
            actor.profile_revision_applied = 0;
        }
    }
    Ok(())
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

// The reader looks only at immediate children of this directory and accepts
// both physical representations of a segment. Rewrite both if both exist.
fn is_history_segment(target: &Path, relative: &str) -> io::Result<bool> {
    let parts = relative.split('/').collect::<Vec<_>>();
    if parts.len() != 4 || !(parts[3].ends_with(".jsonl") || parts[3].ends_with(".jsonl.gz")) {
        return Ok(false);
    }
    let parent = target.join(parts[0]).join(parts[1]).join(parts[2]);
    let segments = target.join("state/ledger/segments");
    // Filesystem identity, not the OS, determines whether differing case is an
    // alias. The stage is private and the candidate parent was just created.
    match same_file::is_same_file(parent, segments) {
        Ok(same) => Ok(same),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

struct HistoryIdentityRewrite<'a> {
    old_group: &'a str,
    new_group: &'a str,
    old_scope: &'a str,
    new_scope: &'a str,
}

#[derive(Deserialize)]
struct HistoryIdentity<'a> {
    #[serde(borrow)]
    group_id: Option<&'a RawValue>,
    #[serde(borrow)]
    scope_key: Option<&'a RawValue>,
}

fn count_unchanged_gzip(data: &[u8], decoded_bytes: &mut u64) -> io::Result<()> {
    // Validate the same first gzip member the ledger reader would read, while
    // retaining the compressed input byte-for-byte when identity is unchanged.
    let remaining = MAX_UNCOMPRESSED_BYTES - *decoded_bytes;
    let mut decoder = GzDecoder::new(data).take(remaining.saturating_add(1));
    let count = io::copy(&mut decoder, &mut io::sink())?;
    if count > remaining {
        return Err(io::Error::other("group copy expands beyond 2 GiB"));
    }
    *decoded_bytes += count;
    Ok(())
}

fn staged_private_file(path: &Path) -> io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

fn rewrite_segment(
    path: &Path,
    relative: &str,
    data: &[u8],
    identity: &HistoryIdentityRewrite<'_>,
    decoded_bytes: &mut u64,
) -> io::Result<()> {
    // The destination lives in the private staging directory. Sync it before
    // publication; errors leave the destination unpublished and staging is
    // removed by import_prepared.
    let output = staged_private_file(path)?;
    if relative.ends_with(".gz") {
        // The ledger reader uses a single-member decoder. A concatenated member
        // or trailing data must not become newly visible after rewriting.
        let decoder = GzDecoder::new(data);
        let mut reader = BufReader::new(decoder);
        let mut encoder = GzEncoder::new(output, Compression::default());
        let decoded = rewrite_history_lines(
            &mut reader,
            &mut encoder,
            identity,
            MAX_REWRITE_LINE_BYTES,
            MAX_UNCOMPRESSED_BYTES - *decoded_bytes,
        )?;
        if !reader.into_inner().into_inner().is_empty() {
            return Err(io::Error::other(
                "group copy history gzip has more than one member or trailing data",
            ));
        }
        encoder.finish()?.sync_all()?;
        *decoded_bytes += decoded;
    } else {
        let mut reader = BufReader::new(data);
        let mut output = output;
        rewrite_history_lines(
            &mut reader,
            &mut output,
            identity,
            MAX_REWRITE_LINE_BYTES,
            u64::MAX,
        )?;
        output.sync_all()?;
    }
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("path has no parent"))?;
    crate::fs::sync_dir(parent)
}

fn rewrite_history_lines(
    reader: &mut impl BufRead,
    writer: &mut impl Write,
    identity: &HistoryIdentityRewrite<'_>,
    line_limit: usize,
    decoded_limit: u64,
) -> io::Result<u64> {
    let mut line = Vec::new();
    let mut decoded = 0_u64;
    loop {
        line.clear();
        let read = reader
            .by_ref()
            .take((line_limit as u64).saturating_add(1))
            .read_until(b'\n', &mut line)?;
        if read == 0 {
            return Ok(decoded);
        }
        if read > line_limit {
            return Err(io::Error::other(
                "group copy history line exceeds 64 MiB while rewriting identity",
            ));
        }
        decoded = decoded.saturating_add(read as u64);
        if decoded > decoded_limit {
            return Err(io::Error::other("group copy expands beyond 2 GiB"));
        }
        write_history_line(writer, &line, identity)?;
    }
}

fn write_history_line(
    writer: &mut impl Write,
    line: &[u8],
    identity: &HistoryIdentityRewrite<'_>,
) -> io::Result<()> {
    // RawValue borrows the actual top-level value's bytes from this line.
    // Nested keys and JSON escapes therefore cannot redirect the replacement.
    // Other fields are copied even if they contain damaged string bytes;
    // strict readers will continue to report those records as unreadable.
    let Ok(parsed) = serde_json::from_slice::<HistoryIdentity<'_>>(line) else {
        return writer.write_all(line);
    };
    let Some(group) = parsed.group_id else {
        return writer.write_all(line);
    };
    if serde_json::from_str::<String>(group.get()).ok().as_deref() != Some(identity.old_group) {
        return writer.write_all(line);
    }
    let mut edits = Vec::with_capacity(2);
    if identity.old_group != identity.new_group {
        edits.push((group, serde_json::to_vec(identity.new_group)?));
    }
    if !identity.old_scope.is_empty() && identity.old_scope != identity.new_scope {
        if let Some(scope) = parsed.scope_key {
            if serde_json::from_str::<String>(scope.get()).ok().as_deref()
                == Some(identity.old_scope)
            {
                edits.push((scope, serde_json::to_vec(identity.new_scope)?));
            }
        }
    }
    if edits.is_empty() {
        return writer.write_all(line);
    }
    edits.sort_unstable_by_key(|(raw, _)| raw.get().as_ptr() as usize);
    let mut offset = 0;
    for (raw, replacement) in edits {
        let start = raw.get().as_ptr() as usize - line.as_ptr() as usize;
        writer.write_all(&line[offset..start])?;
        writer.write_all(&replacement)?;
        offset = start + raw.get().len();
    }
    writer.write_all(&line[offset..])
}

fn safe_zip_name(name: &str) -> io::Result<String> {
    if name.is_empty()
        || name.contains('\\')
        || name.starts_with('/')
        || name.bytes().any(|byte| byte < 32)
    {
        return Err(io::Error::other("invalid group copy entry path"));
    }
    // The archive uses slash-separated names. Do not let filesystem path
    // normalization bypass duplicate, reserved-name or secret exclusions.
    let components = name.strip_suffix('/').unwrap_or(name);
    if components
        .split('/')
        .any(|part| part.is_empty() || part == ".")
    {
        return Err(io::Error::other("noncanonical group copy entry path"));
    }
    #[cfg(windows)]
    if components.split('/').any(|part| {
        part.ends_with(['.', ' '])
            || part
                .chars()
                .any(|ch| matches!(ch, ':' | '<' | '>' | '"' | '|' | '?' | '*'))
    }) {
        return Err(io::Error::other("invalid Windows group copy entry path"));
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
    if name.split('/').any(is_windows_reserved) {
        return Err(io::Error::other(
            "windows reserved group copy path rejected",
        ));
    }
    Ok(name.into())
}

fn is_windows_reserved(part: &str) -> bool {
    let stem = part
        .split('.')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .is_some_and(|suffix| {
                matches!(
                    suffix,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
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
        digest.update(format!("{:x}", Sha256::digest(&files[name])).as_bytes());
        digest.update([0]);
    }
    format!("sha256:{:x}", digest.finalize())
}

fn content_digest_matches(expected: &str, files: &HashMap<String, Vec<u8>>) -> bool {
    expected == content_digest(files) || expected == legacy_rust_content_digest(files)
}

fn legacy_rust_content_digest(files: &HashMap<String, Vec<u8>>) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    fn history_identity() -> HistoryIdentityRewrite<'static> {
        HistoryIdentityRewrite {
            old_group: "g_old",
            new_group: "g_new",
            old_scope: "s_old",
            new_scope: "s_new",
        }
    }

    #[cfg(unix)]
    #[test]
    fn staged_files_respect_a_stricter_umask() {
        use std::os::unix::fs::PermissionsExt;
        const CHILD_PATH: &str = "CCCC_TEST_STAGED_UMASK_PATH";
        if let Some(path) = std::env::var_os(CHILD_PATH) {
            let file = staged_private_file(Path::new(&path)).expect("stage file");
            assert_eq!(
                file.metadata().expect("metadata").permissions().mode() & 0o777,
                0o200
            );
            drop(file);
            fs::remove_file(path).expect("remove file");
            return;
        }
        // umask is process-wide: change it only in an isolated test process.
        let temp = tempfile::tempdir().expect("tempdir");
        let status = std::process::Command::new("sh")
            .args([
                "-c",
                "umask 0400; exec \"$1\" --exact group_copy::tests::staged_files_respect_a_stricter_umask",
                "sh",
            ])
            .arg(std::env::current_exe().expect("test executable"))
            .env(CHILD_PATH, temp.path().join("ledger.jsonl"))
            .status()
            .expect("run isolated umask test");
        assert!(status.success());
    }

    #[test]
    fn history_rewrite_uses_top_level_parsed_values_and_preserves_other_bytes() {
        let input = concat!(
            "{\"data\":{\"group_id\":\"g_old\",\"scope_key\":\"s_old\"},",
            "\"scope\\u005fkey\":\"s\\u005fold\",\"group\\u005fid\":\"g\\u005fold\",\"n\":1}\r\n",
            "{\"group_id\":\"another\",\"scope_key\":\"s_old\"}\n",
            "{\"scope_key\":\"s_old\",\"data\":{\"group_id\":\"g_old\"}}\n",
            "{\"group_id\":null,\"scope_key\":\"s_old\"}\n",
            "{\"group_id\":17,\"scope_key\":\"s_old\"}\n",
            "not json\n"
        );
        let expected = concat!(
            "{\"data\":{\"group_id\":\"g_old\",\"scope_key\":\"s_old\"},",
            "\"scope\\u005fkey\":\"s_new\",\"group\\u005fid\":\"g_new\",\"n\":1}\r\n",
            "{\"group_id\":\"another\",\"scope_key\":\"s_old\"}\n",
            "{\"scope_key\":\"s_old\",\"data\":{\"group_id\":\"g_old\"}}\n",
            "{\"group_id\":null,\"scope_key\":\"s_old\"}\n",
            "{\"group_id\":17,\"scope_key\":\"s_old\"}\n",
            "not json\n"
        );
        let mut reader = BufReader::new(input.as_bytes());
        let mut result = Vec::new();
        rewrite_history_lines(&mut reader, &mut result, &history_identity(), 1024, 4096)
            .expect("rewrite");
        assert_eq!(result, expected.as_bytes());
    }

    #[test]
    fn history_rewrite_bounds_each_line_and_the_decoded_total() {
        let mut reader = BufReader::new(&b"12345\n"[..]);
        let error = rewrite_history_lines(&mut reader, &mut Vec::new(), &history_identity(), 5, 99)
            .expect_err("overlong line rejected");
        assert!(error.to_string().contains("history line"));

        let mut reader = BufReader::new(&b"one\ntwo\n"[..]);
        let error = rewrite_history_lines(&mut reader, &mut Vec::new(), &history_identity(), 8, 7)
            .expect_err("decoded budget rejected");
        assert!(error.to_string().contains("2 GiB"));

        for (damaged, expected) in [
            (
                &b"{\"group_id\":\"g_old\",\"bad\":\"\xff\"}\n"[..],
                &b"{\"group_id\":\"g_new\",\"bad\":\"\xff\"}\n"[..],
            ),
            (
                &b"{\"group_id\":\"g_old\",\"bad\":\"\\uD800\"}\n"[..],
                &b"{\"group_id\":\"g_new\",\"bad\":\"\\uD800\"}\n"[..],
            ),
            (
                &b"{\"group_id\":\"g_\xff\",\"scope_key\":\"s_old\"}\n"[..],
                &b"{\"group_id\":\"g_\xff\",\"scope_key\":\"s_old\"}\n"[..],
            ),
            (
                &b"{\"group_id\":\"g_\\uD800\",\"scope_key\":\"s_old\"}\n"[..],
                &b"{\"group_id\":\"g_\\uD800\",\"scope_key\":\"s_old\"}\n"[..],
            ),
        ] {
            let mut reader = BufReader::new(damaged);
            let mut output = Vec::new();
            rewrite_history_lines(&mut reader, &mut output, &history_identity(), 1024, 1024)
                .expect("retain unparseable history");
            assert_eq!(output, expected);
        }

        let valid = b"{\"group_id\":\"g_old\",\"emoji\":\"\\uD83D\\uDE00\"}\n";
        let mut reader = BufReader::new(&valid[..]);
        let mut output = Vec::new();
        rewrite_history_lines(&mut reader, &mut output, &history_identity(), 1024, 1024)
            .expect("valid surrogate pair");
        assert_eq!(
            output,
            b"{\"group_id\":\"g_new\",\"emoji\":\"\\uD83D\\uDE00\"}\n"
        );
    }

    #[test]
    fn history_segment_detection_follows_the_staged_path() {
        let temp = tempfile::tempdir().expect("tempdir");
        let target = temp.path();
        fs::create_dir_all(target.join("state/ledger/segments/nested")).expect("segments");
        fs::create_dir_all(target.join("STATE/LEDGER/SEGMENTS")).expect("other directory");
        let upper_is_segments = same_file::is_same_file(
            target.join("state/ledger/segments"),
            target.join("STATE/LEDGER/SEGMENTS"),
        )
        .expect("compare directories");
        assert!(is_history_segment(target, "state/ledger/segments/ledger.1.jsonl").expect("plain"));
        assert!(
            is_history_segment(target, "state/ledger/segments/ledger.1.jsonl.gz").expect("gzip")
        );
        assert_eq!(
            is_history_segment(target, "STATE/LEDGER/SEGMENTS/ledger.1.jsonl")
                .expect("case variant"),
            upper_is_segments
        );
        assert!(
            !is_history_segment(target, "state/ledger/segments/nested/ledger.1.jsonl")
                .expect("nested")
        );
        assert!(!is_history_segment(target, "state/ledger/segments/notes.txt").expect("text"));
    }

    #[test]
    fn history_rewrite_rejects_gzip_with_an_extra_member() {
        fn gzip(data: &[u8]) -> Vec<u8> {
            let mut writer = GzEncoder::new(Vec::new(), Compression::default());
            writer.write_all(data).expect("compress");
            writer.finish().expect("finish")
        }
        let mut data = gzip(b"{\"group_id\":\"g_old\"}\n");
        data.extend(gzip(b"{\"group_id\":\"g_old\"}\n"));
        let temp = tempfile::tempdir().expect("tempdir");
        let error = rewrite_segment(
            &temp.path().join("segment.gz"),
            "state/ledger/segments/test.jsonl.gz",
            &data,
            &history_identity(),
            &mut 0,
        )
        .expect_err("extra member must not become visible");
        assert!(error.to_string().contains("more than one member"));
    }

    #[test]
    fn gzip_history_counts_its_decoded_size_even_when_copied_unchanged() {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"hello").expect("gzip");
        let data = encoder.finish().expect("finish");
        let mut used = MAX_UNCOMPRESSED_BYTES - 4;
        let error = count_unchanged_gzip(&data, &mut used).expect_err("decoded budget");
        assert!(error.to_string().contains("2 GiB"));
        assert_eq!(used, MAX_UNCOMPRESSED_BYTES - 4);

        let mut used = MAX_UNCOMPRESSED_BYTES - 5;
        count_unchanged_gzip(&data, &mut used).expect("exactly within budget");
        assert_eq!(used, MAX_UNCOMPRESSED_BYTES);

        let temp = tempfile::tempdir().expect("tempdir");
        let mut used = MAX_UNCOMPRESSED_BYTES - 4;
        let error = rewrite_segment(
            &temp.path().join("rewritten.gz"),
            "state/ledger/segments/test.jsonl.gz",
            &data,
            &history_identity(),
            &mut used,
        )
        .expect_err("rewritten gzip must use the same decoded budget");
        assert!(error.to_string().contains("2 GiB"));
        assert_eq!(used, MAX_UNCOMPRESSED_BYTES - 4);
    }

    #[test]
    fn copy_rejects_case_variants_of_reserved_root_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = crate::HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let group = store.create("copy", "").expect("group");
        let (bytes, _, _) = export(&store, &group.group_id).expect("export");
        for (canonical, variant) in [
            ("ledger.jsonl", "Ledger.jsonl"),
            ("ledger.jsonl", "ledger.jſonl"),
            ("group.yaml", "Group.yaml"),
        ] {
            let mut package = read_package(&bytes).expect("package");
            let content = package.files.remove(canonical).expect("reserved file");
            package.files.insert(variant.to_owned(), content);
            package.manifest.content_digest = content_digest(&package.files);
            let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
            let options = SimpleFileOptions::default();
            writer
                .start_file("manifest.json", options)
                .expect("manifest");
            writer
                .write_all(&serde_json::to_vec(&package.manifest).expect("manifest json"))
                .expect("manifest data");
            for (path, data) in package.files {
                writer
                    .start_file(format!("group/{path}"), options)
                    .expect("entry");
                writer.write_all(&data).expect("entry data");
            }
            let modified = writer.finish().expect("archive").into_inner();
            let error = read_package(&modified)
                .err()
                .expect("noncanonical root name must be rejected");
            assert!(
                error.to_string().contains("reserved group copy entry"),
                "{variant}: {error}"
            );
        }
    }

    // Construct an otherwise valid package so these checks cannot be passed
    // accidentally just because the archive or its content digest is invalid.
    fn package_with_extra_file(
        store: &GroupStore,
        group_id: &str,
        name: &str,
        data: &[u8],
    ) -> Vec<u8> {
        let (bytes, _, _) = export(store, group_id).expect("export");
        let mut package = read_package(&bytes).expect("package");
        package.files.insert(name.to_owned(), data.to_vec());
        package.manifest.content_digest = content_digest(&package.files);
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        writer
            .start_file("manifest.json", options)
            .expect("manifest");
        writer
            .write_all(&serde_json::to_vec(&package.manifest).expect("manifest json"))
            .expect("manifest data");
        for (path, contents) in package.files {
            writer
                .start_file(format!("group/{path}"), options)
                .expect("entry");
            writer.write_all(&contents).expect("entry data");
        }
        writer.finish().expect("archive").into_inner()
    }

    #[test]
    fn copy_rejects_zip_entries_that_exceed_their_declared_size() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = crate::HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let group = store.create("copy", "").expect("group");
        let mut bytes =
            package_with_extra_file(&store, &group.group_id, "payload.txt", &[b'x'; 4096]);
        let (local, central) = {
            let mut archive = ZipArchive::new(Cursor::new(&bytes)).expect("zip");
            let entry = archive.by_name("group/payload.txt").expect("payload");
            (
                entry.header_start() as usize,
                entry.central_header_start() as usize,
            )
        };
        // Deflate still yields all 4096 bytes, but both ZIP headers say one.
        bytes[local + 22..local + 26].copy_from_slice(&1_u32.to_le_bytes());
        bytes[central + 24..central + 28].copy_from_slice(&1_u32.to_le_bytes());
        assert!(preview(&store, &bytes).is_err());
        assert!(import(&store, &bytes, "", "").is_err());
        assert_eq!(store.list().expect("registry").len(), 1);
    }

    #[test]
    fn copy_rejects_noncanonical_entry_paths_before_publication() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = crate::HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let group = store.create("copy", "").expect("group");
        for name in [
            "./ledger.jsonl",
            "state/./im_weixin_context_tokens.json",
            "state//im_weixin_context_tokens.json",
        ] {
            let bytes = package_with_extra_file(&store, &group.group_id, name, b"synthetic");
            assert!(preview(&store, &bytes).is_err(), "preview accepted {name}");
            assert!(
                import(&store, &bytes, "", "").is_err(),
                "import accepted {name}"
            );
            assert_eq!(store.list().expect("registry").len(), 1);
        }
    }

    #[cfg(windows)]
    #[test]
    fn copy_rejects_windows_filename_aliases_before_publication() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = crate::HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let group = store.create("copy", "").expect("group");
        for name in ["ledger.jsonl.", "group.yaml ", "state/notes.txt:stream"] {
            let bytes = package_with_extra_file(&store, &group.group_id, name, b"synthetic");
            assert!(preview(&store, &bytes).is_err(), "preview accepted {name}");
            assert!(
                import(&store, &bytes, "", "").is_err(),
                "import accepted {name}"
            );
            assert_eq!(store.list().expect("registry").len(), 1);
        }
        assert!(safe_zip_name("group/ ledger.jsonl").is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn import_never_replaces_ledger_through_an_existing_short_name() {
        let temp = tempfile::tempdir().expect("tempdir");
        let probe = temp.path().join("probe");
        fs::create_dir(&probe).expect("probe directory");
        fs::write(probe.join("ledger.jsonl"), b"probe").expect("probe ledger");
        if !same_file::is_same_file(probe.join("ledger.jsonl"), probe.join("LEDGER~1.JSO"))
            .unwrap_or(false)
        {
            // Not every Windows volume creates 8.3 names.
            return;
        }

        let home = crate::HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("copy", "").expect("group");
        let ledger = store
            .group_dir(&group.group_id)
            .expect("source")
            .join("ledger.jsonl");
        let original = fs::read(&ledger).expect("source ledger");
        let bytes = package_with_extra_file(&store, &group.group_id, "LEDGER~1.JSO", b"synthetic");
        preview(&store, &bytes).expect("archive uses a syntactically valid name");
        let before = fs::read_dir(home.groups_dir()).expect("groups").count();
        assert!(import(&store, &bytes, "", "").is_err());
        assert_eq!(
            fs::read_dir(home.groups_dir()).expect("groups").count(),
            before
        );
        assert_eq!(store.list().expect("registry").len(), 1);
        assert_eq!(fs::read(ledger).expect("source ledger"), original);
    }

    #[test]
    fn copy_rejects_windows_device_names_including_superscript_digits() {
        for name in ["COM1", "com¹.txt", "Lpt²", "lpt³.log"] {
            assert!(safe_zip_name(&format!("group/{name}")).is_err(), "{name}");
        }
        for name in ["COM0", "COM10", "LPT⁴"] {
            assert!(safe_zip_name(&format!("group/{name}")).is_ok(), "{name}");
        }
    }

    #[test]
    fn copy_excludes_unicode_case_aliases_of_private_names() {
        for name in [
            "state/env_private.jſon",
            "state/im_weixin_context_tokenſ.json",
            "state/paẞword/note.txt",
            "state/KIRO/session.txt",
        ] {
            assert!(excluded(name, false), "private alias {name}");
        }
        assert!(!excluded("state/memory/日本語.md", false));
    }

    #[test]
    fn copy_preserves_distinct_unicode_names_when_the_filesystem_does() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = crate::HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let group = store.create("copy", "").expect("group");
        let memory = store
            .group_dir(&group.group_id)
            .expect("source")
            .join("state/memory");
        fs::create_dir_all(&memory).expect("memory");
        let mut first = staged_private_file(&memory.join("straße.txt")).expect("first");
        first.write_all(b"first").expect("first contents");
        drop(first);
        let mut second = match staged_private_file(&memory.join("strasse.txt")) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return,
            Err(error) => panic!("create second file: {error}"),
        };
        second.write_all(b"second").expect("second contents");
        drop(second);
        let (bytes, _, _) = export(&store, &group.group_id).expect("export");
        let imported = import(&store, &bytes, "", "").expect("import");
        let memory = store
            .group_dir(&imported.group_id)
            .expect("copy")
            .join("state/memory");
        assert_eq!(
            fs::read(memory.join("straße.txt")).expect("first"),
            b"first"
        );
        assert_eq!(
            fs::read(memory.join("strasse.txt")).expect("second"),
            b"second"
        );
    }

    #[test]
    fn history_rewrites_only_the_actual_segment_directory() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = crate::HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let group = store.create("copy", "").expect("group");
        let source = store.group_dir(&group.group_id).expect("source");
        let upper = source.join("STATE/LEDGER/SEGMENTS");
        let lower = source.join("state/ledger/segments");
        fs::create_dir_all(&upper).expect("directory");
        let is_actual_segment = same_file::is_same_file(&upper, &lower).unwrap_or(false);
        let original = format!("{{\"group_id\":\"{}\"}}\n", group.group_id);
        fs::write(upper.join("notes.jsonl"), &original).expect("fixture");
        let (bytes, _, _) = export(&store, &group.group_id).expect("export");
        let imported = import(&store, &bytes, "", "").expect("import");
        let copied = fs::read_to_string(
            store
                .group_dir(&imported.group_id)
                .expect("copy")
                .join("STATE/LEDGER/SEGMENTS/notes.jsonl"),
        )
        .expect("copied file");
        if is_actual_segment {
            assert_eq!(
                copied,
                format!("{{\"group_id\":\"{}\"}}\n", imported.group_id)
            );
        } else {
            assert_eq!(copied, original);
        }
    }

    #[test]
    fn weixin_reply_credentials_never_cross_group_copy_boundary() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = crate::HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let group = store.create("copy", "").expect("group");
        let state = store.state_dir(&group.group_id).expect("state");
        fs::create_dir_all(&state).expect("state directory");
        let credentials = b"{\"fixture-user\":\"synthetic-reply-credential\"}";
        let relative = "state/im_weixin_context_tokens.json";
        fs::write(state.join("im_weixin_context_tokens.json"), credentials).expect("credentials");
        fs::write(state.join("notes.txt"), "ordinary content").expect("content");
        let (bytes, manifest, _) = export(&store, &group.group_id).expect("export");
        assert!(!manifest.contains_secrets);
        let mut package = read_package(&bytes).expect("package");
        assert!(
            !package.files.contains_key(relative),
            "export must exclude reply credentials"
        );
        assert!(package.files.contains_key("state/notes.txt"));

        // Older exporters may have packaged credentials despite the manifest.
        // Import must apply its own exclusion after validating the archive digest.
        package.files.insert(relative.into(), credentials.to_vec());
        package.files.insert(
            "state/im_weixin_context_tokenſ.json".into(),
            credentials.to_vec(),
        );
        package.manifest.content_digest = content_digest(&package.files);
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default();
        writer
            .start_file("manifest.json", options)
            .expect("manifest entry");
        writer
            .write_all(&serde_json::to_vec(&package.manifest).expect("manifest"))
            .expect("write manifest");
        for (path, data) in package.files {
            writer
                .start_file(format!("group/{path}"), options)
                .expect("entry");
            writer.write_all(&data).expect("write entry");
        }
        let bytes = writer.finish().expect("archive").into_inner();
        let imported = import(&store, &bytes, "", "").expect("import");
        let directory = store
            .group_dir(&imported.group_id)
            .expect("imported directory");
        assert!(
            !directory.join(relative).exists(),
            "import must discard reply credentials"
        );
        assert!(
            !directory
                .join("state/im_weixin_context_tokenſ.json")
                .exists(),
            "import must discard case-folded credential aliases"
        );
        assert_eq!(
            fs::read_to_string(directory.join("state/notes.txt")).expect("content"),
            "ordinary content"
        );
        assert_eq!(
            fs::read(state.join("im_weixin_context_tokens.json")).expect("source credentials"),
            credentials
        );
    }

    #[test]
    fn export_collection_rejects_oversized_files_before_reading_them() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("oversized.bin");
        let file = File::create(&path).expect("create sparse file");
        file.set_len(MAX_UNCOMPRESSED_BYTES + 1)
            .expect("size sparse file");

        let error = collect_files(directory.path()).expect_err("oversized export must fail");
        assert!(error.to_string().contains("2 GiB"));
    }

    #[test]
    fn import_allows_independent_web_model_actors() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = crate::HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let source = store.create("source", "").expect("source group");
        let set_web_model = |present: bool| {
            store
                .mutate(&source.group_id, |doc| {
                    doc.actors.clear();
                    if present {
                        let mut actor = cccc_contracts::Actor::new("web");
                        actor.runtime = cccc_contracts::ActorRuntime::WebModel;
                        doc.actors.push(actor);
                    }
                    Ok(())
                })
                .expect("mutate source group");
        };
        set_web_model(true);
        let (bytes, _, _) = export(&store, &source.group_id).expect("export");

        let imported = import(&store, &bytes, "", "").expect("independent Actor import");
        assert_eq!(store.list().expect("registry").len(), 2);
        assert_eq!(
            store.load(&imported.group_id).expect("imported").actors[0].runtime,
            cccc_contracts::ActorRuntime::WebModel
        );
        assert!(
            crate::web_model_connectors::load(store.home())
                .expect("connectors")
                .is_empty(),
            "import does not copy conversation authority"
        );
    }

    #[test]
    fn dots_copy_keeps_runtime_but_requires_fresh_authorization() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = crate::HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let mut source = store.create("Dots source", "").expect("group");
        let mut actor = cccc_contracts::Actor::new("dots");
        actor.runtime = cccc_contracts::ActorRuntime::ChatgptDots;
        crate::actors::add(&mut source, actor).expect("actor");
        store.save(&source).expect("save");
        let configured = crate::web_model_connectors::configure(&home).expect("connector");
        let id = configured["connector"]["connector_id"]
            .as_str()
            .expect("id");
        let binding = crate::web_model_connectors::bind_dots(
            &home,
            id,
            &source.group_id,
            "dots",
            &source.actors[0].generation,
        )
        .expect("bind");
        let (bytes, _, _) = export(&store, &source.group_id).expect("export");
        let inspected = preview(&store, &bytes).expect("preview");
        assert!(inspected.requires_reconnect.chatgpt_web_model);
        assert!(!inspected.requires_reconnect.grok_web_model);
        assert!(!inspected.requires_reconnect.native_sessions);
        assert!(!inspected.actors[0].native_session);
        assert_eq!(inspected.actors[0].enabled, source.actors[0].enabled);
        let imported = import(&store, &bytes, "", "").expect("import");
        let imported_group = store.load(&imported.group_id).expect("imported");
        assert!(!imported_group.running);
        assert_eq!(imported_group.state, GroupState::Idle);
        assert_ne!(imported_group.generation, source.generation);
        let actor = &imported_group.actors[0];
        assert_eq!(actor.runtime, cccc_contracts::ActorRuntime::ChatgptDots);
        assert_eq!(actor.enabled, source.actors[0].enabled);
        let connector = crate::web_model_connectors::load(&home)
            .expect("connector")
            .remove(0);
        assert!(
            crate::web_model_connectors::binding_for_actor(
                &connector,
                &imported.group_id,
                "dots",
                &actor.generation
            )
            .is_none()
        );
        assert_eq!(
            crate::web_model_connectors::binding_for_actor(
                &connector,
                &source.group_id,
                "dots",
                &source.actors[0].generation
            ),
            Some(binding)
        );
    }

    #[test]
    fn import_pauses_native_access_but_keeps_bindings_configuration_and_history() {
        use cccc_contracts::{Actor, ActorRuntime};
        use serde_json::json;

        let temp = tempfile::tempdir().expect("tempdir");
        let home = crate::HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let source = store.create("Native source", "keep topic").expect("source");
        store
            .mutate(&source.group_id, |group| {
                group.active_scope_key = "s_native".into();
                group.scopes.push(Scope {
                    scope_key: "s_native".into(),
                    url: temp.path().to_string_lossy().into_owned(),
                    label: "native workspace".into(),
                    git_remote: String::new(),
                });
                for (id, runtime, source_id, enabled) in [
                    ("native-codex", ActorRuntime::Codex, "codex:copy", true),
                    ("native-claude", ActorRuntime::Claude, "claude:copy", true),
                    (
                        "paused-native",
                        ActorRuntime::Codex,
                        "codex:paused-copy",
                        false,
                    ),
                    ("managed-codex", ActorRuntime::Codex, "", true),
                    ("paused-managed", ActorRuntime::Claude, "", false),
                ] {
                    let mut actor = Actor::new(id);
                    actor.runtime = runtime;
                    actor.external_session_id = source_id.into();
                    actor.enabled = enabled;
                    actor.title = format!("{id} title");
                    actor.default_scope_key = group.active_scope_key.clone();
                    actor.capability_hidden = vec!["fixture-capability".into()];
                    crate::actors::add(group, actor)?;
                }
                group.running = true;
                group.extra.insert(
                    "local_session_conversation".into(),
                    json!({"direct":true,"source_ids":["claude:copy","codex:copy"]}),
                );
                Ok(())
            })
            .expect("source configuration");
        let source = store.load(&source.group_id).expect("configured source");
        let source_directory = store.group_dir(&source.group_id).expect("source directory");
        let mut history = Event::new("chat.message", &source.group_id);
        history.by = "native-codex".into();
        history.scope_key.clone_from(&source.active_scope_key);
        history
            .data
            .insert("text".into(), json!("preserve native history"));
        crate::ledger::append(&source_directory.join("ledger.jsonl"), &history).expect("history");
        let source_yaml = fs::read(source_directory.join("group.yaml")).expect("source yaml");
        let source_ledger = fs::read(source_directory.join("ledger.jsonl")).expect("source ledger");

        let (bytes, _, _) = export(&store, &source.group_id).expect("export");
        let inspected = preview(&store, &bytes).expect("preview");
        assert!(inspected.group_id_conflict);
        assert!(inspected.runtime_reset.actors_stopped);
        assert_eq!(store.list().expect("groups after preview").len(), 1);
        let imported = import(&store, &bytes, "", "").expect("same home import");
        let imported_group = store.load(&imported.group_id).expect("imported group");
        let mut expected_actors = source.actors.clone();
        for actor in &mut expected_actors {
            if !actor.external_session_id.is_empty() {
                actor.enabled = false;
            }
        }
        assert_eq!(imported_group.actors, expected_actors);
        assert!(!imported_group.running);
        assert_eq!(imported_group.state, GroupState::Idle);
        assert!(
            !imported_group
                .extra
                .contains_key("local_session_conversation"),
            "import must not inherit the source Group's direct reuse index"
        );
        let events = crate::ledger::tail(
            &store
                .ledger_path(&imported.group_id)
                .expect("imported ledger"),
            2,
        )
        .expect("imported history");
        let mut expected_history = history;
        expected_history.group_id.clone_from(&imported.group_id);
        assert_eq!(events[0], expected_history);
        assert_eq!(events[1].kind, "group.create");

        let inspected = serde_json::to_value(inspected).expect("serialized preview");
        assert_eq!(inspected["requires_reconnect"]["native_sessions"], true);
        assert_eq!(inspected["requires_reconnect"]["chatgpt_web_model"], false);
        assert_eq!(inspected["requires_reconnect"]["grok_web_model"], false);
        for (actor, expected) in inspected["actors"]
            .as_array()
            .expect("preview actors")
            .iter()
            .zip(&expected_actors)
        {
            assert_eq!(actor["id"], expected.id);
            assert_eq!(
                actor["native_session"],
                !expected.external_session_id.is_empty()
            );
            assert_eq!(actor["enabled"], expected.enabled);
        }
        assert_eq!(
            fs::read(source_directory.join("group.yaml")).expect("unchanged source yaml"),
            source_yaml
        );
        assert_eq!(
            fs::read(source_directory.join("ledger.jsonl")).expect("unchanged source ledger"),
            source_ledger
        );
    }

    #[test]
    fn import_preserves_legacy_profile_without_runtime() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = crate::HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        crate::fs::write_json(
            &home.root().join("profiles.json"),
            &serde_json::json!({"profiles":{"legacy":{"id":"legacy","env":{}}}}),
        )
        .expect("legacy profile");
        let source = store.create("source", "").expect("group");
        store
            .mutate(&source.group_id, |doc| {
                let mut actor = cccc_contracts::Actor::new("linked");
                actor.profile_id = "legacy".into();
                doc.actors.push(actor);
                Ok(())
            })
            .expect("actor");
        let (bytes, _, _) = export(&store, &source.group_id).expect("export");
        let imported = import(&store, &bytes, "", "").expect("import legacy Profile");
        let imported = store.load(&imported.group_id).expect("imported group");
        assert_eq!(imported.actors[0].profile_id, "legacy");
        assert_eq!(
            imported.actors[0].runtime,
            cccc_contracts::ActorRuntime::Codex
        );
    }

    #[test]
    fn import_checks_pending_linked_profile_runtime() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = crate::HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home.clone()).expect("store");
        let profiles = crate::profiles::ProfileStore::new(home).expect("profiles");
        let upsert = |runtime: &str| {
            profiles
                .upsert(
                    serde_json::json!({"id":"shared", "runtime":runtime})
                        .as_object()
                        .expect("profile object")
                        .clone(),
                    None,
                )
                .expect("profile")
        };
        upsert("codex");
        let source = store.create("source", "").expect("group");
        store
            .mutate(&source.group_id, |doc| {
                let mut actor = cccc_contracts::Actor::new("linked");
                actor.profile_id = "shared".into();
                doc.actors.push(actor);
                Ok(())
            })
            .expect("actor");
        let (bytes, _, _) = export(&store, &source.group_id).expect("export");
        upsert("web_model");
        import(&store, &bytes, "", "")
            .expect("linked Profile does not impose an instance singleton");
        assert_eq!(store.list().expect("registry").len(), 2);
    }

    #[test]
    fn content_digest_matches_the_python_package_contract() {
        let files = HashMap::from([
            ("a.txt".into(), b"hello".to_vec()),
            ("b.bin".into(), vec![0, 255]),
        ]);
        assert_eq!(
            content_digest(&files),
            "sha256:9bce08b1462fc2e8bdff4c2b8e0fd1fcbc0977775d40a3d2820df78e3e2ef243"
        );
        assert!(content_digest_matches(&content_digest(&files), &files));
        assert!(content_digest_matches(
            &legacy_rust_content_digest(&files),
            &files
        ));
    }
}
