#![cfg(target_os = "linux")]
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn pi_native_receipts_require_exact_native_input_evidence() {
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("skipping Pi extension fixture: Node is unavailable");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let result = Command::new("node")
        .arg(root.join("tests/fixtures/pi_native.mjs"))
        .arg(root.join("resources/native/pi.ts"))
        .output()
        .expect("isolated Pi extension fixture");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn opencode_native_plugin_uses_current_sdk_results_and_fenced_queue_admission() {
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("skipping OpenCode plugin fixture: Node is unavailable");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let result = Command::new("node")
        .arg(root.join("tests/fixtures/opencode_native.mjs"))
        .arg(root.join("resources/native/opencode-tui.ts"))
        .output()
        .expect("isolated plugin fixture");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn pi_native_setup_preserves_actor_scoped_mcp_configuration() {
    let temp = tempfile::tempdir().expect("isolated Home");
    let root = temp.path();
    let bin = root.join("bin");
    let provider = root.join("pi");
    std::fs::create_dir_all(&bin).expect("bin");
    std::fs::create_dir_all(&provider).expect("provider");
    let cli = bin.join("pi");
    std::fs::write(
        &cli,
        "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 1.4.0; exit 0; fi\nexit 91\n",
    )
    .expect("native CLI fixture");
    std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o700)).expect("mode");
    let expected = json!({"mcpServers":{"cccc":{
        "command":env!("CARGO_BIN_EXE_cccc"),"args":["mcp"],
        "env":{"CCCC_HOME":root.join("cccc"),"CCCC_ACTOR_ID":"fixture-managed-actor"}
    }}});
    let path = provider.join("mcp.json");
    let original = serde_json::to_vec(&expected).expect("fixture config");
    std::fs::write(&path, &original).expect("existing scoped MCP");
    let result = Command::new(env!("CARGO_BIN_EXE_cccc"))
        .args(["sessions", "enable", "--runtime", "pi"])
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("HOME", root)
        .env("CCCC_HOME", root.join("cccc"))
        .env("CODEX_HOME", root.join("codex"))
        .env("PI_CODING_AGENT_DIR", &provider)
        .current_dir(root)
        .output()
        .expect("native setup");
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("scoped identity"));
    assert_eq!(std::fs::read(path).expect("preserved MCP"), original);
    assert!(!root.join("cccc/integrations/pi").exists());
}

#[test]
fn failed_native_mcp_install_does_not_enable_receive_hooks_or_plugins() {
    for runtime in ["codex", "claude"] {
        let temp = tempfile::tempdir().expect("isolated installation");
        let directory = temp.path();
        let bin = directory.join("bin");
        let provider = directory.join("provider");
        std::fs::create_dir(&bin).expect("bin");
        std::fs::create_dir(&provider).expect("provider state");
        let original = r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"true","statusMessage":"unrelated"}]}]}}"#;
        std::fs::write(provider.join("hooks.json"), original).expect("unrelated hooks");
        let program = bin.join(runtime);
        std::fs::write(&program, format!("#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo '{}'; exit 0; fi\nif [ \"$1\" = plugin ]; then echo '[]'; exit 0; fi\nprintf '%s\\n' \"$1 $2\" >> \"$HOME/native-calls\"\nexit 1\n", if runtime=="codex" {"codex-cli 0.163.0"} else {"2.1.306 (Claude Code)"})).expect("native CLI fixture");
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700))
            .expect("executable fixture");
        let output = Command::new(env!("CARGO_BIN_EXE_cccc"))
            .args(["sessions", "enable", "--runtime", runtime])
            .env_clear()
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .env("HOME", directory)
            .env("USERPROFILE", directory)
            .env("CODEX_HOME", &provider)
            .env("CLAUDE_CONFIG_DIR", &provider)
            .env("CCCC_HOME", directory.join("cccc"))
            .current_dir(directory)
            .output()
            .expect("enable fixture");
        assert!(!output.status.success());
        assert_eq!(
            std::fs::read_to_string(provider.join("hooks.json")).expect("hooks"),
            original
        );
        assert_eq!(
            std::fs::read_to_string(directory.join("native-calls"))
                .expect("native calls")
                .trim(),
            if runtime == "codex" {
                "mcp add"
            } else {
                "mcp add-json"
            }
        );
        assert!(
            !directory.join("cccc/integrations/local-sessions").exists(),
            "receiving must not activate after failed identity setup"
        );
    }
}

fn install_fake_claude(fixture: &CodexSetupFixture) {
    let program = fixture.bin.join("claude");
    std::fs::write(&program, r#"#!/usr/bin/python3
import json, os, pathlib, sys
root = pathlib.Path(os.environ['HOME']).parent
path = pathlib.Path(os.environ.get('FIXTURE_CLAUDE_CONFIG_FILE', str(root / 'claude' / '.claude.json')))
assert path.is_relative_to(root)
args = sys.argv[1:]
with (root / 'claude-calls.jsonl').open('a') as log:
    log.write(json.dumps(args) + '\n')
if args == ['--version']:
    print('2.1.306 (Claude Code)')
    sys.exit(0)
if args[:1] == ['plugin']:
    if 'list' in args and os.environ.get('FIXTURE_PLUGIN_READ'):
        mode = os.environ['FIXTURE_PLUGIN_READ']
        print('[]' if mode == 'unavailable' else 'SYNTHETIC_PROVIDER_PRIVATE_DETAIL')
        sys.exit(1 if mode == 'unavailable' else 0)
    registry = root / 'claude-plugins.json'
    state = json.loads(registry.read_text()) if registry.exists() else {}
    if args == ['plugin', 'marketplace', 'list', '--json']:
        print(json.dumps([{'name':'cccc-local','source':'directory','path':state['marketplace'],
                          'installLocation':state['marketplace']}] if 'marketplace' in state else []))
        sys.exit(0)
    if args == ['plugin', 'list', '--json']:
        plugins = []
        if 'installed' in state:
            entry = {'id':'cccc-local-sessions@cccc-local','scope':'user','version':'0.4.45',
                     'enabled':state.get('enabled',False),'installPath':state['installed']}
            if not state.get('cached'):
                entry['readFromFolder'] = state['installed']
            plugins.append(entry)
        print(json.dumps(plugins))
        sys.exit(0)
    assert args[-1] == '--json'
    command = args[1]
    outcome = 'ok'
    extra = {}
    if args[1:3] == ['marketplace','add']:
        assert 'marketplace' not in state
        state['marketplace'] = args[3]
    elif command == 'install':
        state.setdefault('installed', str(pathlib.Path(state['marketplace']) / 'plugins/cccc-local-sessions'))
        state.setdefault('enabled', True)
    elif command in ('enable','disable'):
        wanted = command == 'enable'
        if os.environ.get('FIXTURE_PLUGIN_FAILURE') == command:
            outcome = 'failed'
            extra = {'failureCode':'policy_blocked'}
        elif state.get('enabled',False) == wanted:
            outcome = 'failed'
            extra = {'failureCode':'already_in_goal_state','alreadyInGoalState':True}
        else:
            state['enabled'] = wanted
    else:
        sys.exit(93)
    registry.write_text(json.dumps(state))
    print(json.dumps(dict(command=command,outcome=outcome,message='SYNTHETIC_PROVIDER_PRIVATE_DETAIL',**extra)))
    sys.exit(0 if outcome == 'ok' else 1)
document = json.loads(path.read_text()) if path.exists() else {}
servers = document.setdefault('mcpServers', {})
if args[:5] == ['mcp', 'add-json', '--scope', 'user', 'cccc']:
    if 'cccc' in servers:
        print('MCP server cccc already exists in user config', file=sys.stderr)
        sys.exit(1)
    servers['cccc'] = json.loads(args[5])
elif args == ['mcp', 'remove', '--scope', 'user', 'cccc']:
    mode = os.environ.get('FIXTURE_CLAUDE_REMOVE', '')
    if mode == 'unchanged':
        print('SYNTHETIC_PROVIDER_PRIVATE_DETAIL', file=sys.stderr)
        sys.exit(1)
    if mode == 'changed':
        servers['cccc'] = {'type':'stdio', 'command':'changed-by-user', 'args':[]}
    else:
        servers.pop('cccc', None)
    path.write_text(json.dumps(document))
    sys.exit(1 if mode == 'removed-with-error' else 0)
else:
    print('unexpected or non-passive Claude command', file=sys.stderr)
    sys.exit(93)
path.parent.mkdir(parents=True, exist_ok=True)
path.write_text(json.dumps(document))
"#).expect("fake Claude");
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700))
        .expect("executable fake Claude");
}

#[test]
fn claude_disable_removes_only_its_native_mcp_without_installing_ordinary_mcp() {
    for case in [
        "owned",
        "absent",
        "ordinary",
        "other-home",
        "other-command",
        "extra-env",
        "unchanged",
        "removed-with-error",
        "changed",
    ] {
        let fixture = CodexSetupFixture::new("overwrite");
        install_fake_claude(&fixture);
        let home = fixture.home("native-home");
        fixture.run(&["sessions", "enable", "--runtime", "claude"], &home);
        let path = fixture.home("claude/.claude.json");
        let mut document: Value = cccc_core::fs::read_json(&path).expect("native registration");
        let unrelated = json!({"type":"stdio","command":"unrelated-server","args":["preserve"],"env":{"fixture_private":"SYNTHETIC_PROVIDER_PRIVATE_DETAIL"}});
        document["mcpServers"]["unrelated"] = unrelated.clone();
        match case {
            "absent" => {
                document["mcpServers"]
                    .as_object_mut()
                    .expect("servers")
                    .remove("cccc");
            }
            "ordinary" => document["mcpServers"]["cccc"]["args"] = json!(["mcp"]),
            "other-home" => {
                document["mcpServers"]["cccc"]["env"]["CCCC_HOME"] =
                    json!(fixture.home("another-instance"))
            }
            "other-command" => {
                document["mcpServers"]["cccc"]["command"] = json!("some-other-server")
            }
            "extra-env" => document["mcpServers"]["cccc"]["env"]["CUSTOM"] = json!("preserve"),
            _ => {}
        }
        cccc_core::fs::write_json(&path, &document).expect("fixture config");
        let output = fixture
            .command(&["sessions", "disable", "--runtime", "claude"], &home)
            .env("FIXTURE_CLAUDE_REMOVE", case)
            .output()
            .expect("disable Claude");
        assert_eq!(
            output.status.success(),
            !matches!(case, "unchanged" | "changed"),
            "{case}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let after: Value = cccc_core::fs::read_json(&path).expect("retained user configuration");
        assert_eq!(after["mcpServers"]["unrelated"], unrelated, "{case}");
        if matches!(case, "owned" | "absent" | "removed-with-error") {
            assert!(
                after["mcpServers"].get("cccc").is_none(),
                "{case}: disable must not install an ordinary global MCP"
            );
        } else if case == "changed" {
            assert_eq!(after["mcpServers"]["cccc"]["command"], "changed-by-user");
        } else {
            assert_eq!(
                after, document,
                "{case}: do not modify an unowned or unremoved entry"
            );
        }
        let calls = std::fs::read_to_string(fixture.home("claude-calls.jsonl"))
            .expect("CLI calls")
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("call"))
            .collect::<Vec<_>>();
        assert_eq!(
            calls.iter().filter(|args| args[1] == "add-json").count(),
            1,
            "enable is the only registration, {case}"
        );
        assert_eq!(
            calls.iter().filter(|args| args[1] == "remove").count(),
            usize::from(matches!(
                case,
                "owned" | "unchanged" | "removed-with-error" | "changed"
            )),
            "{case}"
        );
        assert!(
            !String::from_utf8_lossy(&output.stdout).contains("SYNTHETIC_PROVIDER_PRIVATE_DETAIL")
        );
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains("SYNTHETIC_PROVIDER_PRIVATE_DETAIL")
        );
        let layout = cccc_core::HomeLayout::from_path(&home).expect("home");
        assert!(
            cccc_core::local_sessions::integration_disabled(
                &layout,
                cccc_contracts::local_session::LocalSessionRuntime::Claude
            )
            .expect("disabled even if provider cleanup fails")
        );
    }
}

#[test]
fn native_cli_send_keeps_direct_and_group_addressing_separate() {
    let fixture = CodexSetupFixture::new("overwrite");
    let home = fixture.home("conversation-home");
    fixture.hook(
        &home,
        "codex",
        std::process::id(),
        "native-fixture",
        "SessionStart",
    );
    let _daemon = fixture.daemon(&home);
    let direct = fixture.run(
        &[
            "sessions",
            "send",
            "--to",
            "codex:native-fixture",
            "--text",
            "Direct fixture",
        ],
        &home,
    );
    let group_id = direct["group_id"].as_str().expect("direct Group");
    assert_eq!(
        direct["event"]["data"]["to"]
            .as_array()
            .expect("recipients")
            .len(),
        1
    );
    assert_ne!(direct["event"]["data"]["to"], json!(["@all"]));
    let group = fixture.run(
        &[
            "sessions",
            "send",
            "--group",
            group_id,
            "--text",
            "Group fixture",
        ],
        &home,
    );
    assert_eq!(group["group_id"], group_id);
    assert_eq!(group["event"]["data"]["to"], json!(["@all"]));
}

#[test]
fn claude_native_configuration_is_idempotent_and_keeps_real_failures_visible() {
    let fixture = CodexSetupFixture::new("overwrite");
    install_fake_claude(&fixture);
    let home = fixture.home("native-home");
    for action in ["enable", "enable", "disable", "disable", "enable"] {
        fixture.run(&["sessions", action, "--runtime", "claude"], &home);
    }
    let output = fixture
        .command(&["sessions", "disable", "--runtime", "claude"], &home)
        .env("FIXTURE_PLUGIN_FAILURE", "disable")
        .output()
        .expect("genuine plugin rejection");
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("SYNTHETIC_PROVIDER_PRIVATE_DETAIL"));
    assert!(
        cccc_core::local_sessions::integration_disabled(
            &cccc_core::HomeLayout::from_path(&home).expect("home"),
            cccc_contracts::local_session::LocalSessionRuntime::Claude,
        )
        .expect("closed fence")
    );
    fixture.run(&["sessions", "disable", "--runtime", "claude"], &home);
    assert!(
        cccc_core::fs::read_json::<Value>(&fixture.home("claude/.claude.json"))
            .expect("user config")["mcpServers"]
            .get("cccc")
            .is_none()
    );
}

#[test]
fn disabling_another_instance_preserves_provider_registrations_and_hooks() {
    for runtime in ["codex", "claude"] {
        let fixture = CodexSetupFixture::new("overwrite");
        install_fake_claude(&fixture);
        let active = fixture.home("active-instance");
        fixture.run(&["sessions", "enable", "--runtime", runtime], &active);
        let codex = fixture.registry();
        let hooks = fixture.hooks();
        let claude = std::fs::read(fixture.home("claude/.claude.json")).ok();
        let plugins = std::fs::read(fixture.home("claude-plugins.json")).ok();
        fixture.run(
            &["sessions", "disable", "--runtime", runtime],
            &fixture.home("unrelated-instance"),
        );
        assert_eq!(fixture.registry(), codex, "{runtime}");
        assert_eq!(fixture.hooks(), hooks, "{runtime}");
        assert_eq!(
            std::fs::read(fixture.home("claude/.claude.json")).ok(),
            claude,
            "{runtime}"
        );
        assert_eq!(
            std::fs::read(fixture.home("claude-plugins.json")).ok(),
            plugins,
            "{runtime}"
        );
    }
}

#[test]
fn codex_disable_preserves_edited_registrations_and_same_label_hooks() {
    for changed in [
        "command", "args", "env", "env_vars", "cwd", "disabled", "tools", "timeout",
    ] {
        let fixture = CodexSetupFixture::new("overwrite");
        let home = fixture.home("native-home");
        fixture.run(&["sessions", "enable", "--runtime", "codex"], &home);
        let mut registry = fixture.registry();
        let transport = &mut registry["cccc"]["transport"];
        match changed {
            "command" => transport["command"] = json!("custom-wrapper"),
            "args" => {
                transport["args"] = json!(["mcp", "--local-session-runtime", "codex", "--custom"])
            }
            "env" => transport["env"]["EXTRA"] = json!("SYNTHETIC_PROVIDER_PRIVATE_DETAIL"),
            "env_vars" => transport["env_vars"] = json!(["CUSTOM_TOKEN"]),
            "cwd" => transport["cwd"] = json!(fixture.home("custom-directory")),
            "disabled" => registry["cccc"]["enabled"] = json!(false),
            "tools" => registry["cccc"]["enabled_tools"] = json!(["cccc_help"]),
            "timeout" => registry["cccc"]["tool_timeout_sec"] = json!(120),
            _ => unreachable!(),
        }
        fixture.write_registry(&registry);
        let mut hooks: Value = serde_json::from_slice(&fixture.hooks()).expect("hooks");
        let custom = json!({"type":"command","command":"custom-wrapper","statusMessage":"CCCC local session connection"});
        hooks["hooks"]["SessionStart"]
            .as_array_mut()
            .expect("entries")
            .push(json!({"hooks":[custom]}));
        cccc_core::fs::write_json(&fixture.provider.join("hooks.json"), &hooks)
            .expect("edited hooks");
        fixture.run(&["sessions", "disable", "--runtime", "codex"], &home);
        assert_eq!(fixture.registry(), registry, "{changed}");
        let after: Value = serde_json::from_slice(&fixture.hooks()).expect("hooks");
        assert!(
            after["hooks"]["SessionStart"]
                .as_array()
                .expect("entries")
                .iter()
                .any(|entry| entry["hooks"][0] == custom)
        );
        assert!(
            !String::from_utf8(fixture.hooks())
                .expect("hooks")
                .contains("--provider-home")
        );
    }
}

#[test]
fn claude_foreign_marketplace_or_installed_plugin_is_rejected_before_mcp_changes() {
    for case in ["marketplace", "plugin", "cached"] {
        let fixture = CodexSetupFixture::new("overwrite");
        install_fake_claude(&fixture);
        let original = fixture.home("original-instance");
        fixture.run(&["sessions", "enable", "--runtime", "claude"], &original);
        fixture.run(&["sessions", "disable", "--runtime", "claude"], &original);
        let path = fixture.home("claude-plugins.json");
        let mut registry: Value = cccc_core::fs::read_json(&path).expect("plugins");
        let target = if case == "marketplace" {
            fixture.home("new-instance")
        } else {
            original.clone()
        };
        if case != "marketplace" {
            registry["installed"] = json!(fixture.home("stale-installed-plugin"));
            if case == "cached" {
                registry["cached"] = json!(true);
            }
            cccc_core::fs::write_json(&path, &registry).expect("stale installed plugin");
        }
        let before = std::fs::read(fixture.home("claude/.claude.json")).expect("MCP");
        let output = fixture
            .command(&["sessions", "enable", "--runtime", "claude"], &target)
            .output()
            .expect("conflicting setup");
        assert!(!output.status.success(), "{case}");
        assert_eq!(
            std::fs::read(fixture.home("claude/.claude.json")).expect("unchanged MCP"),
            before,
            "{case}"
        );
        assert_eq!(
            cccc_core::fs::read_json::<Value>(&path).expect("unchanged plugins"),
            registry,
            "{case}"
        );
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains("SYNTHETIC_PROVIDER_PRIVATE_DETAIL")
        );
    }
}

#[test]
fn claude_cached_plugin_ownership_uses_its_actual_hook_and_monitor_configuration() {
    let fixture = CodexSetupFixture::new("overwrite");
    install_fake_claude(&fixture);
    let home = fixture.home("native-home");
    fixture.run(&["sessions", "enable", "--runtime", "claude"], &home);
    let plugin = home.join("integrations/local-sessions/plugins/cccc-local-sessions");
    let cache = fixture.home("cached-plugin");
    for file in ["hooks/hooks.json", "monitors/monitors.json"] {
        let target = cache.join(file);
        std::fs::create_dir_all(target.parent().expect("parent")).expect("cache directory");
        std::fs::copy(plugin.join(file), target).expect("cached plugin");
    }
    let path = fixture.home("claude-plugins.json");
    let mut state: Value = cccc_core::fs::read_json(&path).expect("state");
    state["installed"] = json!(cache);
    state["cached"] = json!(true);
    cccc_core::fs::write_json(&path, &state).expect("cache registration");
    fixture.run(&["sessions", "enable", "--runtime", "claude"], &home);
    fixture.run(&["sessions", "disable", "--runtime", "claude"], &home);
    assert_eq!(
        cccc_core::fs::read_json::<Value>(&path).expect("state")["enabled"],
        false
    );
}

#[test]
fn unreadable_claude_registry_does_not_modify_provider_configuration() {
    for mode in ["unavailable", "malformed"] {
        let fixture = CodexSetupFixture::new("overwrite");
        install_fake_claude(&fixture);
        let home = fixture.home("native-home");
        fixture.run(&["sessions", "enable", "--runtime", "claude"], &home);
        let before = std::fs::read(fixture.home("claude/.claude.json")).expect("MCP");
        let plugins = std::fs::read(fixture.home("claude-plugins.json")).expect("plugins");
        for action in ["enable", "disable"] {
            let output = fixture
                .command(&["sessions", action, "--runtime", "claude"], &home)
                .env("FIXTURE_PLUGIN_READ", mode)
                .output()
                .expect("unreadable registry");
            assert!(!output.status.success(), "{mode}, {action}");
            assert_eq!(
                std::fs::read(fixture.home("claude/.claude.json")).expect("MCP"),
                before
            );
            assert_eq!(
                std::fs::read(fixture.home("claude-plugins.json")).expect("plugins"),
                plugins
            );
            assert!(
                !String::from_utf8_lossy(&output.stderr)
                    .contains("SYNTHETIC_PROVIDER_PRIVATE_DETAIL")
            );
        }
    }
}

#[test]
fn claude_disable_uses_the_effective_user_file_and_rejects_unreadable_configuration() {
    for case in [
        "modern",
        "default",
        "legacy",
        "custom",
        "relative",
        "empty",
        "invalid-json",
        "invalid-servers",
    ] {
        let fixture = CodexSetupFixture::new("overwrite");
        install_fake_claude(&fixture);
        let home = fixture.home("native-home");
        let path = fixture.home(match case {
            "default" => "user/.claude.json",
            "legacy" => "claude/.config.json",
            "custom" => "claude/.claude-custom-oauth.json",
            _ => "claude/.claude.json",
        });
        let command = |args: &[&str]| {
            let mut command = fixture.command(args, &home);
            command.env("FIXTURE_CLAUDE_CONFIG_FILE", &path);
            match case {
                "default" => {
                    command.env_remove("CLAUDE_CONFIG_DIR");
                }
                "custom" => {
                    command.env("CLAUDE_CODE_CUSTOM_OAUTH_URL", "https://invalid.test");
                }
                "relative" => {
                    command.env("CLAUDE_CONFIG_DIR", "claude");
                }
                "empty" => {
                    command.env("CLAUDE_CONFIG_DIR", "");
                }
                _ => {}
            }
            command
        };
        let enabled = command(&["sessions", "enable", "--runtime", "claude"])
            .output()
            .expect("enable");
        assert!(
            enabled.status.success(),
            "{case}: {}",
            String::from_utf8_lossy(&enabled.stderr)
        );
        if case == "invalid-json" {
            std::fs::write(&path, b"{").expect("bad JSON");
        }
        if case == "invalid-servers" {
            std::fs::write(&path, b"{\"mcpServers\":[]}").expect("bad structure");
        }
        let before = std::fs::read(&path).expect("before");
        let disabled = command(&["sessions", "disable", "--runtime", "claude"])
            .output()
            .expect("disable");
        let unsupported = matches!(
            case,
            "relative" | "empty" | "invalid-json" | "invalid-servers"
        );
        assert_eq!(
            disabled.status.success(),
            !unsupported,
            "{case}: {}",
            String::from_utf8_lossy(&disabled.stderr)
        );
        if unsupported {
            assert_eq!(
                std::fs::read(&path).expect("untouched config"),
                before,
                "{case}"
            );
        } else {
            let after: Value = cccc_core::fs::read_json(&path).expect("after");
            assert!(after["mcpServers"].get("cccc").is_none(), "{case}");
        }
    }
}

#[test]
fn generated_codex_hook_registers_its_verified_parent_without_a_pid_argument() {
    let fixture = CodexSetupFixture::new("overwrite");
    let home = fixture.home("generated-codex");
    fixture.run(&["sessions", "enable", "--runtime", "codex"], &home);
    let hooks: Value = serde_json::from_slice(&fixture.hooks()).expect("generated hooks");
    let command = hooks["hooks"]["SessionStart"]
        .as_array()
        .expect("SessionStart")
        .iter()
        .flat_map(|entry| entry["hooks"].as_array().expect("hooks"))
        .find(|hook| hook["statusMessage"] == "CCCC local session connection")
        .expect("CCCC hook")["command"]
        .as_str()
        .expect("shell command");
    assert!(!command.contains("--host-pid"));
    let environment = fixture.command(&[], &home);
    let mut hook = Command::new("/bin/sh");
    hook.args(["-c", command])
        .env_clear()
        .envs(
            environment
                .get_envs()
                .map(|(key, value)| (key, value.expect("fixture environment"))),
        )
        .current_dir(fixture.directory.path());
    let callback = json!({"session_id":"native-fixture","hook_event_name":"SessionStart","cwd":fixture.directory.path()}).to_string();
    let output = cccc_runtime::capture_command_blocking(
        &mut hook,
        Some(callback.as_bytes()),
        std::time::Duration::from_secs(5),
        4096,
    )
    .expect("generated callback");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let layout = cccc_core::HomeLayout::from_path(&home).expect("home");
    let source =
        cccc_core::local_sessions::read(&layout, "codex:native-fixture").expect("callback source");
    assert_eq!(
        source.host,
        cccc_core::local_sessions::NativeHost::capture(std::process::id()).expect("host")
    );
    assert!(source.codex_can_receive);
}

#[test]
fn receiver_waits_out_a_previous_lease_instead_of_exiting_on_reload() {
    use cccc_core::local_sessions::NativeHost;
    use std::io::{BufRead, BufReader, Write};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    let fixture = CodexSetupFixture::new("overwrite");
    install_fake_claude(&fixture);
    let path = fixture.home("generated-claude");
    fixture.run(&["sessions", "enable", "--runtime", "claude"], &path);
    let home = cccc_core::HomeLayout::from_path(&path).expect("home");
    let plugin = path.join("integrations/local-sessions/plugins/cccc-local-sessions");
    let hooks: Value =
        cccc_core::fs::read_json(&plugin.join("hooks/hooks.json")).expect("generated hooks");
    let hook = &hooks["hooks"]["SessionStart"][0]["hooks"][0];
    assert_eq!(hook["command"], env!("CARGO_BIN_EXE_cccc"));
    let args = hook["args"]
        .as_array()
        .expect("exec-form hook")
        .iter()
        .map(|value| value.as_str().expect("argument"))
        .collect::<Vec<_>>();
    assert!(!args.contains(&"--host-pid"));
    let mut hook_command = fixture.command(&args, &path);
    hook_command.env("CLAUDE_PID", std::process::id().to_string());
    let callback = json!({"session_id":"fixture-context","hook_event_name":"SessionStart","cwd":fixture.directory.path()}).to_string();
    let output = cccc_runtime::capture_command_blocking(
        &mut hook_command,
        Some(callback.as_bytes()),
        Duration::from_secs(5),
        4096,
    )
    .expect("generated hook");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let host = NativeHost::capture(std::process::id()).expect("fixture host");
    let source_id = format!("claude:{}", host.key());
    let source =
        cccc_core::local_sessions::read(&home, &source_id).expect("generated hook host identity");
    assert_eq!(source.host, host);
    assert_eq!(source.native_session_id, "fixture-context");
    let mcp: Value =
        cccc_core::fs::read_json(&fixture.home("claude/.claude.json")).expect("MCP registration");
    fixture.assert_tool_contract(&json!({"transport":mcp["mcpServers"]["cccc"]}), &path, true);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("IPC listener");
    listener.set_nonblocking(true).expect("bounded server");
    cccc_core::fs::write_json(&home.daemon_dir().join("ccccd.addr.json"), &serde_json::json!({"v":1,"transport":"tcp","path":"","host":"127.0.0.1","port":listener.local_addr().expect("port").port(),"pid":std::process::id(),"version":env!("CARGO_PKG_VERSION"),"ts":"2026-10-21T00:00:00Z"})).expect("IPC address");
    let (sender, received) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(7);
        let mut attach_count = 0;
        let mut offered = false;
        while Instant::now() < deadline {
            let (stream, _) = match listener.accept() {
                Ok(value) => value,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(error) => panic!("accept: {error}"),
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .expect("read timeout");
            let mut stream = BufReader::new(stream);
            let mut line = String::new();
            stream.read_line(&mut line).expect("request");
            let request: cccc_contracts::DaemonRequest =
                serde_json::from_str(&line).expect("request JSON");
            let response = match request.op.as_str() {
                "local_session_receiver_attach" => {
                    attach_count += 1;
                    if attach_count == 1 {
                        serde_json::json!({"ok":false,"error":{"code":"local_session_receiver_conflict","message":"The previous receiver is retiring"},"result":{}})
                    } else {
                        serde_json::json!({"ok":true,"result":{}})
                    }
                }
                "local_session_receiver_next" if !offered => {
                    offered = true;
                    serde_json::json!({"ok":true,"result":{"handoff":{"handoff_id":"fixture-handoff","text":"SYNTHETIC_RECEIVER_RELOAD"}}})
                }
                "local_session_receiver_ack" => {
                    let _ = sender.send(request.args.clone());
                    serde_json::json!({"ok":true,"result":{}})
                }
                _ => serde_json::json!({"ok":true,"result":{"handoff":null}}),
            };
            writeln!(stream.get_mut(), "{response}").expect("response");
            if request.op == "local_session_receiver_detach" {
                break;
            }
        }
        attach_count
    });
    let monitors: Value = cccc_core::fs::read_json(&plugin.join("monitors/monitors.json"))
        .expect("generated monitor");
    let command = monitors[0]["command"].as_str().expect("monitor command");
    assert!(!command.contains("--host-pid"));
    let environment = fixture.command(&[], &path);
    let mut child = Command::new("/bin/sh")
        .args(["-c", command])
        .env_clear()
        .envs(
            environment
                .get_envs()
                .map(|(key, value)| (key, value.expect("fixture environment"))),
        )
        .env("CLAUDE_PID", std::process::id().to_string())
        .current_dir(fixture.directory.path())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("receiver");
    let acknowledgement = received.recv_timeout(Duration::from_secs(5));
    let _ = nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(child.id() as i32),
        nix::sys::signal::Signal::SIGTERM,
    );
    child.wait().expect("own receiver exits");
    let output = child.wait_with_output().expect("receiver output");
    assert_eq!(
        server.join().expect("IPC server"),
        2,
        "replacement receiver must retry registration"
    );
    assert_eq!(
        acknowledgement.expect("new receiver must deliver after conflict")["handoff_id"],
        "fixture-handoff"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).lines().count(),
        1,
        "one native notification, no polling chatter"
    );
}

struct CodexSetupFixture {
    directory: tempfile::TempDir,
    bin: PathBuf,
    provider: PathBuf,
}

impl CodexSetupFixture {
    fn new(policy: &str) -> Self {
        let directory = tempfile::tempdir().expect("isolated Codex setup");
        let bin = directory.path().join("bin");
        let provider = directory.path().join("codex");
        for path in [
            &bin,
            &provider,
            &directory.path().join("user"),
            &directory.path().join("claude"),
        ] {
            std::fs::create_dir(path).expect("isolated provider directories");
        }
        std::fs::write(provider.join("add-policy"), policy).expect("fake add policy");
        cccc_core::fs::write_json(
            &provider.join("registry.json"),
            &json!({"unrelated":{"command":"unrelated-fixture","args":["keep"]}}),
        )
        .expect("unrelated MCP server");
        cccc_core::fs::write_json(
            &provider.join("hooks.json"),
            &json!({"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"true","statusMessage":"unrelated"}]}]}}),
        )
        .expect("unrelated hooks");
        // Matches Codex 0.164's public `mcp list --json` shape, not its
        // private TOML layout. This fixture never invokes another provider CLI.
        let program = bin.join("codex");
        std::fs::write(
            &program,
            r#"#!/usr/bin/python3
import base64, hashlib, json, os, pathlib, struct, sys, time
home = pathlib.Path(os.environ['CODEX_HOME'])
root = home.parent
for key in ('HOME', 'CCCC_HOME', 'CODEX_HOME', 'CLAUDE_CONFIG_DIR'):
    assert pathlib.Path(os.environ[key]).resolve().is_relative_to(root.resolve())
path = home / 'registry.json'
servers = json.loads(path.read_text())
args = sys.argv[1:]

def policy(name):
    path = home / (name + '.json')
    return json.loads(path.read_text()) if path.exists() else {}

def record(method, **fields):
    with (home / 'native-calls.jsonl').open('a') as log:
        log.write(json.dumps(dict(method=method, **fields)) + '\n')

if args == ['app-server', 'proxy']:
    record('proxy', pid=os.getpid())
    if policy('capability').get('proxy') == 'closed':
        sys.exit(0)
    request = b''
    while not request.endswith(b'\r\n\r\n'):
        byte = sys.stdin.buffer.read(1)
        if not byte:
            sys.exit(0)
        request += byte
    key = next(line.split(b':', 1)[1].strip() for line in request.split(b'\r\n')
               if line.lower().startswith(b'sec-websocket-key:'))
    accept = base64.b64encode(hashlib.sha1(key + b'258EAFA5-E914-47DA-95CA-C5AB0DC85B11').digest())
    sys.stdout.buffer.write(b'HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ' + accept + b'\r\n\r\n')
    sys.stdout.buffer.flush()

    def send(value):
        data = json.dumps(value).encode()
        size = len(data)
        length = bytes([size]) if size < 126 else b'\x7e' + struct.pack('!H', size)
        sys.stdout.buffer.write(b'\x81' + length + data)
        sys.stdout.buffer.flush()

    initialized = False
    while True:
        header = sys.stdin.buffer.read(2)
        if len(header) != 2 or header[0] & 15 == 8:
            sys.exit(0)
        assert header[0] & 15 == 1
        size = header[1] & 127
        if size == 126:
            size = struct.unpack('!H', sys.stdin.buffer.read(2))[0]
        elif size == 127:
            size = struct.unpack('!Q', sys.stdin.buffer.read(8))[0]
        mask = sys.stdin.buffer.read(4) if header[1] & 128 else None
        data = sys.stdin.buffer.read(size)
        if mask:
            data = bytes(value ^ mask[index % 4] for index, value in enumerate(data))
        message = json.loads(data)
        method = message['method']
        params = message.get('params', {})
        record(method, params=params)
        if method == 'initialized':
            initialized = True
            continue
        result = {}
        capability = policy('capability')
        thread = capability.get('thread', 'native-fixture')
        if method == 'initialize':
            assert params['capabilities']['experimentalApi'] is True
            if capability.get('initialize') == 'rejected':
                send({'id': message['id'], 'error': {'code': -32600, 'message': 'Initialization unavailable'}})
                continue
        elif method == 'thread/loaded/list':
            assert initialized
            loaded = capability.get('loaded', 'ready')
            if loaded == 'unavailable':
                send({'id': message['id'], 'error': {'code': -32600, 'message': 'Lookup unavailable'}})
                continue
            if loaded == 'paged' and 'cursor' not in params:
                result = {'data': ['another-fixture'], 'nextCursor': 'page-2'}
            elif loaded == 'missing':
                result = {'data': [], 'nextCursor': None}
            else:
                if loaded == 'paged':
                    assert params['cursor'] == 'page-2'
                result = {'data': [thread], 'nextCursor': None}
        elif method == 'thread/queue/list':
            assert initialized
            assert params == {'threadId': thread, 'limit': 1}
            queue = capability.get('queue', 'ready')
            if queue == 'closed':
                sys.exit(0)
            if queue == 'timeout':
                time.sleep(60)
            if queue in ('ineligible', 'unsupported', 'unavailable'):
                code = {'ineligible': -32600, 'unsupported': -32601, 'unavailable': -32603}[queue]
                send({'id': message['id'], 'error': {'code': code, 'message': 'SYNTHETIC_PROVIDER_DETAIL_MUST_NOT_LEAK'}})
                continue
            result = {'data': [], 'nextCursor': None}
            if queue == 'occupied':
                result['data'] = [{'input': [{'type': 'text', 'text': 'SYNTHETIC_NATIVE_INPUT_MUST_NOT_COPY'}]}]
            elif queue == 'malformed':
                result['data'] = {}
        elif method == 'thread/read':
            assert params == {'threadId': thread, 'includeTurns': False}
            origin = capability.get('origin', 'cli')
            if origin == 'timeout':
                time.sleep(60)
            if origin == 'closed':
                sys.exit(0)
            if origin in ('unsupported', 'failed'):
                send({'id': message['id'], 'error': {'code': -32601 if origin == 'unsupported' else -32603, 'message': 'SYNTHETIC_PROVIDER_DETAIL_MUST_NOT_LEAK'}})
                continue
            result = {'thread': {'source': origin, 'preview': 'SYNTHETIC_NATIVE_INPUT_MUST_NOT_COPY'}}
        else:
            sys.exit(93)
        send({'id': message['id'], 'result': result})

if args and args[0] == 'queue':
    assert args[:4] == ['queue', '--remote', 'unix://', '--thread']
    assert args[5] == '--message' and len(args) == 7
    record('queue', threadId=args[4])
    handoff = policy('handoff')
    (home / 'handoff-started').touch()
    if handoff.get('wait'):
        deadline = time.monotonic() + 15
        while not (home / 'handoff-release').exists():
            assert time.monotonic() < deadline
            time.sleep(0.01)
    if 'after' in handoff:
        (home / 'capability.json').write_text(json.dumps(handoff['after']))
    if handoff.get('outcome') == 'timeout':
        time.sleep(60)
    if handoff.get('outcome') == 'rejected':
        print('Error: -32600 capacity full; SYNTHETIC_PROVIDER_DETAIL_MUST_NOT_LEAK', file=sys.stderr)
    sys.exit(1 if handoff.get('outcome') == 'rejected' else 0)

if args == ['--version']:
    print('codex-cli 0.164.0')
    sys.exit(0)
if args in (['mcp', 'list', '--json'], ['mcp', 'get', 'cccc', '--json']):
    mode = os.environ.get('FIXTURE_MCP_READ', '')
    if mode == 'unavailable':
        print('PROVIDER_OUTPUT_MUST_NOT_LEAK', file=sys.stderr)
        sys.exit(1)
    if mode == 'malformed':
        print('PROVIDER_OUTPUT_MUST_NOT_LEAK')
        sys.exit(0)
    if mode == 'oversized':
        print(' ' * 65536, end='')
    if mode == 'not_array':
        print('{}')
        sys.exit(0)
    if mode == 'invalid_entry':
        print('[{"transport": {"type": "stdio"}}]')
        sys.exit(0)
    if args == ['mcp', 'list', '--json']:
        print(json.dumps([dict(entry, name=name) for name, entry in servers.items()]))
        sys.exit(0)
    if 'cccc' not in servers:
        print('The requested server does not exist.', file=sys.stderr)
        sys.exit(1)
    print(json.dumps(servers['cccc']))
    sys.exit(0)
if args[:3] == ['mcp', 'remove', 'cccc'] and len(args) == 3:
    servers.pop('cccc', None)
elif args[:3] == ['mcp', 'add', 'cccc'] and '--' in args:
    if 'cccc' in servers and (home / 'add-policy').read_text() == 'already_exists':
        print('cccc already exists', file=sys.stderr)
        sys.exit(1)
    split = args.index('--')
    env = {}
    index = 3
    while index < split:
        assert args[index] == '--env'
        key, value = args[index + 1].split('=', 1)
        env[key] = value
        index += 2
    command = args[split + 1:]
    assert command
    servers['cccc'] = {
        'name': 'cccc', 'enabled': True, 'disabled_reason': None,
        'transport': {'type': 'stdio', 'command': command[0], 'args': command[1:],
                      'env': env or None, 'env_vars': [], 'cwd': None},
        'enabled_tools': None, 'disabled_tools': None,
        'startup_timeout_sec': None, 'tool_timeout_sec': None,
    }
else:
    print('unexpected fake Codex command', file=sys.stderr)
    sys.exit(93)
path.write_text(json.dumps(servers))
"#,
        )
        .expect("fake Codex CLI");
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700))
            .expect("executable fake Codex");
        Self {
            directory,
            bin,
            provider,
        }
    }

    fn home(&self, name: &str) -> PathBuf {
        self.directory.path().join(name)
    }

    fn command(&self, args: &[&str], home: &Path) -> Command {
        std::fs::create_dir_all(home).expect("fresh CCCC home");
        let mut command = Command::new(env!("CARGO_BIN_EXE_cccc"));
        command
            .args(args)
            .env_clear()
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .env("HOME", self.home("user"))
            .env("USERPROFILE", self.home("user"))
            .env("CCCC_HOME", home)
            .env("CODEX_HOME", &self.provider)
            .env("CLAUDE_CONFIG_DIR", self.home("claude"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .current_dir(self.directory.path());
        command
    }

    fn run(&self, args: &[&str], home: &Path) -> Value {
        let output = self.command(args, home).output().expect("public CCCC CLI");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).expect("CLI response")
    }

    fn registry(&self) -> Value {
        cccc_core::fs::read_json(&self.provider.join("registry.json"))
            .expect("provider registration")
    }

    fn write_registry(&self, registry: &Value) {
        cccc_core::fs::write_json(&self.provider.join("registry.json"), registry)
            .expect("synthetic provider registration");
    }

    fn hooks(&self) -> Vec<u8> {
        std::fs::read(self.provider.join("hooks.json")).expect("provider hooks")
    }

    fn hook(&self, home: &Path, runtime: &str, host_pid: u32, native_id: &str, event: &str) {
        let input = serde_json::to_vec(&json!({
            "session_id":native_id,
            "hook_event_name":event,
            "cwd":self.directory.path(),
            "session_title":"Native fixture",
        }))
        .expect("lifecycle callback");
        let output = cccc_runtime::capture_command_blocking(
            &mut self.command(
                &[
                    "sessions",
                    "hook",
                    "--runtime",
                    runtime,
                    "--host-pid",
                    &host_pid.to_string(),
                ],
                home,
            ),
            Some(&input),
            std::time::Duration::from_secs(5),
            4096,
        )
        .expect("public lifecycle callback");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            output.stdout.is_empty(),
            "callbacks must not expose provider data"
        );
        assert!(
            output.stderr.is_empty(),
            "callbacks must not expose provider errors"
        );
    }

    fn native_calls(&self) -> Vec<Value> {
        std::fs::read_to_string(self.provider.join("native-calls.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).expect("synthetic call record"))
            .collect()
    }

    fn daemon(&self, home: &Path) -> FixtureProcess {
        let mut process = FixtureProcess(
            self.command(
                &["daemon", "run", "--host", "127.0.0.1", "--port", "0"],
                home,
            )
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("isolated daemon"),
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !cccc_core::fs::read_json::<Value>(&home.join("daemon/ccccd.addr.json"))
            .is_ok_and(|address| address["pid"].as_u64() == Some(u64::from(process.0.id())))
        {
            assert!(process.0.try_wait().expect("daemon status").is_none());
            assert!(
                std::time::Instant::now() < deadline,
                "isolated daemon startup"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        process
    }

    async fn request(&self, home: &Path, op: &str, args: Value) -> Value {
        let home = cccc_core::HomeLayout::from_path(home).expect("home");
        let response = cccc_client::DaemonClient::new(home)
            .call(&cccc_contracts::DaemonRequest {
                v: 1,
                op: op.into(),
                args: args.as_object().expect("request object").clone(),
            })
            .await
            .expect("public daemon IPC");
        assert!(response.ok, "{op}: {:?}", response.error);
        Value::Object(response.result)
    }

    fn receipts(&self, home: &Path, group_id: &str, event_id: &str) -> Vec<cccc_contracts::Event> {
        let home = cccc_core::HomeLayout::from_path(home).expect("home");
        let store = cccc_core::GroupStore::new(home).expect("Groups");
        cccc_core::ledger::read_all(&store.ledger_path(group_id).expect("ledger"))
            .expect("public CCCC delivery events")
            .into_iter()
            .filter(|event| {
                event.kind == "runtime.delivery" && event.data["source_event_id"] == event_id
            })
            .collect()
    }

    async fn wait_receipt(
        &self,
        home: &Path,
        group_id: &str,
        event_id: &str,
        expected: &str,
    ) -> cccc_contracts::Event {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            let receipt = self.receipts(home, group_id, event_id).pop();
            if let Some(receipt) = receipt
                && receipt.data["state"] == expected
            {
                return receipt;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "expected delivery {expected}"
            );
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }

    async fn wait_qualification(&self, home: &Path, since: usize, ready: bool) -> Value {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let calls = self.native_calls();
            let finished = calls[since..]
                .iter()
                .find(|call| call["method"] == "proxy")
                .and_then(|call| call["pid"].as_u64())
                .is_some_and(|pid| {
                    cccc_core::local_sessions::NativeHost::capture(pid as u32)
                        .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
                });
            let directory = self
                .request(
                    home,
                    "local_session_list",
                    json!({
                        "include_offline":true,"source_id":"codex:native-fixture"
                    }),
                )
                .await;
            if finished && directory["sources"][0]["connected"] == ready {
                return directory;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "qualification did not settle to {ready}"
            );
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }

    fn assert_tool_contract(&self, registration: &Value, home: &Path, native: bool) {
        let transport = &registration["transport"];
        assert_eq!(transport["command"], env!("CARGO_BIN_EXE_cccc"));
        let mut command = self.command(&[], home);
        for arg in transport["args"].as_array().expect("registered args") {
            command.arg(arg.as_str().expect("registered argument"));
        }
        if let Some(env) = transport["env"].as_object() {
            for (key, value) in env {
                command.env(key, value.as_str().expect("registered environment value"));
            }
        }
        let input = format!(
            "{}\n{}\n",
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"offline-setup-fixture","version":"1"}}}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}})
        );
        let output = cccc_runtime::capture_command_blocking(
            &mut command,
            Some(input.as_bytes()),
            std::time::Duration::from_secs(5),
            1024 * 1024,
        )
        .expect("registered MCP server");
        assert!(output.status.success());
        assert!(!output.stdout_truncated);
        let response = String::from_utf8(output.stdout)
            .expect("MCP stdout")
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("MCP JSON"))
            .find(|message| message["id"] == 2)
            .expect("tools/list response");
        let tools = response["result"]["tools"].as_array().expect("tools");
        assert_eq!(
            tools.iter().any(|tool| tool["name"] == "cccc_session"),
            native
        );
        let send = tools
            .iter()
            .find(|tool| tool["name"] == "cccc_message_send")
            .expect("message tool");
        assert_eq!(
            send["inputSchema"]["required"]
                .as_array()
                .expect("required parameters")
                .contains(&json!("group_id")),
            native,
            "native tools retain explicit Group identity instead of active-Group fallback"
        );
        let history = tools
            .iter()
            .find(|tool| tool["name"] == "cccc_message_history")
            .expect("history tool");
        assert_eq!(
            history["inputSchema"]["properties"]["event_id"]["type"],
            "string"
        );
    }
}

fn assert_native_codex_setup_preserved(policy: &str) {
    for form in ["split", "equals", "extra_args", "default_home"] {
        let fixture = CodexSetupFixture::new(policy);
        let original_home = fixture.home("native-home");
        let other_home = fixture.home("other-home");
        fixture.run(
            &["sessions", "enable", "--runtime", "codex"],
            &original_home,
        );
        let mut registered = fixture.registry();
        assert_eq!(
            registered["cccc"]["transport"]["args"],
            json!(["mcp", "--local-session-runtime", "codex"])
        );
        if form == "equals" {
            registered["cccc"]["transport"]["args"] =
                json!(["mcp", "--local-session-runtime=codex"]);
            registered["cccc"]["enabled"] = json!(false);
        } else if form == "extra_args" {
            registered["cccc"]["transport"]["args"] = json!([
                "mcp",
                "--local-session-host-pid",
                std::process::id().to_string(),
                "--local-session-runtime",
                "codex"
            ]);
        } else if form == "default_home" {
            registered["cccc"]["transport"]["env"] = json!({});
        }
        registered["cccc"]["startup_timeout_sec"] = json!(17.0);
        registered["cccc"]["transport"]["env"]["UNRELATED_FIXTURE"] = json!("keep");
        fixture.write_registry(&registered);
        let hooks = fixture.hooks();
        let response = fixture.run(&["setup", "--runtime", "codex"], &other_home);
        assert_eq!(
            fixture.registry(),
            registered,
            "ordinary setup must not replace the native MCP registration or its original Home"
        );
        assert_eq!(fixture.hooks(), hooks);
        assert_eq!(response["status"], "ready");
        assert_eq!(response["preserved"], true);
        assert_eq!(response["enabled"], registered["cccc"]["enabled"]);
        assert_eq!(
            response["home"],
            registered["cccc"]["transport"]["env"]["CCCC_HOME"]
        );
        fixture.assert_tool_contract(&registered["cccc"], &other_home, true);
    }
}

#[test]
fn ordinary_codex_setup_preserves_native_with_overwriting_provider() {
    assert_native_codex_setup_preserved("overwrite");
}

#[test]
fn ordinary_codex_setup_preserves_native_with_duplicate_rejecting_provider() {
    assert_native_codex_setup_preserved("already_exists");
}

#[test]
fn ordinary_codex_setup_remains_plain_and_idempotent() {
    for policy in ["overwrite", "already_exists"] {
        let fixture = CodexSetupFixture::new(policy);
        let home = fixture.home("ordinary-home");
        let unrelated = fixture.registry()["unrelated"].clone();
        let hooks = fixture.hooks();
        for _ in 0..2 {
            let response = fixture.run(&["setup", "--runtime", "codex"], &home);
            assert_eq!(response["status"], "added");
            let registered = fixture.registry();
            assert_eq!(registered["cccc"]["transport"]["args"], json!(["mcp"]));
            assert_eq!(registered["cccc"]["transport"]["env"], Value::Null);
            assert_eq!(registered["unrelated"], unrelated);
            assert_eq!(fixture.hooks(), hooks);
            fixture.assert_tool_contract(&registered["cccc"], &home, false);
        }
    }
}

#[test]
fn ordinary_codex_setup_uses_registration_not_leftover_native_hooks() {
    let fixture = CodexSetupFixture::new("overwrite");
    fixture.run(
        &["sessions", "enable", "--runtime", "codex"],
        &fixture.home("native-home"),
    );
    let mut registered = fixture.registry();
    registered["cccc"]["transport"]["args"] = json!(["mcp"]);
    fixture.write_registry(&registered);
    let hooks = fixture.hooks();
    let home = fixture.home("ordinary-home");
    fixture.run(&["setup", "--runtime", "codex"], &home);
    assert_eq!(
        fixture.registry()["cccc"]["transport"]["args"],
        json!(["mcp"])
    );
    assert_eq!(fixture.registry()["unrelated"], registered["unrelated"]);
    assert_eq!(fixture.hooks(), hooks);
    fixture.assert_tool_contract(&fixture.registry()["cccc"], &home, false);
}

#[test]
fn explicit_codex_disable_still_restores_ordinary_mcp_and_keeps_unrelated_hooks() {
    use cccc_core::local_sessions::{self, RetireReason};

    let fixture = CodexSetupFixture::new("overwrite");
    let home = fixture.home("native-home");
    let layout = cccc_core::HomeLayout::from_path(&home).expect("home");
    fixture.run(&["sessions", "enable", "--runtime", "codex"], &home);
    fixture.hook(
        &home,
        "codex",
        std::process::id(),
        "native-fixture",
        "SessionStart",
    );
    let original = local_sessions::read(&layout, "codex:native-fixture").expect("registered");
    let unrelated = fixture.registry()["unrelated"].clone();
    let disabled = fixture.run(&["sessions", "disable", "--runtime", "codex"], &home);
    assert_eq!(disabled["enabled"], false);
    assert_eq!(
        fixture.registry()["cccc"]["transport"]["args"],
        json!(["mcp"])
    );
    assert_eq!(fixture.registry()["unrelated"], unrelated);
    let hooks: Value = serde_json::from_slice(&fixture.hooks()).expect("hooks JSON");
    assert_eq!(
        hooks["hooks"]["SessionStart"][0]["hooks"][0]["statusMessage"],
        "unrelated"
    );
    assert!(
        !String::from_utf8(fixture.hooks())
            .expect("hooks")
            .contains("CCCC local session connection")
    );
    fixture.run(&["setup", "--runtime", "codex"], &home);
    fixture.assert_tool_contract(&fixture.registry()["cccc"], &home, false);
    let calls = fixture.native_calls();
    for (native_id, event) in [
        ("native-fixture", "UserPromptSubmit"),
        ("another-thread", "SessionStart"),
    ] {
        fixture.hook(&home, "codex", std::process::id(), native_id, event);
    }
    assert_eq!(
        fixture.native_calls(),
        calls,
        "unreloaded callbacks must not query the provider after explicit disable"
    );
    let retained =
        local_sessions::read_record(&layout, &original.source_id).expect("disabled recovery");
    assert_eq!(retained.retired, Some(RetireReason::IntegrationDisabled));
    assert_eq!(retained.revision, original.revision);
    assert!(local_sessions::read(&layout, &original.source_id).is_err());
    assert!(local_sessions::read_record(&layout, "codex:another-thread").is_err());
    assert!(
        local_sessions::write(&layout, original.clone()).is_err(),
        "a registration already in progress cannot write through the disable fence"
    );
    fixture.run(&["sessions", "enable", "--runtime", "codex"], &home);
    assert!(
        local_sessions::read(&layout, &original.source_id).is_err(),
        "enabling configuration alone must not revive the retired identity"
    );
    fixture.hook(
        &home,
        "codex",
        std::process::id(),
        "native-fixture",
        "UserPromptSubmit",
    );
    let resumed = local_sessions::read(&layout, &original.source_id).expect("explicitly enabled");
    assert!(resumed.retired.is_none());
    assert!(resumed.codex_can_receive);
    assert_ne!(resumed.revision, original.revision);
}

#[test]
fn codex_disable_remains_effective_when_provider_cleanup_or_reenable_fails() {
    use cccc_core::local_sessions::{self, RetireReason};

    let fixture = CodexSetupFixture::new("overwrite");
    let home = fixture.home("native-home");
    let layout = cccc_core::HomeLayout::from_path(&home).expect("home");
    fixture.run(&["sessions", "enable", "--runtime", "codex"], &home);
    fixture.hook(
        &home,
        "codex",
        std::process::id(),
        "native-fixture",
        "SessionStart",
    );
    let original = local_sessions::read(&layout, "codex:native-fixture").expect("registered");
    let hooks = fixture.hooks();
    std::fs::write(fixture.provider.join("hooks.json"), b"{\"hooks\":false}")
        .expect("broken provider configuration");
    for action in ["disable", "enable"] {
        let output = fixture
            .command(&["sessions", action, "--runtime", "codex"], &home)
            .output()
            .expect("failed provider configuration");
        assert!(
            !output.status.success(),
            "{action} must report cleanup failure"
        );
        let calls = fixture.native_calls();
        fixture.hook(
            &home,
            "codex",
            std::process::id(),
            "native-fixture",
            "UserPromptSubmit",
        );
        fixture.hook(
            &home,
            "codex",
            std::process::id(),
            "another-thread",
            "SessionStart",
        );
        assert_eq!(fixture.native_calls(), calls);
        assert!(local_sessions::read(&layout, &original.source_id).is_err());
        assert_eq!(
            local_sessions::read_record(&layout, &original.source_id)
                .expect("recovery")
                .retired,
            Some(RetireReason::IntegrationDisabled)
        );
        assert!(local_sessions::read_record(&layout, "codex:another-thread").is_err());
    }
    std::fs::write(fixture.provider.join("hooks.json"), hooks).expect("repair fixture");
    fixture.run(&["sessions", "enable", "--runtime", "codex"], &home);
    assert!(local_sessions::read(&layout, &original.source_id).is_err());
    fixture.hook(
        &home,
        "codex",
        std::process::id(),
        "native-fixture",
        "SessionStart",
    );
    assert!(
        local_sessions::read(&layout, &original.source_id)
            .expect("fresh callback")
            .codex_can_receive
    );
}

#[test]
fn ordinary_codex_setup_refuses_uncertain_registration_without_mutating_or_leaking_it() {
    for mode in [
        "unavailable",
        "malformed",
        "oversized",
        "not_array",
        "invalid_entry",
    ] {
        let fixture = CodexSetupFixture::new("overwrite");
        fixture.run(
            &["sessions", "enable", "--runtime", "codex"],
            &fixture.home("native-home"),
        );
        let registered = fixture.registry();
        let hooks = fixture.hooks();
        let output = fixture
            .command(
                &["setup", "--runtime", "codex"],
                &fixture.home("other-home"),
            )
            .env("FIXTURE_MCP_READ", mode)
            .output()
            .expect("ordinary setup with uncertain inspection");
        assert!(!output.status.success(), "inspection mode: {mode}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("left unchanged"));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("PROVIDER_OUTPUT_MUST_NOT_LEAK"));
        assert!(!String::from_utf8_lossy(&output.stdout).contains("PROVIDER_OUTPUT_MUST_NOT_LEAK"));
        assert_eq!(fixture.registry(), registered, "inspection mode: {mode}");
        assert_eq!(fixture.hooks(), hooks, "inspection mode: {mode}");
    }
}

#[test]
fn ordinary_codex_setup_repairs_known_non_native_or_broken_entries() {
    for transport in [
        json!({"type":"stdio","command":"old-cccc","args":["mcp","--obsolete"]}),
        json!({"type":"stdio","command":"old-cccc","args":["mcp","--local-session-runtime","unknown"]}),
        json!({"type":"stdio","command":"old-cccc","args":"mcp"}),
        json!({"type":"stdio","command":"","args":["mcp","--local-session-runtime","codex"]}),
        json!({"type":"streamable_http","url":"https://unused.invalid/mcp"}),
    ] {
        let fixture = CodexSetupFixture::new("overwrite");
        let home = fixture.home("ordinary-home");
        let mut registered = fixture.registry();
        registered["cccc"] = json!({"name":"cccc","enabled":true,"transport":transport});
        fixture.write_registry(&registered);
        let hooks = fixture.hooks();
        let repaired = fixture.run(&["setup", "--runtime", "codex"], &home);
        assert_eq!(repaired["status"], "added");
        assert_eq!(
            fixture.registry()["cccc"]["transport"]["args"],
            json!(["mcp"])
        );
        assert_eq!(fixture.registry()["unrelated"], registered["unrelated"]);
        assert_eq!(fixture.hooks(), hooks);
    }
}

#[test]
fn ordinary_codex_setup_allows_unrelated_empty_names_without_overwriting_native_registration() {
    for native in [false, true] {
        let fixture = CodexSetupFixture::new("overwrite");
        if native {
            fixture.run(
                &["sessions", "enable", "--runtime", "codex"],
                &fixture.home("native-home"),
            );
        }
        let mut registered = fixture.registry();
        registered[""] = json!({
            "enabled":false,
            "transport":{"type":"stdio","command":"unused-fixture","args":[]}
        });
        fixture.write_registry(&registered);
        let hooks = fixture.hooks();
        let home = fixture.home("ordinary-home");
        let response = fixture.run(&["setup", "--runtime", "codex"], &home);
        assert_eq!(response["status"], if native { "ready" } else { "added" });
        assert_eq!(fixture.registry()[""], registered[""]);
        assert_eq!(fixture.registry()["unrelated"], registered["unrelated"]);
        assert_eq!(fixture.hooks(), hooks);
        if native {
            assert_eq!(response["preserved"], true);
            assert_eq!(fixture.registry(), registered);
        }
        fixture.assert_tool_contract(&fixture.registry()["cccc"], &home, native);
    }
}

#[tokio::test]
async fn native_backlogs_drain_past_the_queue_and_startup_page_without_manual_redispatch() {
    use std::collections::HashSet;
    use std::time::{Duration, Instant};

    for (count, restart) in [(400, false), (270, true)] {
        let fixture = CodexSetupFixture::new("overwrite");
        let home = fixture.home("backlog-home");
        fixture.hook(
            &home,
            "codex",
            std::process::id(),
            "native-fixture",
            "SessionStart",
        );
        let mut daemon = Some(fixture.daemon(&home));
        let created = fixture
            .request(
                &home,
                "local_session_conversation",
                json!({"source_ids":["codex:native-fixture"]}),
            )
            .await;
        let group = created["group_id"].as_str().expect("Group");
        let actor = created["group"]["actors"][0]["id"].as_str().expect("Actor");
        let control = fixture.request(&home, "message_send", json!({"group_id":group,"by":"user","to":[actor],"message_mode":"send","text":"Accepted before disconnect"})).await;
        let control_id = control["event"]["id"].as_str().expect("control ID");
        fixture
            .wait_receipt(&home, group, control_id, "accepted")
            .await;
        cccc_core::fs::write_json(
            &fixture.provider.join("capability.json"),
            &json!({"loaded":"missing"}),
        )
        .expect("offline synthetic receiver");
        fixture.hook(
            &home,
            "codex",
            std::process::id(),
            "native-fixture",
            "UserPromptSubmit",
        );
        let mut expected = HashSet::from([control_id.to_owned()]);
        for index in 0..count {
            let sent = fixture.request(&home, "message_send", json!({"group_id":group,"by":"user","to":[actor],"message_mode":"send","text":format!("Offline backlog {index}")})).await;
            expected.insert(sent["event"]["id"].as_str().expect("message ID").to_owned());
        }
        assert_eq!(
            fixture
                .native_calls()
                .iter()
                .filter(|call| call["method"] == "queue")
                .count(),
            1
        );
        if restart {
            drop(daemon.take());
            daemon = Some(fixture.daemon(&home));
        }
        cccc_core::fs::write_json(&fixture.provider.join("capability.json"), &json!({}))
            .expect("synthetic receiver restored");
        let refreshed = fixture
            .request(
                &home,
                "local_session_refresh",
                json!({"group_id":group,"actor_id":actor,"by":"user"}),
            )
            .await;
        assert_eq!(refreshed["recovery"]["state"], "connected");
        let layout = cccc_core::HomeLayout::from_path(&home).expect("home");
        let ledger = cccc_core::GroupStore::new(layout)
            .expect("store")
            .ledger_path(group)
            .expect("ledger");
        let deadline = Instant::now() + Duration::from_secs(45);
        loop {
            let events = cccc_core::ledger::read_all(&ledger).expect("delivery ledger");
            let accepted = events
                .iter()
                .filter(|event| {
                    event.kind == "runtime.delivery"
                        && event.data["actor_id"] == actor
                        && event.data["state"] == "accepted"
                })
                .map(|event| {
                    event.data["source_event_id"]
                        .as_str()
                        .expect("accepted ID")
                        .to_owned()
                })
                .collect::<Vec<_>>();
            if accepted.len() == expected.len() {
                assert_eq!(accepted.into_iter().collect::<HashSet<_>>(), expected);
                break;
            }
            assert!(
                Instant::now() < deadline,
                "backlog={count}, restart={restart}: only {} of {} messages accepted",
                accepted.len(),
                expected.len()
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let calls = fixture.native_calls();
        assert_eq!(
            calls
                .iter()
                .filter(|call| call["method"] == "queue")
                .count(),
            count + 1
        );
        tokio::time::sleep(Duration::from_millis(1200)).await;
        assert_eq!(
            fixture.native_calls(),
            calls,
            "idle reconciliation must not probe or replay native work"
        );
        drop(daemon);
    }
}

struct FixtureProcess(std::process::Child);

impl Drop for FixtureProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
async fn ended_and_exited_sources_keep_their_reason_and_are_included_in_explicit_disable() {
    use cccc_core::local_sessions::{self, RetireReason};
    let fixture = CodexSetupFixture::new("overwrite");
    let home = fixture.home("retirement-home");
    let layout = cccc_core::HomeLayout::from_path(&home).expect("home");
    fixture.run(&["sessions", "enable", "--runtime", "codex"], &home);
    let mut host = FixtureProcess(
        Command::new("/bin/sleep")
            .arg("30")
            .env_clear()
            .spawn()
            .expect("owned native host"),
    );
    fixture.hook(
        &home,
        "codex",
        host.0.id(),
        "native-fixture",
        "SessionStart",
    );
    fixture.hook(
        &home,
        "codex",
        host.0.id(),
        "crashed-fixture",
        "SessionStart",
    );
    fixture.hook(
        &home,
        "claude",
        std::process::id(),
        "unrelated",
        "SessionStart",
    );
    let unrelated = local_sessions::scan(&layout)
        .expect("live sources")
        .0
        .into_iter()
        .find(|session| {
            session.runtime == cccc_contracts::local_session::LocalSessionRuntime::Claude
        })
        .expect("unrelated runtime");
    let _daemon = fixture.daemon(&home);
    let created = fixture
        .request(
            &home,
            "local_session_conversation",
            json!({"source_ids":["codex:native-fixture"]}),
        )
        .await;
    let args = json!({
        "group_id":created["group_id"],
        "actor_id":created["group"]["actors"][0]["id"],
        "by":"user",
    });
    fixture.hook(&home, "codex", host.0.id(), "native-fixture", "SessionEnd");
    host.0.kill().expect("exit owned host");
    host.0.wait().expect("reap owned host");
    let ended = fixture
        .request(&home, "local_session_recovery", args.clone())
        .await;
    assert_eq!(ended["recovery"]["state"], "session_ended");
    let calls = fixture.native_calls();
    let refreshed = fixture
        .request(&home, "local_session_refresh", args.clone())
        .await;
    assert_eq!(refreshed["refresh_status"], "not_checked");
    assert_eq!(refreshed["recovery"], ended["recovery"]);
    assert_eq!(refreshed["source_id"], ended["source_id"]);
    assert_eq!(
        fixture.native_calls(),
        calls,
        "refresh must not revive an ended host"
    );
    let original =
        local_sessions::read_record(&layout, "codex:native-fixture").expect("retained identity");
    fixture.run(&["sessions", "disable", "--runtime", "codex"], &home);
    for source_id in ["codex:native-fixture", "codex:crashed-fixture"] {
        let disabled = local_sessions::read_record(&layout, source_id).expect("disabled record");
        assert_eq!(disabled.retired, Some(RetireReason::IntegrationDisabled));
        assert!(!disabled.codex_can_receive);
        assert!(local_sessions::read(&layout, source_id).is_err());
    }
    assert_eq!(
        fixture.request(&home, "local_session_refresh", args).await["recovery"]["state"],
        "integration_disabled"
    );
    assert_eq!(
        fixture.native_calls(),
        calls,
        "refresh must not query a disabled source"
    );
    assert_eq!(
        local_sessions::read(&layout, &unrelated.source_id).expect("unrelated runtime unchanged"),
        unrelated
    );
    local_sessions::retire(
        &layout,
        &original.source_id,
        &original.native_session_id,
        &original.host,
        RetireReason::SessionEnd,
    )
    .expect("late end after disable");
    assert_eq!(
        local_sessions::read_record(&layout, &original.source_id)
            .expect("disable remains authoritative")
            .retired,
        Some(RetireReason::IntegrationDisabled)
    );
}

#[test]
fn a_late_codex_end_cannot_retire_the_same_thread_on_a_replacement_host() {
    use cccc_core::local_sessions::{self, NativeHost};
    let fixture = CodexSetupFixture::new("overwrite");
    let home = fixture.home("lifecycle-home");
    let layout = cccc_core::HomeLayout::from_path(&home).expect("home");
    let replacement = FixtureProcess(
        Command::new("/bin/sleep")
            .arg("30")
            .env_clear()
            .spawn()
            .expect("isolated replacement host"),
    );
    fixture.hook(
        &home,
        "codex",
        std::process::id(),
        "native-fixture",
        "SessionStart",
    );
    let old = local_sessions::read(&layout, "codex:native-fixture").expect("original host");
    fixture.hook(
        &home,
        "codex",
        replacement.0.id(),
        "native-fixture",
        "SessionStart",
    );
    let current = local_sessions::read(&layout, &old.source_id).expect("replacement host");
    assert_eq!(
        current.host,
        NativeHost::capture(replacement.0.id()).expect("host")
    );
    assert_ne!(old.revision, current.revision);
    let calls = fixture.native_calls();
    fixture.hook(
        &home,
        "codex",
        std::process::id(),
        "native-fixture",
        "SessionEnd",
    );
    assert_eq!(
        local_sessions::read(&layout, &old.source_id).expect("replacement retained"),
        current
    );
    fixture.hook(
        &home,
        "codex",
        replacement.0.id(),
        "native-fixture",
        "SessionEnd",
    );
    assert_eq!(
        local_sessions::read(&layout, &old.source_id)
            .expect_err("current host retired")
            .kind(),
        std::io::ErrorKind::NotFound
    );
    assert_eq!(
        fixture.native_calls(),
        calls,
        "retirement must not query a provider"
    );
    let ended = local_sessions::read_record(&layout, &old.source_id).expect("recovery record");
    assert_eq!(ended.native_session_id, "native-fixture");
    assert_eq!(ended.codex_home, Some(fixture.provider.clone()));
    assert_eq!(
        ended.retired,
        Some(local_sessions::RetireReason::SessionEnd)
    );
    fixture.hook(
        &home,
        "codex",
        replacement.0.id(),
        "native-fixture",
        "SessionStart",
    );
    let resumed = local_sessions::read(&layout, &old.source_id)
        .expect("fresh callback reactivates the exact source");
    assert_eq!(resumed.source_id, old.source_id);
    assert_eq!(resumed.native_session_id, "native-fixture");
    assert_ne!(resumed.revision, ended.revision);
    assert!(resumed.retired.is_none());
}

#[test]
fn a_late_claude_end_cannot_retire_the_current_conversation_on_the_same_host() {
    use cccc_core::local_sessions::{self, NativeHost};
    let fixture = CodexSetupFixture::new("overwrite");
    let home = fixture.home("lifecycle-home");
    let layout = cccc_core::HomeLayout::from_path(&home).expect("home");
    let source_id = format!(
        "claude:{}",
        NativeHost::capture(std::process::id()).expect("host").key()
    );
    fixture.hook(&home, "claude", std::process::id(), "first", "SessionStart");
    let first = local_sessions::read(&layout, &source_id).expect("first conversation");
    fixture.hook(
        &home,
        "claude",
        std::process::id(),
        "second",
        "SessionStart",
    );
    let second = local_sessions::read(&layout, &source_id).expect("second conversation");
    assert_ne!(first.revision, second.revision);
    fixture.hook(&home, "claude", std::process::id(), "first", "SessionEnd");
    assert_eq!(
        local_sessions::read(&layout, &source_id).expect("current conversation retained"),
        second
    );
    fixture.hook(&home, "claude", std::process::id(), "second", "SessionEnd");
    assert_eq!(
        local_sessions::read(&layout, &source_id)
            .expect_err("current conversation retired")
            .kind(),
        std::io::ErrorKind::NotFound
    );
    assert!(
        fixture.native_calls().is_empty(),
        "Claude callbacks must not query Codex"
    );
    let ended =
        local_sessions::read_record(&layout, &source_id).expect("last conversation retained");
    assert_eq!(
        ended.native_session_id, "second",
        "clear updated the recoverable conversation"
    );
    assert_eq!(ended.claude_config_dir, Some(fixture.home("claude")));
    assert_eq!(
        ended.retired,
        Some(local_sessions::RetireReason::SessionEnd)
    );
}

#[test]
fn codex_callbacks_recheck_loaded_and_public_queue_capability_without_copying_input() {
    use cccc_core::local_sessions;
    let fixture = CodexSetupFixture::new("overwrite");
    let home = fixture.home("callback-home");
    let layout = cccc_core::HomeLayout::from_path(&home).expect("home");
    let mut revision = None;
    for (loaded, queue, ready) in [
        ("ready", "closed", false),
        ("ready", "occupied", true),
        ("ready", "unsupported", false),
        ("ready", "ready", true),
        ("missing", "ready", false),
        ("paged", "ready", true),
        ("ready", "ineligible", false),
        ("ready", "ready", true),
        ("ready", "malformed", true),
        ("ready", "closed", true),
    ] {
        cccc_core::fs::write_json(
            &fixture.provider.join("capability.json"),
            &json!({"loaded":loaded,"queue":queue}),
        )
        .expect("synthetic capability");
        let before = fixture.native_calls().len();
        fixture.hook(
            &home,
            "codex",
            std::process::id(),
            "native-fixture",
            if revision.is_none() {
                "SessionStart"
            } else {
                "UserPromptSubmit"
            },
        );
        let session =
            local_sessions::read(&layout, "codex:native-fixture").expect("callback identity");
        assert_eq!(
            session.codex_can_receive, ready,
            "loaded={loaded}, queue={queue}"
        );
        if let Some(revision) = &revision {
            assert_eq!(
                &session.revision, revision,
                "capability changes do not replace native conversation identity"
            );
        } else {
            revision = Some(session.revision.clone());
        }
        let calls = fixture.native_calls();
        let methods = calls[before..]
            .iter()
            .map(|call| call["method"].as_str().expect("method"))
            .collect::<Vec<_>>();
        let mut expected = vec!["proxy", "initialize", "initialized", "thread/loaded/list"];
        if loaded == "paged" {
            expected.push("thread/loaded/list");
        }
        if loaded != "missing" {
            expected.push("thread/queue/list");
        }
        if loaded == "missing" || !matches!(queue, "closed" | "malformed") {
            expected.push("thread/read");
        }
        assert_eq!(methods, expected, "one check per callback, no native work");
        let metadata = serde_json::to_string(&session).expect("whitelisted metadata");
        assert!(!metadata.contains("SYNTHETIC_NATIVE_INPUT_MUST_NOT_COPY"));
        assert!(!metadata.contains("SYNTHETIC_PROVIDER_DETAIL_MUST_NOT_LEAK"));
    }
}

#[test]
fn optional_codex_origin_failures_never_remove_receive_readiness() {
    use cccc_core::local_sessions;
    let fixture = CodexSetupFixture::new("overwrite");
    for (index, origin) in [
        json!("cli"),
        json!("vscode"),
        json!("future-origin"),
        json!({"subAgent":"SYNTHETIC_NATIVE_INPUT_MUST_NOT_COPY"}),
        Value::Null,
        json!("unsupported"),
        json!("failed"),
        json!("closed"),
        json!("timeout"),
    ]
    .into_iter()
    .enumerate()
    {
        let home = fixture.home(&format!("origin-{index}"));
        let layout = cccc_core::HomeLayout::from_path(&home).expect("home");
        cccc_core::fs::write_json(
            &fixture.provider.join("capability.json"),
            &json!({"origin":origin}),
        )
        .expect("capability");
        fixture.hook(
            &home,
            "codex",
            std::process::id(),
            "native-fixture",
            "SessionStart",
        );
        let session = local_sessions::read(&layout, "codex:native-fixture").expect("registered");
        assert!(session.codex_can_receive, "optional origin {origin}");
        let expected = origin
            .as_str()
            .filter(|origin| matches!(*origin, "cli" | "vscode"));
        assert_eq!(session.origin.as_deref(), expected);
        let stored = serde_json::to_string(&session).expect("metadata");
        assert!(!stored.contains("SYNTHETIC_NATIVE_INPUT_MUST_NOT_COPY"));
        assert!(!stored.contains("SYNTHETIC_PROVIDER_DETAIL_MUST_NOT_LEAK"));
        if expected == Some("vscode") {
            cccc_core::fs::write_json(
                &fixture.provider.join("capability.json"),
                &json!({"origin":"failed"}),
            )
            .expect("capability");
            fixture.hook(
                &home,
                "codex",
                std::process::id(),
                "native-fixture",
                "UserPromptSubmit",
            );
            let later = local_sessions::read(&layout, "codex:native-fixture").expect("same source");
            assert_eq!(later.origin, session.origin);
            assert_eq!(
                later.revision, session.revision,
                "display hints are not identity changes"
            );
        }
    }
}

#[test]
fn a_codex_callback_without_a_cli_keeps_identity_but_cannot_reuse_receive_readiness() {
    use cccc_core::local_sessions;
    let fixture = CodexSetupFixture::new("overwrite");
    let home = fixture.home("callback-home");
    let layout = cccc_core::HomeLayout::from_path(&home).expect("home");
    fixture.hook(
        &home,
        "codex",
        std::process::id(),
        "native-fixture",
        "SessionStart",
    );
    let previous = local_sessions::read(&layout, "codex:native-fixture").expect("ready source");
    assert!(previous.codex_can_receive);
    let calls = fixture.native_calls();
    let input = serde_json::to_vec(&json!({"session_id":"native-fixture","hook_event_name":"UserPromptSubmit","cwd":fixture.directory.path()})).expect("callback");
    let output = cccc_runtime::capture_command_blocking(
        fixture
            .command(
                &[
                    "sessions",
                    "hook",
                    "--runtime",
                    "codex",
                    "--host-pid",
                    &std::process::id().to_string(),
                ],
                &home,
            )
            .env("PATH", ""),
        Some(&input),
        std::time::Duration::from_secs(5),
        4096,
    )
    .expect("callback without a provider on PATH");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let current = local_sessions::read(&layout, "codex:native-fixture").expect("identity retained");
    assert!(!current.codex_can_receive);
    assert!(current.codex_program.is_none());
    assert_eq!(current.host, previous.host);
    assert_eq!(current.native_session_id, previous.native_session_id);
    assert_eq!(current.revision, previous.revision);
    assert_eq!(
        fixture.native_calls(),
        calls,
        "no fallback provider invocation"
    );
}

#[tokio::test]
async fn codex_handoff_failure_rechecks_only_channel_qualification_and_never_replays() {
    for (outcome, after, ready, expected) in [
        ("rejected", json!({}), true, "failed"),
        ("rejected", json!({"queue":"ineligible"}), false, "failed"),
        ("rejected", json!({"queue":"unsupported"}), false, "failed"),
        ("rejected", json!({"loaded":"missing"}), false, "failed"),
        ("rejected", json!({"initialize":"rejected"}), true, "failed"),
        ("rejected", json!({"loaded":"unavailable"}), true, "failed"),
        ("rejected", json!({"queue":"unavailable"}), true, "failed"),
        ("rejected", json!({"queue":"closed"}), true, "failed"),
        ("rejected", json!({"queue":"malformed"}), true, "failed"),
        ("rejected", json!({"queue":"timeout"}), true, "failed"),
        ("timeout", json!({"queue":"ineligible"}), false, "ambiguous"),
    ] {
        let fixture = CodexSetupFixture::new("overwrite");
        let home = fixture.home("handoff-home");
        fixture.hook(
            &home,
            "codex",
            std::process::id(),
            "native-fixture",
            "SessionStart",
        );
        let _daemon = fixture.daemon(&home);
        let created = fixture.run(
            &["sessions", "create", "--sources", "codex:native-fixture"],
            &home,
        );
        let group = created["group_id"].as_str().expect("Group ID");
        let actor = created["group"]["actors"][0]["id"]
            .as_str()
            .expect("native member");
        cccc_core::fs::write_json(
            &fixture.provider.join("handoff.json"),
            &json!({"outcome":outcome,"after":after}),
        )
        .expect("synthetic queue outcome");
        let sent = fixture.request(&home, "message_send", json!({"group_id":group,"by":"user","to":[actor],"message_mode":"send","text":"SYNTHETIC_HANDOFF"})).await;
        let event_id = sent["event"]["id"].as_str().expect("source event");
        let receipt = fixture.wait_receipt(&home, group, event_id, expected).await;
        assert_eq!(receipt.data["transport"], "local_session");
        assert!(
            receipt
                .data
                .get("retryable")
                .is_none_or(|value| value.as_bool() == Some(false)),
            "an actual native submission must not be classified as never submitted"
        );
        assert_eq!(
            fixture
                .native_calls()
                .iter()
                .filter(|call| call["method"] == "queue")
                .count(),
            1,
            "the fake public queue was actually called once before the failure"
        );
        // The send has its own read-only steer preflight. Qualification starts
        // after the queue handoff; an exited preflight proxy is not evidence
        // that the post-failure qualification has settled.
        let since = fixture
            .native_calls()
            .iter()
            .rposition(|call| call["method"] == "queue")
            .expect("recorded queue handoff")
            + 1;
        assert!(
            !serde_json::to_string(&receipt)
                .expect("receipt")
                .contains("SYNTHETIC_PROVIDER_DETAIL_MUST_NOT_LEAK")
        );

        if after["queue"] == "timeout" {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
            let proxy_pid = loop {
                if let Some(pid) = fixture.native_calls()[since..]
                    .iter()
                    .find(|call| call["method"] == "proxy")
                    .and_then(|call| call["pid"].as_u64())
                {
                    break pid as u32;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "qualification must start"
                );
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            };
            let proxy = cccc_core::local_sessions::NativeHost::capture(proxy_pid)
                .expect("failure receipt must precede qualification timeout");
            let started = std::time::Instant::now();
            fixture
                .request(
                    &home,
                    "actor_stop",
                    json!({"group_id":group,"actor_id":actor,"by":"user"}),
                )
                .await;
            assert!(
                proxy.alive(),
                "Actor stop must not await qualification timeout"
            );
            println!(
                "Actor stopped during pending qualification in {:?}",
                started.elapsed()
            );
            fixture
                .request(
                    &home,
                    "actor_start",
                    json!({"group_id":group,"actor_id":actor,"by":"user"}),
                )
                .await;
        }

        let directory = fixture.wait_qualification(&home, since, ready).await;
        let calls = fixture.native_calls();
        assert_eq!(
            directory["sources"][0]["connected"], ready,
            "outcome={outcome}, after={after}"
        );
        assert_eq!(
            directory["self_memberships"][0]["enabled"], true,
            "negative receive evidence must retain Group access and identity"
        );
        let actors = fixture
            .request(&home, "actor_list", json!({"group_id":group}))
            .await;
        assert_eq!(actors["actors"][0]["running"], ready);
        fixture.run(&["sessions", "list"], &home);
        assert_eq!(
            fixture.native_calls(),
            calls,
            "directory and Actor reads must not query the provider"
        );

        cccc_core::fs::write_json(&fixture.provider.join("capability.json"), &json!({}))
            .expect("channel restored");
        cccc_core::fs::write_json(&fixture.provider.join("handoff.json"), &json!({}))
            .expect("later queue acceptance");
        let refreshed = fixture.run(
            &[
                "sessions",
                "recovery",
                "--group",
                group,
                "--actor",
                actor,
                "--refresh",
            ],
            &home,
        );
        assert_eq!(refreshed["recovery"]["state"], "connected");
        assert_eq!(refreshed["recovery"]["shell"], "posix");
        assert_eq!(refreshed["source_id"], "codex:native-fixture");
        fixture
            .request(
                &home,
                "actor_stop",
                json!({"group_id":group,"actor_id":actor,"by":"user"}),
            )
            .await;
        fixture
            .request(
                &home,
                "actor_start",
                json!({"group_id":group,"actor_id":actor,"by":"user"}),
            )
            .await;
        let before = fixture.native_calls().len();
        let later = fixture.request(&home, "message_send", json!({"group_id":group,"by":"user","to":[actor],"message_mode":"send","text":"SYNTHETIC_LATER_HANDOFF"})).await;
        fixture
            .wait_receipt(
                &home,
                group,
                later["event"]["id"].as_str().expect("later event"),
                "accepted",
            )
            .await;
        let calls = fixture.native_calls();
        assert_eq!(
            calls[before..]
                .iter()
                .map(|call| call["method"].as_str().expect("method"))
                .collect::<Vec<_>>(),
            ["proxy", "initialize", "initialized", "thread/read", "queue"],
            "accepted handoffs preflight the current turn but add no post-failure qualification"
        );
        assert_eq!(
            calls
                .iter()
                .filter(|call| call["method"] == "queue")
                .count(),
            2,
            "reconnection and access resume must not replay a failed or ambiguous handoff"
        );
        assert_eq!(
            fixture
                .receipts(&home, group, event_id)
                .iter()
                .map(|receipt| receipt.data["state"].as_str().expect("state"))
                .collect::<Vec<_>>(),
            ["claimed", expected]
        );
    }
}

#[tokio::test]
async fn a_failed_codex_handoff_cannot_revoke_a_replacement_hosts_readiness() {
    use cccc_core::local_sessions;
    let fixture = CodexSetupFixture::new("overwrite");
    let home = fixture.home("handoff-home");
    let layout = cccc_core::HomeLayout::from_path(&home).expect("home");
    fixture.hook(
        &home,
        "codex",
        std::process::id(),
        "native-fixture",
        "SessionStart",
    );
    let old = local_sessions::read(&layout, "codex:native-fixture").expect("original host");
    let _daemon = fixture.daemon(&home);
    let created = fixture.run(
        &["sessions", "create", "--sources", "codex:native-fixture"],
        &home,
    );
    let group = created["group_id"].as_str().expect("Group ID");
    cccc_core::fs::write_json(
        &fixture.provider.join("handoff.json"),
        &json!({"outcome":"rejected","wait":true,"after":{"queue":"ineligible"}}),
    )
    .expect("delayed queue rejection");
    let sent = fixture.request(&home, "message_send", json!({"group_id":group,"by":"user","to":["@all"],"message_mode":"send","text":"SYNTHETIC_STALE_HANDOFF"})).await;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !fixture.provider.join("handoff-started").exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "the original queue handoff must start"
        );
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let replacement = FixtureProcess(
        Command::new("/bin/sleep")
            .arg("30")
            .env_clear()
            .spawn()
            .expect("replacement host"),
    );
    fixture.hook(
        &home,
        "codex",
        replacement.0.id(),
        "native-fixture",
        "SessionStart",
    );
    let current =
        local_sessions::read(&layout, "codex:native-fixture").expect("replacement metadata");
    assert_ne!(old.host, current.host);
    assert_ne!(old.revision, current.revision);
    let since = fixture.native_calls().len();
    std::fs::write(fixture.provider.join("handoff-release"), "release")
        .expect("release old handoff");
    fixture
        .wait_receipt(
            &home,
            group,
            sent["event"]["id"].as_str().expect("event"),
            "failed",
        )
        .await;
    fixture.wait_qualification(&home, since, true).await;
    assert_eq!(
        local_sessions::read(&layout, &old.source_id).expect("current source"),
        current,
        "old endpoint evidence must not revoke a replacement"
    );
    let directory = fixture
        .request(&home, "local_session_list", json!({}))
        .await;
    assert_eq!(directory["sources"][0]["connected"], true);
    assert_eq!(
        fixture
            .native_calls()
            .iter()
            .filter(|call| call["method"] == "queue")
            .count(),
        1,
        "stale failure is never replayed"
    );
}

#[test]
fn opencode_native_plugin_enable_disable_preserves_jsonc_and_customizations() {
    let temp = tempfile::tempdir().expect("isolated Home");
    let root = temp.path();
    let bin = root.join("bin");
    let config = root.join("config/opencode");
    std::fs::create_dir_all(&bin).expect("bin");
    std::fs::create_dir_all(&config).expect("config");
    let original = "{\n // Native user settings and comments stay byte-for-byte unchanged.\n \"model\": \"fixture/model\",\n}\n";
    std::fs::write(config.join("opencode.jsonc"), original).expect("user config");
    let cli = bin.join("opencode");
    std::fs::write(&cli,r#"#!/bin/sh
if [ "$1" = --version ]; then echo 'opencode v2.1.4'; exit 0; fi
if [ "$1 $2" = 'debug paths' ]; then printf 'config     %s/opencode\n' "$XDG_CONFIG_HOME"; exit 0; fi
exit 91
"#).expect("official CLI fixture");
    std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o700)).expect("mode");
    let run = |action: &str| {
        Command::new(env!("CARGO_BIN_EXE_cccc"))
            .args(["sessions", action, "--runtime", "opencode"])
            .env_clear()
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .env("HOME", root)
            .env("USERPROFILE", root)
            .env("CCCC_HOME", root.join("cccc"))
            .env("CODEX_HOME", root.join("codex"))
            .env("XDG_CONFIG_HOME", root.join("config"))
            .current_dir(root)
            .output()
            .expect("configure")
    };
    for _ in 0..2 {
        let r = run("enable");
        assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    }
    let plugin = config.join("plugins/cccc-native");
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(plugin.join("package.json")).expect("manifest"))
            .expect("JSON");
    assert_eq!(manifest["name"], "@cccc/native-opencode");
    assert!(
        std::fs::read_to_string(plugin.join("tui.ts"))
            .expect("TUI")
            .contains("extension-bridge")
    );
    for _ in 0..2 {
        let r = run("disable");
        assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    }
    assert!(!plugin.exists());
    assert_eq!(
        std::fs::read_to_string(config.join("opencode.jsonc")).expect("preserved config"),
        original
    );
    assert!(run("enable").status.success());
    std::fs::write(plugin.join("tui.ts"), "user customization").expect("modified generated file");
    assert!(!run("disable").status.success());
    assert_eq!(
        std::fs::read_to_string(plugin.join("tui.ts")).expect("preserved customization"),
        "user customization"
    );
    assert_eq!(
        serde_json::from_slice::<Value>(
            &std::fs::read(root.join("cccc/state/local_sessions/opencode.disabled"))
                .expect("durable fence")
        )
        .expect("fence"),
        true
    );
    assert!(!run("enable").status.success());
    assert_eq!(
        std::fs::read_to_string(config.join("opencode.jsonc")).expect("native config"),
        original
    );
}
