use super::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn concurrent_first_use_runs_the_installer_once() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().to_path_buf();
    let executable = cccc_executable(&root);
    let installs = Arc::new(AtomicUsize::new(0));
    let mut threads = Vec::new();
    for _ in 0..4 {
        let root = root.clone();
        let executable = executable.clone();
        let installs = Arc::clone(&installs);
        threads.push(std::thread::spawn(move || {
            let home = test_home(&root);
            let mut env = test_env(&root);
            ensure_with(
                &home,
                &mut env,
                &executable,
                |home, env| {
                    installs.fetch_add(1, Ordering::SeqCst);
                    install_fixture(home, env)
                },
                |_command, _env| Ok(()),
                fixture_ready,
            )
            .expect("concurrent setup");
        }));
    }
    for thread in threads {
        thread.join().expect("join setup");
    }
    assert_eq!(installs.load(Ordering::SeqCst), 1);
}

#[cfg(unix)]
#[test]
fn npm_installer_writes_the_pinned_package_tuple() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("tempdir");
    let npm = temp.path().join("npm");
    fs::write(
        &npm,
        r#"#!/bin/sh
set -eu
printf '%s\n' "$@" > npm-args.txt
for spec in "$@"; do
  case "$spec" in
    @deepseek-ai/dsh-acp@*) name=dsh-acp; version=0.1.0-rc.6 ;;
    @deepseek-ai/dsh-mcp-client@*) name=dsh-mcp-client; version=0.1.0-rc.6 ;;
    @deepseek-ai/dsh-acp-demo@*) name=dsh-acp-demo; version=0.1.0-rc.6 ;;
    @deepseek-ai/dsh-llm-deepseek@*) name=dsh-llm-deepseek; version=0.1.0-rc.6 ;;
    *) continue ;;
  esac
  mkdir -p "node_modules/@deepseek-ai/$name"
  printf '{"version":"%s"}\n' "$version" > "node_modules/@deepseek-ai/$name/package.json"
done
printf '%s\n' '{"lockfileVersion":3,"packages":{"":{"dependencies":{"@deepseek-ai/dsh-acp":"0.1.0-rc.6","@deepseek-ai/dsh-mcp-client":"0.1.0-rc.6","@deepseek-ai/dsh-acp-demo":"0.1.0-rc.6","@deepseek-ai/dsh-llm-deepseek":"0.1.0-rc.6"}},"node_modules/@deepseek-ai/dsh-acp":{"version":"0.1.0-rc.6"},"node_modules/@deepseek-ai/dsh-mcp-client":{"version":"0.1.0-rc.6"},"node_modules/@deepseek-ai/dsh-acp-demo":{"version":"0.1.0-rc.6"},"node_modules/@deepseek-ai/dsh-llm-deepseek":{"version":"0.1.0-rc.6"}}}' > package-lock.json
"#,
    )
    .expect("npm fixture");
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
    install_packages(&dsh_home, &env).expect("install packages");
    assert!(packages_ready(&dsh_home));
    let args = fs::read_to_string(dsh_home.join("npm-args.txt")).expect("npm args");
    assert!(args.lines().any(|line| line == DEEPSEEK_NPM_BEFORE));
    assert!(
        !args
            .lines()
            .any(|line| line == "@deepseek-ai/dsh@0.1.0-rc.6")
    );
}
