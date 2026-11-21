use super::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn concurrent_first_use_runs_the_installer_once() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().to_path_buf();
    let installs = Arc::new(AtomicUsize::new(0));
    let mut threads = Vec::new();
    for _ in 0..4 {
        let root = root.clone();
        let installs = Arc::clone(&installs);
        threads.push(std::thread::spawn(move || {
            let home = test_home(&root);
            let mut env = test_env(&root);
            ensure_with(
                &home,
                &mut env,
                |home, env| {
                    installs.fetch_add(1, Ordering::SeqCst);
                    install_fixture(home, env)
                },
                node_supported,
                cccc_runtime::deepseek_installation_ready,
            )
            .expect("concurrent setup");
        }));
    }
    for thread in threads {
        thread.join().expect("join setup");
    }
    assert_eq!(installs.load(Ordering::SeqCst), 1);
}

/// Runs `install_packages` against an `npm` stand-in with this body.
#[cfg(unix)]
fn run_npm_fixture(body: &str) -> (tempfile::TempDir, PathBuf, Result<(), String>) {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("tempdir");
    let npm = temp.path().join("npm");
    fs::write(&npm, format!("#!/bin/sh\n{body}")).expect("npm fixture");
    fs::set_permissions(&npm, fs::Permissions::from_mode(0o755)).expect("npm mode");
    let dsh_home = temp.path().join("dsh-runtime");
    fs::create_dir(&dsh_home).expect("dsh home");
    let mut paths = vec![temp.path().to_path_buf()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let env = BTreeMap::from([(
        "PATH".into(),
        std::env::join_paths(paths)
            .expect("fixture path")
            .to_string_lossy()
            .into_owned(),
    )]);
    let result = install_packages(&dsh_home, &env);
    (temp, dsh_home, result)
}

#[cfg(unix)]
#[test]
fn npm_installs_only_the_pinned_launcher_without_dependency_scripts() {
    let (_temp, dsh_home, result) = run_npm_fixture(
        "printf '%s\\n' \"$@\" > npm-args.txt\necho added 671 packages\necho 'npm warn deprecated' >&2\n",
    );
    result.expect("install packages");
    let args = fs::read_to_string(dsh_home.join("npm-args.txt")).expect("npm args");
    let args = args.lines().collect::<Vec<_>>();
    let launcher = format!("{DEEPSEEK_PACKAGE}@{DEEPSEEK_RELEASE_VERSION}");
    assert_eq!(
        args,
        [
            "install",
            "--save-exact",
            "--no-audit",
            "--no-fund",
            "--ignore-scripts",
            "--before",
            DEEPSEEK_NPM_BEFORE,
            launcher.as_str(),
        ]
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(dsh_home.join("package.json")).expect("manifest"))
            .expect("manifest json");
    assert_eq!(
        manifest,
        cccc_runtime::canonical_deepseek_runtime_manifest()
    );
    let log = fs::read_to_string(dsh_home.join("npm-install.log")).expect("npm log");
    assert!(
        log.contains("added 671 packages") && log.contains("npm warn deprecated"),
        "{log}"
    );
}

#[cfg(unix)]
#[test]
fn npm_failure_points_to_its_output() {
    let (_temp, dsh_home, result) = run_npm_fixture("echo 'npm error code ETARGET' >&2\nexit 1\n");
    let error = result.expect_err("npm failure");
    let log = dsh_home.join("npm-install.log");
    assert!(error.contains(&log.display().to_string()), "{error}");
    assert!(
        fs::read_to_string(&log)
            .expect("npm log")
            .contains("npm error code ETARGET")
    );
}
