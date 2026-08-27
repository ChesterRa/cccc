use base64::Engine;
use serde_json::{Map, Value};
use std::{fs, io};

use crate::HomeLayout;

pub fn apple_touch_icon_url(home: &HomeLayout, raw: &Map<String, Value>) -> String {
    let version = raw
        .get("updated_at")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("default");
    for (asset_kind, key) in [
        ("logo_icon", "logo_icon_asset_path"),
        ("favicon", "favicon_asset_path"),
    ] {
        let Some(relative) = raw
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let is_png = std::path::Path::new(relative)
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("png"));
        if is_png && crate::branding::resolve(home, relative).is_ok() {
            return format!("/api/v1/branding/assets/{asset_kind}?v={version}");
        }
    }
    "/ui/logo.png".into()
}

pub fn pwa_icon_svg(
    home: &HomeLayout,
    raw: &Map<String, Value>,
    maskable: bool,
) -> io::Result<Vec<u8>> {
    let relative = raw
        .get("logo_icon_asset_path")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            raw.get("favicon_asset_path")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
        })
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "custom PWA icon not found"))?;
    let path = crate::branding::resolve(home, relative)?;
    let bytes = fs::read(&path)?;
    let mime = mime_guess::from_path(&path)
        .first_or_octet_stream()
        .to_string();
    if !mime.starts_with("image/") {
        return Err(io::Error::other("custom PWA icon is not an image"));
    }
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    let (background, position, size) = if maskable {
        (
            r##"<rect width="1024" height="1024" fill="#0f172a"/>"##,
            "128",
            "768",
        )
    } else {
        ("", "0", "1024")
    };
    Ok(format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">{background}<image href="data:{mime};base64,{encoded}" x="{position}" y="{position}" width="{size}" height="{size}" preserveAspectRatio="xMidYMid meet"/></svg>"#
    )
    .into_bytes())
}

#[cfg(test)]
mod tests {
    use super::apple_touch_icon_url;
    use crate::HomeLayout;
    use crate::branding::store;
    use serde_json::{Map, Value};

    #[test]
    fn apple_icon_prefers_a_png_favicon_when_the_custom_logo_is_not_png() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("home");
        let logo =
            store(&home, "logo_icon", b"<svg/>", "image/svg+xml", "logo.svg").expect("SVG logo");
        let favicon =
            store(&home, "favicon", b"png", "image/png", "favicon.png").expect("PNG favicon");
        let mut raw = Map::new();
        raw.insert("logo_icon_asset_path".into(), Value::String(logo.rel_path));
        raw.insert("favicon_asset_path".into(), Value::String(favicon.rel_path));
        raw.insert("updated_at".into(), Value::String("v1".into()));

        assert_eq!(
            apple_touch_icon_url(&home, &raw),
            "/api/v1/branding/assets/favicon?v=v1"
        );
    }

    #[test]
    fn apple_icon_falls_back_to_the_builtin_png_without_a_custom_png() {
        assert_eq!(
            apple_touch_icon_url(
                &HomeLayout::from_path("/missing-home").expect("home"),
                &Map::new()
            ),
            "/ui/logo.png"
        );
    }
}
