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

fn test_home(root: &Path) -> HomeLayout {
    HomeLayout::from_path(root.join("cccc-home")).expect("home")
}

fn install_fixture(dsh_home: &Path, _env: &BTreeMap<String, String>) -> Result<(), String> {
    let mut lock_packages = serde_json::Map::new();
    for (package, version) in required_packages() {
        let manifest = dsh_home
            .join("node_modules")
            .join(package)
            .join("package.json");
        fs::create_dir_all(manifest.parent().expect("manifest parent"))
            .map_err(|error| error.to_string())?;
        fs::write(manifest, format!(r#"{{"version":"{version}"}}"#))
            .map_err(|error| error.to_string())?;
        lock_packages.insert(
            format!("node_modules/{package}"),
            json!({"version": version}),
        );
    }
    let dependencies = required_packages().into_iter().collect::<BTreeMap<_, _>>();
    lock_packages.insert("".into(), json!({"dependencies": dependencies}));
    fs::write(
        dsh_home.join("package.json"),
        serde_json::to_vec(&cccc_runtime::canonical_deepseek_runtime_manifest())
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    fs::write(
        dsh_home.join("package-lock.json"),
        serde_json::to_vec(&json!({"lockfileVersion": 3, "packages": lock_packages}))
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
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
    let home = test_home(temp.path());
    let mut env = test_env(temp.path());
    let executable = cccc_executable(temp.path());
    let first = ensure_with(
        &home,
        &mut env,
        &executable,
        install_fixture,
        |_command, _env| Ok(()),
        fixture_ready,
    )
    .expect("first setup");
    assert_eq!(
        first.dsh_home,
        home.root()
            .join("runtimes/deepseek")
            .join(DEEPSEEK_RELEASE_VERSION)
    );
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
    let first_files = ["package.json", "cordis.yml"].map(|name| {
        (
            name,
            fs::read(first.profile.join(name)).expect("first profile file"),
        )
    });

    let second = ensure_with(
        &home,
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
    let config = fs::read_to_string(profile.join("cordis.yml")).expect("config");
    assert!(config.contains(&format!("command: '{escaped_path}'")));
    assert!(config.contains(&format!(
        "maxTokens: {}",
        cccc_contracts::DEEPSEEK_MAX_OUTPUT_TOKENS
    )));
    assert!(cccc_runtime::is_canonical_deepseek_config(&config));
}

#[test]
fn explicit_node_proxy_setting_is_preserved() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = test_home(temp.path());
    let mut env = test_env(temp.path());
    env.insert("NODE_USE_ENV_PROXY".into(), "0".into());
    let executable = cccc_executable(temp.path());
    ensure_with(
        &home,
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
    let home = test_home(temp.path());
    let dsh_home = home
        .root()
        .join("runtimes/deepseek")
        .join(DEEPSEEK_RELEASE_VERSION);
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
        &home,
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
    let first_files = ["package.json", "cordis.yml"].map(|name| {
        (
            name,
            fs::read(profile.join(name)).expect("migrated profile file"),
        )
    });
    let second = ensure_with(
        &home,
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
    let home = test_home(temp.path());
    let mut env = test_env(temp.path());
    let executable = cccc_executable(temp.path());
    let error = ensure_with(
        &home,
        &mut env,
        &executable,
        |_home, _env| Err("offline".into()),
        |_command, _env| Ok(()),
        fixture_ready,
    )
    .expect_err("install failure");
    assert!(error.contains("offline"));
    assert!(
        !home
            .root()
            .join("runtimes/deepseek")
            .join(DEEPSEEK_RELEASE_VERSION)
            .join("profiles/cccc-acp")
            .exists()
    );
    ensure_with(
        &home,
        &mut env,
        &executable,
        install_fixture,
        |_command, _env| Ok(()),
        fixture_ready,
    )
    .expect("retry setup");
}

#[cfg(unix)]
#[test]
fn no_managed_record_preserves_legacy_entry_outcome_environment_and_errors() {
    use cccc_core::cli_management as management;
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let home = test_home(temp.path());
    let executable = cccc_executable(temp.path());
    let initial = test_env(temp.path());
    let legacy = ensure_with(
        &home,
        &mut initial.clone(),
        &executable,
        install_fixture,
        |_, _| Ok(()),
        fixture_ready,
    )
    .unwrap();
    let bin = legacy.dsh_home.join("node_modules/.bin");
    fs::create_dir_all(&bin).unwrap();
    for (name, body) in [
        ("dsh-acp-demo", "#!/bin/sh\nexit 0\n"),
        ("node", "#!/bin/sh\nprintf 'v22.19.0\\n'\n"),
    ] {
        fs::write(bin.join(name), body).unwrap();
        fs::set_permissions(bin.join(name), fs::Permissions::from_mode(0o700)).unwrap();
    }
    let files = [
        legacy.dsh_home.join("package.json"),
        legacy.dsh_home.join("package-lock.json"),
        legacy.profile.join("package.json"),
        legacy.profile.join("cordis.yml"),
    ];
    let before = files.each_ref().map(|path| fs::read(path).unwrap());
    // 缺少管理状态和合法但没有受管记录的状态，都不接管原生路径。
    for existing_empty_state in [false, true] {
        if existing_empty_state {
            cccc_core::fs::write_json_committed(
                &management::root(&home).join("state.json"),
                &management::State::default(),
            )
            .unwrap();
        }
        let state_before = fs::read(management::root(&home).join("state.json")).ok();
        for proxy in [None, Some("0")] {
            let mut expected_env = initial.clone();
            expected_env.insert(
                "CCCC_DEEPSEEK_SESSION_ROOT".into(),
                "/unchanged/sessions".into(),
            );
            if let Some(proxy) = proxy {
                expected_env.insert(NODE_USE_ENV_PROXY.into(), proxy.into());
            }
            let mut actual_env = expected_env.clone();
            let expected = ensure_with(
                &home,
                &mut expected_env,
                &executable,
                |_, _| panic!("ready legacy installation must not be installed again"),
                cccc_runtime::deepseek_external_preflight,
                cccc_runtime::deepseek_preflight,
            )
            .unwrap();
            let actual = ensure(&home, &mut actual_env, &executable).unwrap();
            assert_eq!(actual, expected);
            assert!(!actual.packages_installed && !actual.profile_created);
            assert_eq!(actual_env, expected_env);
            assert_eq!(files.each_ref().map(|path| fs::read(path).unwrap()), before);
        }
        // 原生 Node 校验失败仍返回同一错误及环境，不触发安装或修复。
        fs::write(bin.join("node"), "#!/bin/sh\nprintf 'v18.0.0\\n'\n").unwrap();
        let mut expected_env = initial.clone();
        let mut actual_env = initial.clone();
        let expected = ensure_with(
            &home,
            &mut expected_env,
            &executable,
            |_, _| panic!("invalid Node must not invoke installer"),
            cccc_runtime::deepseek_external_preflight,
            cccc_runtime::deepseek_preflight,
        )
        .unwrap_err();
        assert_eq!(
            ensure(&home, &mut actual_env, &executable).unwrap_err(),
            expected
        );
        assert_eq!(actual_env, expected_env);
        assert_eq!(files.each_ref().map(|path| fs::read(path).unwrap()), before);
        assert_eq!(
            fs::read(management::root(&home).join("state.json")).ok(),
            state_before
        );
        fs::write(bin.join("node"), "#!/bin/sh\nprintf 'v22.19.0\\n'\n").unwrap();
    }
}

#[cfg(unix)]
#[test]
fn selected_install_is_read_only_and_preserves_legacy_home_and_session_root() {
    use cccc_core::cli_management as management;
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let home = test_home(temp.path());
    let executable = cccc_executable(temp.path());
    let mut env = test_env(temp.path());
    let legacy = ensure_with(
        &home,
        &mut env,
        &executable,
        install_fixture,
        |_, _| Ok(()),
        fixture_ready,
    )
    .unwrap();
    let legacy_config = fs::read(legacy.profile.join("cordis.yml")).unwrap();
    let isolated = management::root(&home).join("versions/test/deepseek");
    fs::create_dir_all(&isolated).unwrap();
    ensure_at_with(
        &home,
        isolated.clone(),
        &mut env,
        &executable,
        install_fixture,
        |_, _| Ok(()),
        fixture_ready,
    )
    .unwrap();
    let bin = isolated.join("node_modules/.bin");
    fs::create_dir_all(&bin).unwrap();
    for (name, body) in [
        ("dsh-acp-demo", "#!/bin/sh\nexit 0\n"),
        ("node", "#!/bin/sh\nprintf 'v22.19.0\\n'\n"),
    ] {
        fs::write(bin.join(name), body).unwrap();
        fs::set_permissions(bin.join(name), fs::Permissions::from_mode(0o700)).unwrap();
    }
    let now = chrono::Utc::now();
    management::submit(
        &home,
        "deepseek",
        management::Operation::Install,
        "test",
        now,
    )
    .unwrap();
    management::claim_next(&home, now).unwrap();
    management::finish(
        &home,
        "test",
        Ok(management::Installation {
            version: DEEPSEEK_RELEASE_VERSION.into(),
            executable: bin.join("dsh-acp-demo"),
            bin_paths: vec![bin],
            installed_at: now.to_rfc3339(),
        }),
        now,
    )
    .unwrap();
    env.insert(
        "CCCC_DEEPSEEK_SESSION_ROOT".into(),
        "/unchanged/sessions".into(),
    );
    let selected = ensure(&home, &mut env, &executable).unwrap();
    assert_eq!(selected.dsh_home, isolated);
    assert!(!selected.packages_installed && !selected.profile_created);
    assert_eq!(env["CCCC_DEEPSEEK_SESSION_ROOT"], "/unchanged/sessions");
    assert_eq!(
        fs::read(legacy.profile.join("cordis.yml")).unwrap(),
        legacy_config
    );
    let manifest = selected
        .dsh_home
        .join("node_modules")
        .join(DEEPSEEK_LLM_ADAPTER_PACKAGE)
        .join("package.json");
    fs::write(&manifest, r#"{"version":"incompatible"}"#).unwrap();
    assert!(ensure(&home, &mut env, &executable).is_err());
    assert_eq!(
        fs::read_to_string(manifest).unwrap(),
        r#"{"version":"incompatible"}"#
    );
}

#[test]
fn upgrades_legacy_bundle_root_and_removes_obsolete_profile_patch() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = test_home(temp.path());
    let dsh_home = home
        .root()
        .join("runtimes/deepseek")
        .join(DEEPSEEK_RELEASE_VERSION);
    install_fixture(&dsh_home, &BTreeMap::new()).expect("installed package fixture");
    fs::write(
        dsh_home.join("package.json"),
        serde_json::to_vec(&json!({
            "name": "legacy-deepseek-runtime",
            "private": true,
            "dependencies": {
                "@deepseek-ai/dsh": DEEPSEEK_RELEASE_VERSION,
                DEEPSEEK_ACP_PACKAGE: DEEPSEEK_ACP_VERSION,
                DEEPSEEK_MCP_CLIENT_PACKAGE: DEEPSEEK_MCP_CLIENT_VERSION,
                DEEPSEEK_ACP_APP_PACKAGE: DEEPSEEK_ACP_APP_VERSION,
                DEEPSEEK_LLM_ADAPTER_PACKAGE: DEEPSEEK_LLM_ADAPTER_VERSION,
            }
        }))
        .expect("legacy root manifest"),
    )
    .expect("write legacy root manifest");
    let profile = dsh_home.join("profiles/cccc-acp");
    write_profile_files(&profile, &cccc_executable(temp.path())).expect("managed profile");
    fs::write(profile.join("cordis.patch.yml"), "- insert: []\n").expect("legacy patch");

    let installs = std::cell::Cell::new(0_u32);
    let ready = |_command: &[String], env: &BTreeMap<String, String>| {
        let root = PathBuf::from(env.get("DSH_HOME").ok_or("missing DSH_HOME")?);
        let manifest: Value = serde_json::from_slice(
            &fs::read(root.join("package.json")).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        let dependencies = manifest
            .get("dependencies")
            .and_then(Value::as_object)
            .ok_or("missing root dependencies")?;
        if dependencies.len() != required_packages().len()
            || required_packages().iter().any(|(package, version)| {
                dependencies.get(*package).and_then(Value::as_str) != Some(*version)
            })
        {
            return Err("legacy root dependency set".into());
        }
        if profile.join("cordis.patch.yml").exists() {
            return Err("obsolete profile patch remains".into());
        }
        fixture_ready(_command, env)
    };
    let mut env = test_env(temp.path());
    ensure_with(
        &home,
        &mut env,
        &cccc_executable(temp.path()),
        |root, env| {
            installs.set(installs.get() + 1);
            install_fixture(root, env)?;
            Ok(())
        },
        |_command, _env| Ok(()),
        ready,
    )
    .expect("legacy runtime migration");

    assert_eq!(installs.get(), 1);
    assert!(!profile.join("cordis.patch.yml").exists());
}
