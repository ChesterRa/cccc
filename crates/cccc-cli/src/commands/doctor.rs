use anyhow::Result;
use cccc_core::HomeLayout;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub fn run(home: &HomeLayout) -> Result<()> {
    let browser = find_browser();
    println!(
        "{}",
        serde_json::to_string_pretty(&report(home, browser.as_deref()))?
    );
    Ok(())
}

fn report(home: &HomeLayout, browser: Option<&Path>) -> Value {
    json!({
        "implementation":"rust",
        "home":home.root(),
        "runtimes":cccc_runtime::detect_runtimes(),
        "projected_browser":{
            "mode":"headless",
            "browser_available":browser.is_some(),
            "browser_path":browser,
            "xvfb_required":false,
            "note":"Rust browser surfaces run headless and do not attach to the host desktop."
        }
    })
}

fn find_browser() -> Option<PathBuf> {
    let names = if cfg!(target_os = "windows") {
        &["chrome.exe", "msedge.exe", "chromium.exe"][..]
    } else {
        &[
            "google-chrome",
            "google-chrome-stable",
            "chromium",
            "chromium-browser",
            "microsoft-edge",
            "microsoft-edge-stable",
        ][..]
    };
    if let Some(path) = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .flat_map(|directory| names.iter().map(move |name| directory.join(name)))
            .find(|path| path.is_file())
    }) {
        return Some(path);
    }
    if cfg!(target_os = "macos") {
        [
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
        ]
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_rust_headless_browser_contract_without_xvfb_requirement() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let browser = Path::new("/usr/bin/google-chrome");
        let value = report(&home, Some(browser));
        assert_eq!(value["projected_browser"]["mode"], "headless");
        assert_eq!(value["projected_browser"]["browser_available"], true);
        assert_eq!(
            value["projected_browser"]["browser_path"],
            browser.to_string_lossy().as_ref()
        );
        assert_eq!(value["projected_browser"]["xvfb_required"], false);
    }
}
