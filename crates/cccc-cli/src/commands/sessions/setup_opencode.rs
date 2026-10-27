//! An automatically discovered, owned plugin; user JSON/JSONC stays native-owned.
use super::*;
use sha2::{Digest, Sha256};
use std::path::Path;

const NAME: &str = "@cccc/native-opencode";

fn config_directory() -> Result<PathBuf> {
    let result = invoke("opencode", &["debug".into(), "paths".into()])?;
    anyhow::ensure!(
        result.status.success() && !result.stdout_truncated,
        "Could not locate OpenCode's native configuration directory"
    );
    let output = String::from_utf8(result.stdout)?;
    let path = output
        .lines()
        .find_map(|line| {
            let value = line.strip_prefix("config")?;
            value
                .starts_with(char::is_whitespace)
                .then(|| PathBuf::from(value.trim()))
        })
        .context("OpenCode did not report its configuration directory")?;
    anyhow::ensure!(
        path.is_absolute(),
        "OpenCode configuration directory is not absolute"
    );
    Ok(path)
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn owned_manifest(path: &Path, home: &HomeLayout, executable: &Path) -> Result<Option<Value>> {
    if !path.try_exists()? {
        return Ok(None);
    }
    let manifest: Value = fs::read_json(&path.join("package.json"))
        .context("The OpenCode CCCC plugin is unreadable; no provider files were changed")?;
    anyhow::ensure!(
        manifest["name"] == NAME
            && manifest["cccc"]["home"] == json!(home.root())
            && manifest["cccc"]["executable"] == json!(executable),
        "The OpenCode CCCC plugin belongs to another installation; no provider files were changed"
    );
    for file in ["index.ts", "tui.ts"] {
        let bytes = std::fs::read(path.join(file))?;
        anyhow::ensure!(
            manifest["cccc"]["files"][file] == hash(&bytes),
            "The generated OpenCode CCCC plugin was modified; preserve or remove that customization before reconfiguring it"
        );
    }
    Ok(Some(manifest))
}

pub(super) fn configure(home: &HomeLayout, enabled: bool) -> Result<()> {
    platform()?;
    home.initialize()?;
    if !enabled {
        local_sessions::set_integration_disabled(home, LocalSessionRuntime::Opencode, true)?;
    }
    version("opencode")?;
    let executable = crate::commands::setup::public_executable()?;
    let config = config_directory()?;
    let plugin = config.join("plugins/cccc-native");
    let owned = owned_manifest(&plugin, home, &executable)?;
    if enabled {
        local_sessions::set_integration_disabled(home, LocalSessionRuntime::Opencode, true)?;
        std::fs::create_dir_all(&plugin)?;
        let render = |source: &str| -> Result<String> {
            Ok(source
                .replace("__CCCC_EXECUTABLE__", &serde_json::to_string(&executable)?)
                .replace("__CCCC_HOME__", &serde_json::to_string(home.root())?)
                .replace("__OPENCODE_CONFIG__", &serde_json::to_string(&config)?))
        };
        let backend = render(include_str!("../../../resources/native/opencode.ts"))?;
        let tui = render(include_str!("../../../resources/native/opencode-tui.ts"))?;
        fs::atomic_write(&plugin.join("index.ts"), backend.as_bytes())?;
        fs::atomic_write(&plugin.join("tui.ts"), tui.as_bytes())?;
        fs::write_json(
            &plugin.join("package.json"),
            &json!({
                "name":NAME,"version":env!("CARGO_PKG_VERSION"),"type":"module",
                "exports":{".":"./index.ts","./tui":"./tui.ts"},
                "cccc":{"home":home.root(),"executable":executable,
                    "files":{"index.ts":hash(backend.as_bytes()),"tui.ts":hash(tui.as_bytes())}},
            }),
        )?;
        local_sessions::set_integration_disabled(home, LocalSessionRuntime::Opencode, false)?;
    } else if owned.is_some() {
        for file in ["index.ts", "tui.ts", "package.json"] {
            std::fs::remove_file(plugin.join(file))?;
        }
        match std::fs::remove_dir(&plugin) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => {
                eprintln!(
                    "Native access is disabled; additional files in the OpenCode plugin directory were preserved."
                );
            }
            Err(error) => return Err(error.into()),
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({"runtime":"opencode","enabled":enabled,
        "note":if enabled {"Configuration saved. OpenCode watches its configuration directory; reload or reopen the original CLI session if the plugin has not loaded. CCCC uses the existing local backend and does not host its terminal. An existing cccc MCP entry must belong to this installation."} else {"Native access is disabled. OpenCode unloads the owned plugin after its configuration refresh; original sessions and user MCP configuration are retained."}}))?
    );
    Ok(())
}
