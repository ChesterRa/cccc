use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

const WEB_INPUTS: &[&str] = &[
    "src",
    "public",
    "index.html",
    "package.json",
    "package-lock.json",
    "components.json",
    "postcss.config.cjs",
    "tailwind.config.ts",
    "tsconfig.json",
    "vite.config.ts",
];

fn main() {
    println!("cargo:rerun-if-env-changed=CCCC_FORCE_WEB_BUILD");
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("cccc-web must be inside the workspace crates directory");
    let web = workspace.join("web");
    if !web.join("package.json").is_file() {
        let assets = manifest_dir.join("assets/web-dist");
        assert!(
            assets.join("index.html").is_file(),
            "packaged Web assets are missing from {}",
            assets.display()
        );
        export_assets_dir(&assets);
        return;
    }
    for input in WEB_INPUTS {
        println!("cargo:rerun-if-changed={}", web.join(input).display());
    }

    let index = web.join("dist/index.html");
    println!("cargo:rerun-if-changed={}", index.display());
    let force = std::env::var_os("CCCC_FORCE_WEB_BUILD").is_some();
    if !force && web_bundle_is_current(&web, &index).unwrap_or(false) {
        export_assets_dir(&web.join("dist"));
        return;
    }
    build_web(workspace, &web, &index);
    export_assets_dir(&web.join("dist"));
}

fn export_assets_dir(path: &Path) {
    println!("cargo:rustc-env=CCCC_WEB_DIST_DIR={}", path.display());
}

fn web_bundle_is_current(web: &Path, index: &Path) -> io::Result<bool> {
    let built_at = fs::metadata(index)?.modified()?;
    Ok(latest_input_change(web)? <= built_at)
}

fn latest_input_change(web: &Path) -> io::Result<SystemTime> {
    let mut latest = SystemTime::UNIX_EPOCH;
    for input in WEB_INPUTS {
        latest = latest.max(latest_change(&web.join(input))?);
    }
    Ok(latest)
}

fn latest_change(path: &Path) -> io::Result<SystemTime> {
    let metadata = fs::metadata(path)?;
    if metadata.is_file() {
        return metadata.modified();
    }
    let mut latest = metadata.modified()?;
    for entry in fs::read_dir(path)? {
        latest = latest.max(latest_change(&entry?.path())?);
    }
    Ok(latest)
}

fn build_web(workspace: &Path, web: &Path, index: &Path) {
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    if !web_dependencies_are_current(npm, workspace, web) {
        run(
            Command::new(npm)
                .current_dir(workspace)
                .args(["ci", "--prefix", "web"]),
            "install Web dependencies",
        );
    }
    run(
        Command::new(npm)
            .current_dir(workspace)
            .args(["-C", "web", "run", "build"]),
        "build the Web UI",
    );
    if !index.is_file() {
        panic!("Web build completed without producing {}", index.display());
    }
}

fn web_dependencies_are_current(npm: &str, workspace: &Path, web: &Path) -> bool {
    if !web.join("node_modules").is_dir() {
        return false;
    }
    Command::new(npm)
        .current_dir(workspace)
        .args(["-C", "web", "ls", "--depth=0", "--silent"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn run(command: &mut Command, action: &str) {
    let display = format!("{command:?}");
    let status = command.status().unwrap_or_else(|error| {
        panic!("failed to {action} with {display}: {error}");
    });
    assert!(
        status.success(),
        "failed to {action} with {display}: {status}"
    );
}
