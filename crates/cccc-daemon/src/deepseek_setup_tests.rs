use super::*;

#[path = "deepseek_setup_tests/install_tests.rs"]
mod install_tests;

fn test_env(root: &Path) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("HOME".into(), root.to_string_lossy().into_owned()),
        ("PATH".into(), "/test/bin".into()),
    ])
}

fn cccc_executable(root: &Path) -> PathBuf {
    let path = root.join(if cfg!(windows) { "cccc.exe" } else { "cccc" });
    fs::write(&path, b"cccc").expect("cccc executable");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("executable mode");
    }
    path
}

fn install_fixture(dsh_home: &Path, _env: &BTreeMap<String, String>) -> Result<(), String> {
    for (package, version) in required_packages() {
        let manifest = dsh_home
            .join("node_modules")
            .join(package)
            .join("package.json");
        fs::create_dir_all(manifest.parent().expect("manifest parent"))
            .map_err(|error| error.to_string())?;
        fs::write(manifest, format!(r#"{{"version":"{version}"}}"#))
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn fixture_ready(_command: &[String], env: &BTreeMap<String, String>) -> Result<(), String> {
    let dsh_home = PathBuf::from(env.get("DSH_HOME").ok_or("missing DSH_HOME")?);
    if !packages_ready(&dsh_home) {
        return Err("packages missing".into());
    }
    let profile = dsh_home.join("profiles/cccc-acp");
    let manifest: Value = serde_json::from_slice(
        &fs::read(profile.join("package.json")).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    if !cccc_runtime::is_canonical_deepseek_profile_manifest(&manifest) {
        return Err("invalid profile".into());
    }
    let patch =
        fs::read_to_string(profile.join("cordis.patch.yml")).map_err(|error| error.to_string())?;
    if !cccc_runtime::is_canonical_deepseek_patch(&patch) {
        return Err("invalid patch".into());
    }
    let config =
        fs::read_to_string(profile.join("cordis.yml")).map_err(|error| error.to_string())?;
    if !cccc_runtime::is_canonical_deepseek_config(&config) {
        return Err("invalid config".into());
    }
    Ok(())
}

#[test]
fn first_use_installs_packages_creates_profile_and_is_idempotent() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut env = test_env(temp.path());
    let executable = cccc_executable(temp.path());
    let first = ensure_with(
        &mut env,
        &executable,
        install_fixture,
        |_command, _env| Ok(()),
        fixture_ready,
    )
    .expect("first setup");
    assert_eq!(first.dsh_home, temp.path().join(".dsh"));
    assert!(first.packages_installed);
    assert!(first.profile_created);
    assert_eq!(
        env.get("DSH_HOME"),
        Some(&first.dsh_home.to_string_lossy().into_owned())
    );
    assert_eq!(
        std::env::split_paths(env.get("PATH").expect("path")).next(),
        Some(first.dsh_home.join("node_modules/.bin"))
    );
    assert_eq!(env.get("NODE_USE_ENV_PROXY").map(String::as_str), Some("1"));
    let first_files = ["package.json", "cordis.patch.yml", "cordis.yml"].map(|name| {
        (
            name,
            fs::read(first.profile.join(name)).expect("first profile file"),
        )
    });

    let second = ensure_with(
        &mut env,
        &executable,
        |_home, _env| Err("installer must not run".into()),
        |_command, _env| Ok(()),
        fixture_ready,
    )
    .expect("idempotent setup");
    assert!(!second.packages_installed);
    assert!(!second.profile_created);
    for (name, expected) in first_files {
        assert_eq!(
            fs::read(second.profile.join(name)).expect("second profile file"),
            expected
        );
    }
}

#[cfg(unix)]
#[test]
fn profile_paths_escape_yaml_apostrophes() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("tempdir");
    let executable = temp.path().join("acme's").join("cccc");
    fs::create_dir_all(executable.parent().expect("executable parent")).expect("parent");
    fs::write(&executable, b"cccc").expect("executable");
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).expect("permissions");
    let profile = temp.path().join("profile");

    write_profile_files(&profile, &executable).expect("profile");

    let escaped_path = executable.to_string_lossy().replace('\'', "''");
    let patch = fs::read_to_string(profile.join("cordis.patch.yml")).expect("patch");
    let config = fs::read_to_string(profile.join("cordis.yml")).expect("config");
    assert!(patch.contains(&format!("command: '{escaped_path}'")));
    assert!(config.contains(&format!("command: '{escaped_path}'")));
    assert!(cccc_runtime::is_canonical_deepseek_patch(&patch));
    assert!(cccc_runtime::is_canonical_deepseek_config(&config));
}

#[test]
fn explicit_node_proxy_setting_is_preserved() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut env = test_env(temp.path());
    env.insert("NODE_USE_ENV_PROXY".into(), "0".into());
    let executable = cccc_executable(temp.path());
    ensure_with(
        &mut env,
        &executable,
        install_fixture,
        |_command, _env| Ok(()),
        fixture_ready,
    )
    .expect("setup");
    assert_eq!(env.get("NODE_USE_ENV_PROXY").map(String::as_str), Some("0"));
}

#[test]
fn upgrades_three_package_managed_profile_to_full_acp_composition() {
    let temp = tempfile::tempdir().expect("tempdir");
    let dsh_home = temp.path().join(".dsh");
    for (package, version) in required_packages().into_iter().take(3) {
        let manifest = dsh_home
            .join("node_modules")
            .join(package)
            .join("package.json");
        fs::create_dir_all(manifest.parent().expect("manifest parent")).expect("package dir");
        fs::write(manifest, format!(r#"{{"version":"{version}"}}"#)).expect("manifest");
    }
    let profile = dsh_home.join("profiles/cccc-acp");
    fs::create_dir_all(&profile).expect("profile dir");
    fs::write(profile.join("package.json"), r#"{"ccccManaged":true}"#).expect("old manifest");
    fs::write(profile.join("cordis.yml"), "[]\n").expect("old config");

    let mut env = test_env(temp.path());
    let outcome = ensure_with(
        &mut env,
        &cccc_executable(temp.path()),
        install_fixture,
        |_command, _env| Err("deepseek executable not found: dsh-acp-demo".into()),
        fixture_ready,
    )
    .expect("upgrade setup");
    assert!(outcome.packages_installed);
    assert!(!outcome.profile_created);
    assert!(packages_ready(&dsh_home));
    assert!(cccc_runtime::is_canonical_deepseek_config(
        &fs::read_to_string(profile.join("cordis.yml")).expect("canonical config")
    ));
    let first_files = ["package.json", "cordis.patch.yml", "cordis.yml"].map(|name| {
        (
            name,
            fs::read(profile.join(name)).expect("migrated profile file"),
        )
    });
    let second = ensure_with(
        &mut env,
        &cccc_executable(temp.path()),
        |_home, _env| Err("installer must not run".into()),
        |_command, _env| Ok(()),
        fixture_ready,
    )
    .expect("idempotent migrated setup");
    assert!(!second.packages_installed);
    assert!(!second.profile_created);
    for (name, expected) in first_files {
        assert_eq!(
            fs::read(profile.join(name)).expect("stable migrated profile file"),
            expected
        );
    }
}

#[test]
fn failed_install_leaves_profile_absent_and_retryable() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut env = test_env(temp.path());
    let executable = cccc_executable(temp.path());
    let error = ensure_with(
        &mut env,
        &executable,
        |_home, _env| Err("offline".into()),
        |_command, _env| Ok(()),
        fixture_ready,
    )
    .expect_err("install failure");
    assert!(error.contains("offline"));
    assert!(!temp.path().join(".dsh/profiles/cccc-acp").exists());
    ensure_with(
        &mut env,
        &executable,
        install_fixture,
        |_command, _env| Ok(()),
        fixture_ready,
    )
    .expect("retry setup");
}
