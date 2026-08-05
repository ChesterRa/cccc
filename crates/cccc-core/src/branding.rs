use cccc_contracts::utc_now;
use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::PathBuf;

use crate::HomeLayout;
use crate::fs::atomic_write;

const MAX_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StoredAsset {
    pub asset_kind: String,
    pub mime_type: String,
    pub bytes: usize,
    pub sha256: String,
    pub rel_path: String,
    pub public_url: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Payload {
    pub product_name: String,
    pub logo_icon_url: String,
    pub favicon_url: String,
    pub has_custom_logo_icon: bool,
    pub has_custom_favicon: bool,
    pub updated_at: Option<String>,
}

pub fn store(
    home: &HomeLayout,
    kind: &str,
    data: &[u8],
    content_type: &str,
    filename: &str,
) -> io::Result<StoredAsset> {
    validate_kind(kind)?;
    if data.len() > MAX_BYTES {
        return Err(io::Error::other("branding asset exceeds 2 MiB"));
    }
    let mime = normalize_mime(content_type, filename);
    if !allowed(kind, &mime) {
        return Err(io::Error::other(format!("unsupported {kind} type: {mime}")));
    }
    let sha256 = format!("{:x}", Sha256::digest(data));
    let extension = extension(&mime);
    let name = format!("{kind}_{}{extension}", &sha256[..16]);
    let relative = format!("state/web_branding/{name}");
    let path = home.root().join(&relative);
    atomic_write(&path, data)?;
    Ok(StoredAsset {
        asset_kind: kind.into(),
        mime_type: mime,
        bytes: data.len(),
        sha256: sha256.clone(),
        rel_path: relative,
        public_url: format!("/api/v1/branding/assets/{kind}?v={}", &sha256[..16]),
    })
}

pub fn resolve(home: &HomeLayout, relative: &str) -> io::Result<PathBuf> {
    let base = home.root().canonicalize()?;
    let path = base.join(relative.trim()).canonicalize()?;
    if path.starts_with(&base) && path.is_file() {
        Ok(path)
    } else {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "branding asset not found",
        ))
    }
}

pub fn delete(home: &HomeLayout, relative: &str) -> io::Result<()> {
    if relative.trim().is_empty() {
        return Ok(());
    }
    match resolve(home, relative) {
        Ok(path) => fs::remove_file(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub fn payload(raw: &Map<String, Value>) -> Payload {
    let product_name = raw
        .get("product_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("CCCC")
        .into();
    let logo = raw
        .get("logo_icon_asset_path")
        .and_then(Value::as_str)
        .unwrap_or("");
    let favicon = raw
        .get("favicon_asset_path")
        .and_then(Value::as_str)
        .unwrap_or("");
    let updated_at = raw
        .get("updated_at")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let version = updated_at.as_deref().unwrap_or("default");
    let logo_url = if logo.is_empty() {
        "/ui/logo.svg".into()
    } else {
        format!("/api/v1/branding/assets/logo_icon?v={version}")
    };
    let favicon_url = if favicon.is_empty() {
        logo_url.clone()
    } else {
        format!("/api/v1/branding/assets/favicon?v={version}")
    };
    Payload {
        product_name,
        logo_icon_url: logo_url,
        favicon_url,
        has_custom_logo_icon: !logo.is_empty(),
        has_custom_favicon: !favicon.is_empty(),
        updated_at,
    }
}

pub fn asset_relative(raw: &Map<String, Value>, kind: &str) -> io::Result<String> {
    validate_kind(kind)?;
    Ok(raw
        .get(&format!("{kind}_asset_path"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .into())
}

pub fn touch(raw: &mut Map<String, Value>) {
    raw.insert("updated_at".into(), Value::String(utc_now()));
}

fn validate_kind(kind: &str) -> io::Result<()> {
    if matches!(kind, "logo_icon" | "favicon") {
        Ok(())
    } else {
        Err(io::Error::other("asset kind must be logo_icon or favicon"))
    }
}

fn normalize_mime(content_type: &str, filename: &str) -> String {
    let explicit = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if explicit.starts_with("image/") {
        explicit
    } else {
        mime_guess::from_path(filename)
            .first_or_octet_stream()
            .to_string()
    }
}

fn allowed(kind: &str, mime: &str) -> bool {
    let common = matches!(
        mime,
        "image/svg+xml" | "image/png" | "image/x-icon" | "image/vnd.microsoft.icon"
    );
    common
        || kind == "logo_icon"
            && matches!(
                mime,
                "image/jpeg" | "image/webp" | "image/gif" | "image/avif"
            )
}

fn extension(mime: &str) -> &'static str {
    match mime {
        "image/svg+xml" => ".svg",
        "image/png" => ".png",
        "image/jpeg" => ".jpg",
        "image/webp" => ".webp",
        "image/gif" => ".gif",
        "image/avif" => ".avif",
        "image/x-icon" | "image/vnd.microsoft.icon" => ".ico",
        _ => ".bin",
    }
}
