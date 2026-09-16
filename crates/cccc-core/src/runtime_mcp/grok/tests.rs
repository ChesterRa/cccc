use super::*;
use serde_json::json;

const CURRENT: &str = "[mcp_servers.cccc]\ncommand='${CCCC_CLI:-cccc}'\nargs=['mcp']\n";
const UNRELATED: &str = "[ui]\nscreen_mode='minimal'\n[mcp_servers.other]\ncommand='other-tool'\n";

fn report(path: &Path, target: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({"mcpServers":[{
        "name":"cccc","transport":"stdio","target":target,
        "source":{"type":"configToml","path":path}
    }]}))
    .expect("report")
}

fn native_report(args: &[&str], path: &Path, target: &str) -> Vec<u8> {
    match args {
        ["inspect", "--json"] => report(path, target),
        ["mcp", "list", "--json"] => {
            let mut entry = effective_entry();
            entry["command"] = json!(target);
            serde_json::to_vec(&json!([entry])).expect("effective report")
        }
        _ => panic!("unexpected native command: {args:?}"),
    }
}

#[test]
fn repairs_only_native_registration_then_reads_it_without_rewriting() {
    let temp = tempfile::tempdir().expect("tempdir");
    let config = temp.path().join("config.toml");
    std::fs::write(&config, UNRELATED).expect("config");
    let cli = Path::new("/current/cccc");
    let mut additions = 0;
    for _ in 0..2 {
        let path = ensure_entry(&config, cli, |args| {
            if args.get(1) == Some(&"add") {
                assert_eq!(
                    args,
                    [
                        "mcp",
                        "add",
                        "--scope",
                        "user",
                        "cccc",
                        "--",
                        MCP_COMMAND,
                        "mcp"
                    ]
                );
                additions += 1;
                std::fs::write(&config, format!("{UNRELATED}{CURRENT}"))?;
                return Ok(Vec::new());
            }
            if read_entry(&config)?.is_some() {
                Ok(native_report(args, &config, "/current/cccc"))
            } else {
                Ok(serde_json::to_vec(&json!({"mcpServers":[{
                    "name":"cccc","target":"legacy-python",
                    "source":{"type":"claudeJson","path":"untouched-claude.json"}
                }]}))
                .expect("report"))
            }
        })
        .expect("configured");
        assert_eq!(path, config);
    }
    assert_eq!(additions, 1);
    assert!(
        std::fs::read_to_string(config)
            .expect("config")
            .starts_with(UNRELATED)
    );
}

#[test]
fn shared_registration_uses_each_instances_executable_without_pinning_identity() {
    let temp = tempfile::tempdir().expect("tempdir");
    let config = temp.path().join("config.toml");
    std::fs::write(&config, CURRENT).expect("config");
    for cli in ["/instance-a/cccc", "/instance-b/cccc"] {
        ensure_entry(&config, Path::new(cli), |args| {
            Ok(native_report(args, &config, cli))
        })
        .expect("same global configuration serves both instances");
    }
    assert_eq!(std::fs::read_to_string(config).expect("config"), CURRENT);
}

#[test]
fn malformed_configuration_never_reaches_the_native_writer_or_error_output() {
    let temp = tempfile::tempdir().expect("tempdir");
    let config = temp.path().join("config.toml");
    for raw in [
        "private-fixture-token =",
        "mcp_servers=3",
        "[mcp_servers]\ncccc=42",
    ] {
        std::fs::write(&config, raw).expect("config");
        let error = ensure_entry(&config, Path::new("/current/cccc"), |_| {
            panic!("malformed input must not reach native configuration commands")
        })
        .expect_err("invalid config");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(!error.to_string().contains("private-fixture-token"));
        assert_eq!(
            std::fs::read_to_string(&config).expect("unchanged config"),
            raw
        );
    }
}

#[test]
fn accepts_matching_project_entry_but_never_repairs_a_conflicting_project() {
    let temp = tempfile::tempdir().expect("tempdir");
    let user = temp.path().join("user.toml");
    let project = temp.path().join("project.toml");
    std::fs::write(&user, UNRELATED).expect("user config");
    std::fs::write(&project, CURRENT).expect("project config");
    let inspect = |args: &[&str]| Ok(native_report(args, &project, "/current/cccc"));
    assert_eq!(
        ensure_entry(&user, Path::new("/current/cccc"), inspect).expect("project entry"),
        project
    );
    let stale = "[mcp_servers.cccc]\ncommand='/old/python/cccc'\nargs=['mcp']\n";
    std::fs::write(&project, stale).expect("stale project");
    let error =
        ensure_entry(&user, Path::new("/current/cccc"), inspect).expect_err("scope conflict");
    assert!(error.to_string().contains("project MCP entry"));
    assert_eq!(std::fs::read_to_string(&project).expect("project"), stale);
    assert_eq!(std::fs::read_to_string(&user).expect("user"), UNRELATED);
}

#[test]
fn refuses_unavailable_configured_entry_without_repeatedly_rewriting_it() {
    let temp = tempfile::tempdir().expect("tempdir");
    let config = temp.path().join("config.toml");
    std::fs::write(&config, CURRENT).expect("config");
    let error = ensure_entry(&config, Path::new("/current/cccc"), |args| {
        assert_eq!(args, ["inspect", "--json"]);
        Ok(br#"{"mcpServers":[]}"#.to_vec())
    })
    .expect_err("hidden by policy or project");
    assert!(error.to_string().contains("disabled servers"));
    assert_eq!(std::fs::read_to_string(config).expect("config"), CURRENT);
}

#[test]
fn validates_native_inspection_and_post_write_visibility() {
    let temp = tempfile::tempdir().expect("tempdir");
    let config = temp.path().join("config.toml");
    let error = ensure_entry(&config, Path::new("/current/cccc"), |_| Ok(b"{}".to_vec()))
        .expect_err("invalid report is not an empty catalog");
    assert!(
        error
            .to_string()
            .contains("invalid MCP configuration report")
    );
    let mut additions = 0;
    let error = ensure_entry(&config, Path::new("/current/cccc"), |args| {
        if args[0] == "mcp" {
            additions += 1;
            std::fs::write(&config, CURRENT)?;
        }
        Ok(br#"{"mcpServers":[]}"#.to_vec())
    })
    .expect_err("native command success is not effective configuration");
    assert_eq!(additions, 1);
    assert!(error.to_string().contains("did not expose"));
}

#[test]
fn rejects_saved_identity_and_fixed_installations_as_ready_registrations() {
    let table =
        |source: &str| source.parse::<toml::Table>().expect("TOML")["mcp_servers"]["cccc"].clone();
    assert!(matches(&table(CURRENT)));
    for extra in [
        "env={CCCC_HOME='/old'}",
        "env={CCCC_MCP_TOOL_PROFILE='full'}",
        "env={Path='/old'}",
        "enabled=false",
        "url='https://example.test/mcp'",
    ] {
        assert!(!matches(&table(&format!("{CURRENT}{extra}\n"))), "{extra}");
    }
    assert!(!matches(&table(
        &CURRENT.replace(MCP_COMMAND, "/fixed/cccc")
    )));
    assert!(matches(&table(&format!("{CURRENT}env={{EXTRA='kept'}}"))));
}

#[test]
fn honors_explicit_and_relative_grok_home() {
    let env = BTreeMap::from([("GROK_HOME".into(), "provider-home".into())]);
    assert_eq!(
        config_path(Path::new("/workspace"), &env).expect("config"),
        Path::new("/workspace/provider-home/config.toml")
    );
}

fn effective_entry() -> Value {
    json!({"name":"cccc","command":"/current/cccc","args":["mcp"],"enabled":true,"scope":"user"})
}

#[test]
fn rejects_effective_argument_and_identity_overrides_without_rewriting_configuration() {
    let temp = tempfile::tempdir().expect("tempdir");
    let config = temp.path().join("config.toml");
    for (field, value) in [
        ("args", json!(["--help"])),
        ("env", json!({"CCCC_ACTOR_ID":"private-fixture-value"})),
        ("env", json!({"CCCC_HOME":"/other-instance"})),
        ("env", json!({"CCCC_MCP_TOOL_PROFILE":"full"})),
        ("env", json!({"CCCC_VOICE_ORIGIN":"other-call"})),
        ("env", json!({"PATH":"/old-cli"})),
        ("command", json!("/other/cccc")),
        ("enabled", json!(false)),
    ] {
        std::fs::write(&config, CURRENT).expect("base config");
        let mut effective = effective_entry();
        effective[field] = value;
        let error = ensure_entry(&config, Path::new("/current/cccc"), |args| match args {
            ["inspect", "--json"] => Ok(report(&config, "/current/cccc")),
            ["mcp", "list", "--json"] => {
                Ok(serde_json::to_vec(&json!([effective])).expect("effective report"))
            }
            _ => panic!("effective overrides must not trigger a config rewrite"),
        })
        .expect_err("effective entry contradicts the ready base table");
        assert!(error.to_string().contains("effective MCP"));
        assert!(!error.to_string().contains("private-fixture-value"));
        assert_eq!(
            std::fs::read_to_string(&config).expect("preserved config"),
            CURRENT
        );
    }
}

#[test]
fn accepts_native_resolution_of_inactive_overrides_and_unrelated_environment() {
    let temp = tempfile::tempdir().expect("tempdir");
    let config = temp.path().join("config.toml");
    let raw = format!(
        "{CURRENT}\n[[version_overrides]]\nminimum_version='99.0.0'\n[version_overrides.mcp_servers.cccc]\nargs=['--help']\n"
    );
    std::fs::write(&config, &raw).expect("future override");
    let mut effective = effective_entry();
    effective["env"] = json!({"UNRELATED":"preserved"});
    let mut resolved = false;
    ensure_entry(&config, Path::new("/current/cccc"), |args| match args {
        ["inspect", "--json"] => Ok(report(&config, "/current/cccc")),
        ["mcp", "list", "--json"] => {
            resolved = true;
            Ok(serde_json::to_vec(&json!([effective])).expect("effective report"))
        }
        _ => panic!("compatible configuration must not be rewritten"),
    })
    .expect("native resolution is authoritative");
    assert!(resolved, "base table alone is insufficient");
    assert_eq!(
        std::fs::read_to_string(config).expect("preserved config"),
        raw
    );
}

#[test]
fn rejects_policy_denial_from_either_native_report_without_echoing_it() {
    let temp = tempfile::tempdir().expect("tempdir");
    let config = temp.path().join("config.toml");
    std::fs::write(&config, CURRENT).expect("config");
    for discovery_blocked in [true, false] {
        let mut inspected: Value =
            serde_json::from_slice(&report(&config, "/current/cccc")).expect("inspect report");
        let mut effective = effective_entry();
        if discovery_blocked {
            inspected["mcpServers"][0]["disabledReason"] = json!("private-fixture-value");
        } else {
            effective["blocked_reason"] = json!("private-fixture-value");
        }
        let error = ensure_entry(&config, Path::new("/current/cccc"), |args| match args {
            ["inspect", "--json"] => Ok(serde_json::to_vec(&inspected).expect("inspect report")),
            ["mcp", "list", "--json"] => {
                assert!(
                    !discovery_blocked,
                    "discovery denial should stop immediately"
                );
                Ok(serde_json::to_vec(&json!([effective])).expect("effective report"))
            }
            _ => panic!("native policy must not trigger a config rewrite"),
        })
        .expect_err("native policy blocks the registered server");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert!(!error.to_string().contains("private-fixture-value"));
    }
    assert_eq!(
        std::fs::read_to_string(config).expect("preserved config"),
        CURRENT
    );
}

#[test]
fn refuses_missing_or_invalid_effective_reports() {
    let temp = tempfile::tempdir().expect("tempdir");
    let config = temp.path().join("config.toml");
    std::fs::write(&config, CURRENT).expect("config");
    for effective in [
        json!({}),
        json!([]),
        json!([{"name":"other"}]),
        json!([{"name":"cccc"}]),
    ] {
        ensure_entry(&config, Path::new("/current/cccc"), |args| match args {
            ["inspect", "--json"] => Ok(report(&config, "/current/cccc")),
            ["mcp", "list", "--json"] => Ok(serde_json::to_vec(&effective).expect("report")),
            _ => panic!("invalid report must not trigger a config rewrite"),
        })
        .expect_err("discovery alone does not establish readiness");
    }
}
