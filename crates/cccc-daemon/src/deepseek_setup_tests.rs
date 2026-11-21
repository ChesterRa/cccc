use super::*;
use serde_json::json;

#[path = "deepseek_setup_tests/install_tests.rs"]
mod install_tests;

const ACP_PROFILE_PACKAGE: &str = "@deepseek-ai/dsh-acp-app";
const ACP_BRIDGE_PACKAGE: &str = "@deepseek-ai/dsh-acp";

fn test_env(root: &Path) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("HOME".into(), root.to_string_lossy().into_owned()),
        ("PATH".into(), "/test/bin".into()),
    ])
}

fn test_home(root: &Path) -> HomeLayout {
    HomeLayout::from_path(root.join("cccc-home")).expect("home")
}

fn node_supported(_command: &[String], _env: &BTreeMap<String, String>) -> Result<(), String> {
    Ok(())
}

fn default_command() -> Vec<String> {
    cccc_runtime::default_command(ActorRuntime::Deepseek)
}

/// What a successful `npm install` of the pinned launcher leaves behind.
fn install_fixture(dsh_home: &Path, _env: &BTreeMap<String, String>) -> Result<(), String> {
    write_installation(dsh_home).map_err(|error| error.to_string())
}

fn write_installation(dsh_home: &Path) -> std::io::Result<()> {
    let mut lock = serde_json::Map::from_iter([(
        String::new(),
        json!({"dependencies": {DEEPSEEK_PACKAGE: DEEPSEEK_RELEASE_VERSION}}),
    )]);
    for package in [DEEPSEEK_PACKAGE, ACP_PROFILE_PACKAGE, ACP_BRIDGE_PACKAGE] {
        let dir = dsh_home.join("node_modules").join(package);
        fs::create_dir_all(&dir)?;
        let mut manifest = json!({"version": DEEPSEEK_RELEASE_VERSION});
        if package == ACP_PROFILE_PACKAGE {
            manifest["dsh"] = json!({"bundle": {"patch": "./cordis.patch.yml"}});
            fs::write(dir.join("cordis.patch.yml"), "[]\n")?;
        }
        fs::write(dir.join("package.json"), serde_json::to_vec(&manifest)?)?;
        lock.insert(
            format!("node_modules/{package}"),
            json!({"version": DEEPSEEK_RELEASE_VERSION}),
        );
    }
    write_launcher(dsh_home)?;
    fs::write(
        dsh_home.join("package.json"),
        serde_json::to_vec(&cccc_runtime::canonical_deepseek_runtime_manifest())?,
    )?;
    fs::write(
        dsh_home.join("package-lock.json"),
        serde_json::to_vec(&json!({"lockfileVersion": 3, "packages": lock}))?,
    )
}

fn write_launcher(dsh_home: &Path) -> std::io::Result<()> {
    let bin = dsh_home.join("node_modules/.bin");
    fs::create_dir_all(&bin)?;
    #[cfg(windows)]
    fs::write(bin.join("dsh.cmd"), "@exit /b 0\r\n")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let launcher = bin.join("dsh");
        fs::write(&launcher, "#!/bin/sh\nexit 0\n")?;
        fs::set_permissions(&launcher, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

fn remove_launcher(dsh_home: &Path) {
    let bin = dsh_home.join("node_modules/.bin");
    for name in ["dsh", "dsh.cmd"] {
        match fs::remove_file(bin.join(name)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("remove launcher: {error}"),
        }
    }
}

#[test]
fn first_use_installs_once_and_leaves_the_agent_environment_to_the_launch() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = test_home(temp.path());
    let previous = home.root().join("runtimes/deepseek/0.1.0-rc.6");
    fs::create_dir_all(&previous).expect("previous installation");
    fs::write(previous.join("package.json"), "{}").expect("previous manifest");
    let mut env = test_env(temp.path());

    let first = ensure_with(
        &home,
        &mut env,
        install_fixture,
        node_supported,
        cccc_runtime::deepseek_installation_ready,
    )
    .expect("first setup");
    let dsh_home = home
        .root()
        .join("runtimes/deepseek")
        .join(DEEPSEEK_RELEASE_VERSION);
    assert_eq!(
        first,
        DeepSeekSetupOutcome {
            dsh_home: dsh_home.clone(),
            profile: dsh_home.join("node_modules").join(ACP_PROFILE_PACKAGE),
            packages_installed: true,
            profile_created: false,
        }
    );
    // Each agent's launch chooses its own DSH_HOME and starts the launcher by
    // its path, so setup sets neither.
    assert!(!env.contains_key("DSH_HOME"));
    assert_eq!(env.get("PATH").map(String::as_str), Some("/test/bin"));
    assert_eq!(
        env.get("CCCC_HOME"),
        Some(&home.root().to_string_lossy().into_owned())
    );
    assert_eq!(env.get("NODE_USE_ENV_PROXY").map(String::as_str), Some("1"));

    let second = ensure_with(
        &home,
        &mut env,
        |_home, _env| Err("installer must not run".into()),
        node_supported,
        cccc_runtime::deepseek_installation_ready,
    )
    .expect("idempotent setup");
    assert!(!second.packages_installed);
    // Agents that still run from the previous release keep their files.
    assert_eq!(
        fs::read(previous.join("package.json")).expect("previous manifest"),
        b"{}"
    );
}

#[test]
fn an_unsupported_node_stops_setup_before_any_download() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = test_home(temp.path());
    let mut env = test_env(temp.path());
    let error = ensure_with(
        &home,
        &mut env,
        |_home, _env| Err("installer must not run".into()),
        |_command, _env| Err("DeepSeek Harness requires Node >=24.9.0 (found v22.19.0)".into()),
        cccc_runtime::deepseek_installation_ready,
    )
    .expect_err("old node");
    assert!(error.contains(">=24.9.0"), "{error}");
    assert!(!home.root().join("runtimes/deepseek").exists());
}

#[test]
fn a_missing_launcher_is_repaired_by_installing_again() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = test_home(temp.path());
    let dsh_home = home
        .root()
        .join("runtimes/deepseek")
        .join(DEEPSEEK_RELEASE_VERSION);
    write_installation(&dsh_home).expect("installation");
    remove_launcher(&dsh_home);
    let mut env = test_env(temp.path());

    // An installer that cannot restore the launcher leaves a clear error.
    let error = ensure_with(
        &home,
        &mut env,
        |_home, _env| Ok(()),
        node_supported,
        cccc_runtime::deepseek_installation_ready,
    )
    .expect_err("launcher still missing");
    assert!(
        error.contains("still incomplete after installation")
            && error.contains("deepseek executable not found: dsh"),
        "{error}"
    );

    let installs = std::cell::Cell::new(0_u32);
    let outcome = ensure_with(
        &home,
        &mut env,
        |root, env| {
            installs.set(installs.get() + 1);
            install_fixture(root, env)
        },
        node_supported,
        cccc_runtime::deepseek_installation_ready,
    )
    .expect("repaired setup");
    assert_eq!(installs.get(), 1);
    assert!(outcome.packages_installed);
    cccc_runtime::deepseek_installation_ready(&dsh_home).expect("ready after repair");
}

#[test]
fn a_profile_bundle_without_its_patch_is_unpacked_again_alone() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = test_home(temp.path());
    let dsh_home = home
        .root()
        .join("runtimes/deepseek")
        .join(DEEPSEEK_RELEASE_VERSION);
    write_installation(&dsh_home).expect("installation");
    let profile = dsh_home.join("node_modules").join(ACP_PROFILE_PACKAGE);
    fs::remove_file(profile.join("cordis.patch.yml")).expect("remove bundle patch");
    let launcher_marker = dsh_home
        .join("node_modules")
        .join(DEEPSEEK_PACKAGE)
        .join("marker");
    fs::write(&launcher_marker, "kept").expect("marker");
    let installs = std::cell::Cell::new(0_u32);
    let mut env = test_env(temp.path());
    let outcome = ensure_with(
        &home,
        &mut env,
        |root, env| {
            installs.set(installs.get() + 1);
            // npm would keep a package directory at the right version as it
            // is; setup must have removed the incomplete one.
            assert!(!profile.exists(), "incomplete bundle still present");
            install_fixture(root, env)
        },
        node_supported,
        cccc_runtime::deepseek_installation_ready,
    )
    .expect("repaired setup");
    assert_eq!(installs.get(), 1);
    assert!(outcome.packages_installed);
    assert!(profile.join("cordis.patch.yml").is_file());
    assert_eq!(fs::read(&launcher_marker).expect("marker"), b"kept");
}

#[test]
fn a_changed_runtime_manifest_is_installed_again() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = test_home(temp.path());
    let dsh_home = home
        .root()
        .join("runtimes/deepseek")
        .join(DEEPSEEK_RELEASE_VERSION);
    write_installation(&dsh_home).expect("installation");
    fs::write(
        dsh_home.join("package.json"),
        serde_json::to_vec(&json!({
            "name": "cccc-deepseek-runtime",
            "private": true,
            "ccccManaged": true,
            "dependencies": {"@deepseek-ai/dsh-acp-demo": "0.1.0-rc.6"}
        }))
        .expect("json"),
    )
    .expect("changed manifest");
    let installs = std::cell::Cell::new(0_u32);
    let mut env = test_env(temp.path());
    let outcome = ensure_with(
        &home,
        &mut env,
        |root, env| {
            installs.set(installs.get() + 1);
            install_fixture(root, env)
        },
        node_supported,
        cccc_runtime::deepseek_installation_ready,
    )
    .expect("reinstalled setup");
    assert_eq!(installs.get(), 1);
    assert!(outcome.packages_installed);
}

#[test]
fn a_failed_install_can_be_retried() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = test_home(temp.path());
    let mut env = test_env(temp.path());
    let error = ensure_with(
        &home,
        &mut env,
        |_home, _env| Err("offline".into()),
        node_supported,
        cccc_runtime::deepseek_installation_ready,
    )
    .expect_err("install failure");
    assert!(error.contains("offline"), "{error}");
    ensure_with(
        &home,
        &mut env,
        install_fixture,
        node_supported,
        cccc_runtime::deepseek_installation_ready,
    )
    .expect("retry setup");
}

#[test]
fn a_start_never_installs_and_names_the_setup_command() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = test_home(temp.path());
    let mut env = test_env(temp.path());
    let error = prepare_for_start_with(
        &home,
        &mut env,
        &default_command(),
        node_supported,
        cccc_runtime::deepseek_installation_ready,
    )
    .expect_err("not installed");
    assert!(
        error.contains("cccc setup --runtime deepseek")
            && error.contains(&format!("{DEEPSEEK_PACKAGE}@{DEEPSEEK_RELEASE_VERSION}")),
        "{error}"
    );
    assert!(!home.root().join("runtimes/deepseek").exists());

    let dsh_home = home
        .root()
        .join("runtimes/deepseek")
        .join(DEEPSEEK_RELEASE_VERSION);
    write_installation(&dsh_home).expect("installation");
    remove_launcher(&dsh_home);
    let error = prepare_for_start_with(
        &home,
        &mut env,
        &default_command(),
        node_supported,
        cccc_runtime::deepseek_installation_ready,
    )
    .expect_err("incomplete");
    assert!(
        error.contains("deepseek executable not found: dsh")
            && error.contains("cccc setup --runtime deepseek"),
        "{error}"
    );
}

#[test]
fn a_start_over_a_complete_installation_prepares_the_same_environment_as_setup() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = test_home(temp.path());
    let dsh_home = home
        .root()
        .join("runtimes/deepseek")
        .join(DEEPSEEK_RELEASE_VERSION);
    write_installation(&dsh_home).expect("installation");
    let mut env = test_env(temp.path());
    env.insert(
        "DEEPSEEK_BASE_URL".into(),
        "http://127.0.0.2:8000/v1".into(),
    );
    // The member's own command is what gets checked, so a custom launcher
    // keeps its executable check.
    let custom = vec![
        "custom-dsh".to_owned(),
        "--profile".to_owned(),
        "acp".to_owned(),
    ];
    let started = prepare_for_start_with(
        &home,
        &mut env,
        &custom,
        |command, _env| {
            assert_eq!(command, custom.as_slice());
            Ok(())
        },
        cccc_runtime::deepseek_installation_ready,
    )
    .expect("start");
    let mut setup_env = test_env(temp.path());
    setup_env.insert(
        "DEEPSEEK_BASE_URL".into(),
        "http://127.0.0.2:8000/v1".into(),
    );
    let setup = ensure_with(
        &home,
        &mut setup_env,
        |_home, _env| Err("installer must not run".into()),
        node_supported,
        cccc_runtime::deepseek_installation_ready,
    )
    .expect("setup");
    // A ready check reports what setup reports for an installation it keeps.
    assert_eq!(started, setup);
    assert!(!started.packages_installed);
    assert_eq!(env, setup_env);
    assert_eq!(env.get("NO_PROXY").map(String::as_str), Some("127.0.0.2"));

    let error = prepare_for_start_with(
        &home,
        &mut env,
        &default_command(),
        |_command, _env| Err("DeepSeek Harness requires Node >=24.9.0 (found v22.19.0)".into()),
        cccc_runtime::deepseek_installation_ready,
    )
    .expect_err("old node");
    assert!(
        error.starts_with("DeepSeek Harness requires Node"),
        "{error}"
    );
}

#[test]
fn explicit_node_proxy_setting_is_preserved() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = test_home(temp.path());
    let mut env = test_env(temp.path());
    env.insert("NODE_USE_ENV_PROXY".into(), "0".into());
    ensure_with(
        &home,
        &mut env,
        install_fixture,
        node_supported,
        cccc_runtime::deepseek_installation_ready,
    )
    .expect("setup");
    assert_eq!(env.get("NODE_USE_ENV_PROXY").map(String::as_str), Some("0"));
}

#[test]
fn loopback_base_extends_the_effective_node_proxy_exclusions() {
    for (base, initial, key, expected) in [
        ("http://127.0.0.2:8000/v1", vec![], "NO_PROXY", "127.0.0.2"),
        (
            "http://localhost.:8000",
            vec![("NO_PROXY", "internal.example")],
            "NO_PROXY",
            "internal.example,localhost.",
        ),
        (
            "http://[::1]:8000",
            vec![("NO_PROXY", "upper.example"), ("no_proxy", "lower.example")],
            "no_proxy",
            "lower.example,[::1]",
        ),
        (
            "http://[::ffff:127.0.0.1]:8000",
            vec![],
            "NO_PROXY",
            "[::ffff:7f00:1]",
        ),
        (
            "http://127.0.0.1:8000",
            vec![("NO_PROXY", "upper.example"), ("no_proxy", "")],
            "no_proxy",
            "127.0.0.1",
        ),
        (
            "http://127.0.0.1:8000",
            vec![("NO_PROXY", "*")],
            "NO_PROXY",
            "*",
        ),
        (
            "http://[::1]:8000",
            vec![("NO_PROXY", "upper.example"), ("no_proxy", "*")],
            "no_proxy",
            "*",
        ),
    ] {
        let mut env = initial
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect::<BTreeMap<_, _>>();
        env.insert("DEEPSEEK_BASE_URL".into(), base.into());
        let before = env.clone();
        bypass_proxy_for_loopback_base(&mut env);
        assert_eq!(env.get(key).map(String::as_str), Some(expected), "{base}");
        for (name, value) in before.iter().filter(|(name, _)| name.as_str() != key) {
            assert_eq!(env.get(name), Some(value), "{name} changed for {base}");
        }
        let once = env.clone();
        bypass_proxy_for_loopback_base(&mut env);
        assert_eq!(env, once, "duplicate exclusion for {base}");
    }
}

#[test]
fn setup_passes_the_loopback_exclusion_to_the_runtime() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = test_home(temp.path());
    let mut env = test_env(temp.path());
    env.insert(
        "DEEPSEEK_BASE_URL".into(),
        "http://127.0.0.2:8000/v1".into(),
    );
    env.insert("NO_PROXY".into(), "internal.example".into());
    ensure_with(
        &home,
        &mut env,
        |_home, _env| Err("installer must not run".into()),
        |command, env| {
            // Setup has no member, so it checks the default launcher.
            assert_eq!(command, default_command().as_slice());
            assert_eq!(
                env.get("NO_PROXY").map(String::as_str),
                Some("internal.example,127.0.0.2")
            );
            Ok(())
        },
        |_dsh_home| Ok(()),
    )
    .expect("setup");
}

#[test]
fn remote_or_missing_base_does_not_change_proxy_exclusions() {
    for base in [
        None,
        Some("https://api.deepseek.com"),
        Some("http://localhost.example.com"),
        Some("invalid URL"),
    ] {
        let mut env = BTreeMap::from([
            ("NO_PROXY".into(), "upper.example".into()),
            ("no_proxy".into(), "lower.example".into()),
        ]);
        if let Some(base) = base {
            env.insert("DEEPSEEK_BASE_URL".into(), base.into());
        }
        let before = env.clone();
        bypass_proxy_for_loopback_base(&mut env);
        assert_eq!(env, before);
    }
}
