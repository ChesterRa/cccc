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
            "mode":"hybrid",
            "web_model_mode":"system_browser_cdp",
            "other_surface_mode":"headless",
            "browser_available":browser.is_some(),
            "browser_path":browser,
            "xvfb_required":false,
            "system_browser_available":browser.is_some(),
            "system_browser_path":browser,
            "xvfb_required_for_linux_web_model":false,
            "note":"Web Model uses system Chrome/Edge/Chromium via CDP; Linux uses Xvfb when available and otherwise falls back to headless CDP projection."
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
            "microsoft-edge",
            "microsoft-edge-stable",
            "chromium",
            "chromium-browser",
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
    fn reports_hybrid_browser_contract() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let browser = Path::new("/usr/bin/google-chrome");
        let value = report(&home, Some(browser));
        assert_eq!(value["projected_browser"]["mode"], "hybrid");
        assert_eq!(
            value["projected_browser"]["web_model_mode"],
            "system_browser_cdp"
        );
        assert_eq!(value["projected_browser"]["other_surface_mode"], "headless");
        assert_eq!(value["projected_browser"]["browser_available"], true);
        assert_eq!(
            value["projected_browser"]["browser_path"],
            browser.to_string_lossy().as_ref()
        );
        assert_eq!(value["projected_browser"]["xvfb_required"], false);
        assert_eq!(value["projected_browser"]["system_browser_available"], true);
        assert_eq!(
            value["projected_browser"]["system_browser_path"],
            browser.to_string_lossy().as_ref()
        );
        assert_eq!(
            value["projected_browser"]["xvfb_required_for_linux_web_model"],
            false
        );
    }
}
