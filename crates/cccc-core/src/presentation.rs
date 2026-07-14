use cccc_contracts::utc_now;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::fs::{read_json, write_json};
use crate::{GroupDoc, GroupStore, blobs};

const SLOT_IDS: [&str; 4] = ["slot-1", "slot-2", "slot-3", "slot-4"];
const CARD_TYPES: [&str; 6] = ["markdown", "table", "image", "pdf", "file", "web_preview"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TableData {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Content {
    pub mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub markdown: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<TableData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob_rel_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_rel_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
}

impl Content {
    fn new(mode: &str) -> Self {
        Self {
            mode: mode.into(),
            markdown: None,
            table: None,
            url: None,
            blob_rel_path: None,
            workspace_rel_path: None,
            mime_type: None,
            file_name: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Card {
    pub slot_id: String,
    pub title: String,
    pub card_type: String,
    pub published_by: String,
    pub published_at: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_label: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_ref: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
    pub content: Content,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Slot {
    pub slot_id: String,
    pub index: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<Card>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Snapshot {
    pub v: u8,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub highlight_slot_id: String,
    pub slots: Vec<Slot>,
}

#[derive(Debug, Clone, Default)]
pub struct Publish {
    pub slot: String,
    pub card_type: String,
    pub title: String,
    pub summary: String,
    pub content: String,
    pub table: Option<Value>,
    pub path: String,
    pub url: String,
    pub blob_rel_path: String,
    pub by: String,
    pub source_label: String,
    pub source_ref: String,
}

pub fn load(store: &GroupStore, group_id: &str) -> io::Result<Snapshot> {
    store.load(group_id)?;
    let path = path(store, group_id)?;
    if !path.exists() {
        return Ok(empty());
    }
    let parsed: Snapshot = read_json(&path)?;
    normalize(parsed)
}

pub fn publish(
    store: &GroupStore,
    group_id: &str,
    request: Publish,
) -> io::Result<(String, Card, Snapshot, bool)> {
    let group = store.load(group_id)?;
    validate_publisher(&group, &request.by)?;
    let mut snapshot = load(store, group_id)?;
    let slot_id = choose_slot(&snapshot, &request.slot)?;
    let card_type = infer_card_type(&request)?;
    let card = build_card(store, &group, &slot_id, &card_type, request)?;
    let slot = snapshot
        .slots
        .iter_mut()
        .find(|slot| slot.slot_id == slot_id)
        .ok_or_else(|| io::Error::other("presentation slot not found"))?;
    let replaced = slot.card.replace(card.clone()).is_some();
    snapshot.highlight_slot_id.clone_from(&slot_id);
    snapshot.updated_at = utc_now();
    save(store, group_id, &snapshot)?;
    Ok((slot_id, card, snapshot, replaced))
}

pub fn clear(
    store: &GroupStore,
    group_id: &str,
    requested: &str,
) -> io::Result<(Vec<String>, Snapshot)> {
    let mut snapshot = load(store, group_id)?;
    let requested = normalize_slot(requested, false)?;
    let clear_all = requested.is_empty();
    let mut cleared = Vec::new();
    for slot in &mut snapshot.slots {
        if (clear_all || slot.slot_id == requested) && slot.card.take().is_some() {
            cleared.push(slot.slot_id.clone());
        }
    }
    if clear_all || cleared.contains(&snapshot.highlight_slot_id) {
        snapshot.highlight_slot_id.clear();
    }
    snapshot.updated_at = utc_now();
    save(store, group_id, &snapshot)?;
    Ok((cleared, snapshot))
}

pub fn workspace_root(group: &GroupDoc) -> io::Result<PathBuf> {
    let scope = group
        .scopes
        .iter()
        .find(|scope| scope.scope_key == group.active_scope_key)
        .ok_or_else(|| io::Error::other("group has no active scope"))?;
    Path::new(&scope.url).canonicalize()
}

pub fn resolve_workspace_path(group: &GroupDoc, relative: &str) -> io::Result<PathBuf> {
    let root = workspace_root(group)?;
    let rel = safe_relative(relative)?;
    let candidate = root.join(rel).canonicalize()?;
    if !candidate.starts_with(&root) || !candidate.is_file() {
        return Err(io::Error::other(
            "path must be a file under the active scope",
        ));
    }
    Ok(candidate)
}

pub fn asset_path(
    store: &GroupStore,
    group_id: &str,
    slot_id: &str,
) -> io::Result<(PathBuf, String, String)> {
    let group = store.load(group_id)?;
    let slot_id = normalize_slot(slot_id, false)?;
    if slot_id.is_empty() {
        return Err(io::Error::other("slot is required"));
    }
    let card = load(store, group_id)?
        .slots
        .into_iter()
        .find(|slot| slot.slot_id == slot_id)
        .and_then(|slot| slot.card)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "presentation slot is empty"))?;
    let path = if let Some(relative) = &card.content.workspace_rel_path {
        resolve_workspace_path(&group, relative)?
    } else if let Some(relative) = &card.content.blob_rel_path {
        blobs::resolve(store.home(), group_id, relative)?
    } else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "presentation card has no local asset",
        ));
    };
    let mime = card.content.mime_type.unwrap_or_else(|| {
        mime_guess::from_path(&path)
            .first_or_octet_stream()
            .to_string()
    });
    let file_name = card.content.file_name.unwrap_or_else(|| {
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("asset")
            .into()
    });
    Ok((path, mime, file_name))
}

fn empty() -> Snapshot {
    Snapshot {
        v: 1,
        updated_at: String::new(),
        highlight_slot_id: String::new(),
        slots: SLOT_IDS
            .iter()
            .enumerate()
            .map(|(index, id)| Slot {
                slot_id: (*id).into(),
                index: index + 1,
                card: None,
            })
            .collect(),
    }
}

fn normalize(snapshot: Snapshot) -> io::Result<Snapshot> {
    let mut cards = snapshot
        .slots
        .into_iter()
        .filter(|slot| SLOT_IDS.contains(&slot.slot_id.as_str()))
        .map(|slot| (slot.slot_id, slot.card))
        .collect::<BTreeMap<_, _>>();
    let mut normalized = empty();
    normalized.updated_at = snapshot.updated_at;
    normalized.highlight_slot_id = if SLOT_IDS.contains(&snapshot.highlight_slot_id.as_str()) {
        snapshot.highlight_slot_id
    } else {
        String::new()
    };
    for slot in &mut normalized.slots {
        slot.card = cards.remove(&slot.slot_id).flatten();
    }
    Ok(normalized)
}

fn validate_publisher(group: &GroupDoc, by: &str) -> io::Result<()> {
    let by = by.trim();
    if by.is_empty()
        || matches!(by, "user" | "system")
        || group.actors.iter().any(|actor| actor.id == by)
    {
        Ok(())
    } else {
        Err(io::Error::other(format!("unknown actor: {by}")))
    }
}

fn choose_slot(snapshot: &Snapshot, requested: &str) -> io::Result<String> {
    let requested = normalize_slot(requested, true)?;
    if requested != "auto" {
        return Ok(requested);
    }
    if let Some(slot) = snapshot.slots.iter().find(|slot| slot.card.is_none()) {
        return Ok(slot.slot_id.clone());
    }
    snapshot
        .slots
        .iter()
        .min_by_key(|slot| {
            slot.card
                .as_ref()
                .map(|card| card.published_at.as_str())
                .unwrap_or("~")
        })
        .map(|slot| slot.slot_id.clone())
        .ok_or_else(|| io::Error::other("presentation has no slots"))
}

fn normalize_slot(value: &str, allow_auto: bool) -> io::Result<String> {
    let mut normalized = value.trim().to_ascii_lowercase().replace('_', "-");
    if normalized.is_empty() {
        return Ok(if allow_auto {
            "auto".into()
        } else {
            String::new()
        });
    }
    if allow_auto && normalized == "auto" {
        return Ok(normalized);
    }
    if normalized.bytes().all(|byte| byte.is_ascii_digit()) {
        normalized = format!("slot-{normalized}");
    }
    if SLOT_IDS.contains(&normalized.as_str()) {
        Ok(normalized)
    } else {
        Err(io::Error::other(if allow_auto {
            "slot must be auto or slot-1 through slot-4"
        } else {
            "slot must be slot-1 through slot-4"
        }))
    }
}

fn infer_card_type(request: &Publish) -> io::Result<String> {
    let explicit = request.card_type.trim().to_ascii_lowercase();
    if !explicit.is_empty() {
        return CARD_TYPES
            .contains(&explicit.as_str())
            .then_some(explicit)
            .ok_or_else(|| io::Error::other("unsupported card_type"));
    }
    if request.table.is_some() {
        return Ok("table".into());
    }
    let hint = if request.path.is_empty() {
        &request.url
    } else {
        &request.path
    };
    let suffix = extension_hint(hint);
    let inferred = match suffix.as_str() {
        "md" | "markdown" => "markdown",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "avif" => "image",
        "pdf" => "pdf",
        "html" | "htm" => "web_preview",
        _ if !request.content.is_empty() => "markdown",
        _ if !hint.is_empty() || !request.blob_rel_path.is_empty() => "file",
        _ => return Err(io::Error::other("unable to infer card_type")),
    };
    Ok(inferred.into())
}

fn build_card(
    store: &GroupStore,
    group: &GroupDoc,
    slot_id: &str,
    card_type: &str,
    request: Publish,
) -> io::Result<Card> {
    let title = derive_title(&request, card_type);
    let mut source_label = request.source_label.trim().to_owned();
    let mut source_ref = request.source_ref.trim().to_owned();
    let mut content = Content::new("inline");

    match card_type {
        "markdown" if !request.path.trim().is_empty() => {
            let (path, relative) = resolve_input_path(group, &request.path)?;
            source_label = defaulted(source_label, file_name(&path));
            source_ref = defaulted(source_ref, relative.clone());
            content.mode = "workspace_link".into();
            content.workspace_rel_path = Some(relative);
            content.mime_type = Some(
                mime_guess::from_path(&path)
                    .first_or_text_plain()
                    .to_string(),
            );
            content.file_name = Some(file_name(&path));
        }
        "markdown" => {
            if request.content.is_empty() {
                return Err(io::Error::other("markdown card requires content or path"));
            }
            content.markdown = Some(request.content);
        }
        "table" => content.table = Some(normalize_table(request.table.as_ref())?),
        "web_preview" if !request.path.trim().is_empty() => {
            let (path, relative) = resolve_input_path(group, &request.path)?;
            source_label = defaulted(source_label, file_name(&path));
            source_ref = defaulted(source_ref, relative.clone());
            content.mode = "workspace_link".into();
            content.workspace_rel_path = Some(relative);
            content.mime_type = Some(
                mime_guess::from_path(&path)
                    .first_or_octet_stream()
                    .to_string(),
            );
            content.file_name = Some(file_name(&path));
        }
        "web_preview"
            if !request.content.is_empty()
                && request.url.is_empty()
                && request.blob_rel_path.is_empty() =>
        {
            let blob = blobs::store(store.home(), &group.group_id, request.content.as_bytes())?;
            let name = if title.ends_with(".html") {
                title.clone()
            } else {
                format!("{title}.html")
            };
            source_label = defaulted(source_label, name.clone());
            source_ref = defaulted(source_ref, "inline-html".into());
            content.mode = "reference".into();
            content.blob_rel_path = Some(blob.path);
            content.mime_type = Some("text/html".into());
            content.file_name = Some(name);
        }
        "web_preview" => fill_reference(
            store,
            group,
            &request,
            &title,
            &mut source_label,
            &mut source_ref,
            &mut content,
        )?,
        "image" | "pdf" | "file" if !request.path.trim().is_empty() => {
            let (path, relative) = resolve_input_path(group, &request.path)?;
            source_label = defaulted(source_label, file_name(&path));
            source_ref = defaulted(source_ref, relative.clone());
            content.mode = "workspace_link".into();
            content.workspace_rel_path = Some(relative);
            content.mime_type = Some(
                mime_guess::from_path(&path)
                    .first_or_octet_stream()
                    .to_string(),
            );
            content.file_name = Some(file_name(&path));
        }
        "image" | "pdf" | "file" => fill_reference(
            store,
            group,
            &request,
            &title,
            &mut source_label,
            &mut source_ref,
            &mut content,
        )?,
        _ => return Err(io::Error::other("unsupported card_type")),
    }

    Ok(Card {
        slot_id: slot_id.into(),
        title,
        card_type: card_type.into(),
        published_by: defaulted(request.by.trim().to_owned(), "user".into()),
        published_at: utc_now(),
        source_label,
        source_ref,
        summary: request.summary.trim().into(),
        content,
    })
}

fn fill_reference(
    store: &GroupStore,
    group: &GroupDoc,
    request: &Publish,
    title: &str,
    source_label: &mut String,
    source_ref: &mut String,
    content: &mut Content,
) -> io::Result<()> {
    content.mode = "reference".into();
    if !request.blob_rel_path.trim().is_empty() {
        let relative = normalize_blob_path(&request.blob_rel_path);
        let path = blobs::resolve(store.home(), &group.group_id, &relative)?;
        *source_label = defaulted(std::mem::take(source_label), file_name(&path));
        *source_ref = defaulted(std::mem::take(source_ref), relative.clone());
        content.mime_type = Some(
            mime_guess::from_path(if request.title.is_empty() {
                &path
            } else {
                Path::new(&request.title)
            })
            .first_or_octet_stream()
            .to_string(),
        );
        content.file_name = Some(if request.title.is_empty() {
            file_name(&path)
        } else {
            request.title.clone()
        });
        content.blob_rel_path = Some(relative);
    } else if !request.url.trim().is_empty() {
        let url = request.url.trim().to_owned();
        *source_ref = defaulted(std::mem::take(source_ref), url.clone());
        content.mime_type = mime_guess::from_path(&url)
            .first()
            .map(|mime| mime.to_string());
        content.file_name = Some(title.into());
        content.url = Some(url);
    } else {
        return Err(io::Error::other(
            "card requires path, url, or blob_rel_path",
        ));
    }
    Ok(())
}

fn normalize_table(value: Option<&Value>) -> io::Result<TableData> {
    let value = value.ok_or_else(|| io::Error::other("table is required"))?;
    let rows_value = value.get("rows").unwrap_or(value);
    let rows = rows_value
        .as_array()
        .ok_or_else(|| io::Error::other("table rows must be an array"))?;
    let mut columns = value
        .get("columns")
        .and_then(Value::as_array)
        .map(|values| values.iter().map(value_text).collect::<Vec<_>>())
        .unwrap_or_default();
    if columns.is_empty() {
        for row in rows {
            if let Some(object) = row.as_object() {
                for key in object.keys() {
                    if !columns.contains(key) {
                        columns.push(key.clone());
                    }
                }
            }
        }
    }
    let normalized_rows = rows
        .iter()
        .filter_map(|row| {
            row.as_array()
                .map(|cells| cells.iter().map(value_text).collect())
                .or_else(|| {
                    row.as_object().map(|object| {
                        columns
                            .iter()
                            .map(|key| object.get(key).map(value_text).unwrap_or_default())
                            .collect()
                    })
                })
        })
        .collect();
    Ok(TableData {
        columns,
        rows: normalized_rows,
    })
}

fn derive_title(request: &Publish, card_type: &str) -> String {
    if !request.title.trim().is_empty() {
        return request.title.trim().into();
    }
    if !request.path.trim().is_empty() {
        return Path::new(request.path.trim())
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(card_type)
            .into();
    }
    if !request.url.trim().is_empty() {
        let without_query = request
            .url
            .split(['?', '#'])
            .next()
            .unwrap_or(&request.url)
            .trim_end_matches('/');
        return without_query
            .rsplit('/')
            .next()
            .filter(|part| !part.is_empty())
            .unwrap_or(card_type)
            .into();
    }
    card_type.replace('_', " ")
}

fn resolve_input_path(group: &GroupDoc, input: &str) -> io::Result<(PathBuf, String)> {
    let root = workspace_root(group)?;
    let raw = Path::new(input.trim());
    let path = if raw.is_absolute() {
        raw.canonicalize()?
    } else {
        root.join(safe_relative(input)?).canonicalize()?
    };
    let relative = path
        .strip_prefix(&root)
        .map_err(|_| io::Error::other("path must be under the active scope"))?
        .to_string_lossy()
        .replace('\\', "/");
    if !path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "workspace file not found",
        ));
    }
    Ok((path, relative))
}

fn safe_relative(value: &str) -> io::Result<PathBuf> {
    let path = Path::new(value.trim());
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        Err(io::Error::other("path must stay under the active scope"))
    } else {
        Ok(path.into())
    }
}

fn extension_hint(value: &str) -> String {
    let path = value.split(['?', '#']).next().unwrap_or(value);
    Path::new(path)
        .extension()
        .and_then(|part| part.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn normalize_blob_path(value: &str) -> String {
    let value = value.trim();
    if value.starts_with("state/blobs/") {
        value.into()
    } else {
        format!(
            "state/blobs/{}",
            Path::new(value)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(value)
        )
    }
}

fn value_text(value: &Value) -> String {
    value.as_str().map(str::to_owned).unwrap_or_else(|| {
        if value.is_null() {
            String::new()
        } else {
            value.to_string()
        }
    })
}

fn defaulted(value: String, fallback: String) -> String {
    if value.is_empty() { fallback } else { value }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("asset")
        .into()
}

fn save(store: &GroupStore, group_id: &str, snapshot: &Snapshot) -> io::Result<()> {
    write_json(&path(store, group_id)?, snapshot)
}

fn path(store: &GroupStore, group_id: &str) -> io::Result<PathBuf> {
    Ok(store.state_dir(group_id)?.join("presentation.json"))
}

pub fn list_workspace(
    group: &GroupDoc,
    relative: &str,
) -> io::Result<(PathBuf, String, Option<String>, Vec<WorkspaceItem>)> {
    let root = workspace_root(group)?;
    let relative_path = if relative.trim().is_empty() {
        PathBuf::new()
    } else {
        safe_relative(relative)?
    };
    let directory = root.join(&relative_path).canonicalize()?;
    if !directory.starts_with(&root) || !directory.is_dir() {
        return Err(io::Error::other(
            "workspace path must be a directory under the active scope",
        ));
    }
    let mut items = fs::read_dir(&directory)?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let relative = path
                .strip_prefix(&root)
                .ok()?
                .to_string_lossy()
                .replace('\\', "/");
            Some(WorkspaceItem {
                name: entry.file_name().to_string_lossy().into_owned(),
                path: relative,
                is_dir: path.is_dir(),
                mime_type: (!path.is_dir()).then(|| {
                    mime_guess::from_path(path)
                        .first_or_octet_stream()
                        .to_string()
                }),
            })
        })
        .collect::<Vec<_>>();
    items.sort_by_key(|item| (!item.is_dir, item.name.to_ascii_lowercase()));
    let normalized = relative_path.to_string_lossy().replace('\\', "/");
    let parent = relative_path
        .parent()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .filter(|path| !path.is_empty());
    Ok((root, normalized, parent, items))
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkspaceItem {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}
