use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_manifest = manifest_dir.join("../..").join("Cargo.toml");
    println!("cargo:rerun-if-changed={}", workspace_manifest.display());

    let product_version = fs::read_to_string(&workspace_manifest)
        .ok()
        .and_then(|manifest| workspace_version(&manifest))
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_owned());
    println!("cargo:rustc-env=CCCC_PRODUCT_VERSION={product_version}");
}

fn workspace_version(manifest: &str) -> Option<String> {
    let mut in_workspace_package = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_workspace_package = line == "[workspace.package]";
            continue;
        }
        if !in_workspace_package {
            continue;
        }
        let Some(value) = line.strip_prefix("version") else {
            continue;
        };
        let Some(value) = value.trim_start().strip_prefix('=') else {
            continue;
        };
        let value = value.trim();
        return value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .map(str::to_owned);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::workspace_version;

    #[test]
    fn reads_only_the_workspace_package_version() {
        let manifest = r#"
[package]
version = "0.0.3"

[workspace.package]
version = "0.4.33"
edition = "2024"
"#;
        assert_eq!(workspace_version(manifest).as_deref(), Some("0.4.33"));
    }
}
