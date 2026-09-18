use super::*;
use std::os::unix::fs::PermissionsExt;

fn isolated(test_name: &str, test: impl FnOnce()) {
    let name = format!(
        "{}::{test_name}",
        module_path!().split_once("::").expect("module").1
    );
    if std::env::var("CCCC_SECRETARY_START_FIXTURE").as_deref() == Ok(&name) {
        test();
        return;
    }
    // Resolve the MCP launcher and isolate Grok's native registry without an
    // installed CCCC or process-global changes in the parallel test harness.
    let temp = tempfile::tempdir().expect("launcher directory");
    let launcher = temp.path().join("cccc");
    std::fs::write(&launcher, "#!/bin/sh\nexit 1\n").expect("unused MCP launcher");
    std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o700))
        .expect("executable");
    let output = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .args(["--exact", &name, "--nocapture"])
        .env("CCCC_SECRETARY_START_FIXTURE", &name)
        .env("CCCC_LAUNCHER_PATH", launcher)
        .env("GROK_HOME", temp.path().join("grok-home"))
        .output()
        .expect("isolated regression");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "{}\n{}",
        stdout,
        String::from_utf8_lossy(&output.stderr)
    );
}

struct ManagedSecretary {
    home: HomeLayout,
    group_id: String,
    _temp: tempfile::TempDir,
}

impl ManagedSecretary {
    fn new() -> Self {
        // Keep Grok's Unix socket below its path limit on macOS as well.
        let temp = tempfile::tempdir_in("/tmp").expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        let workspace = temp.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace");
        let executable = temp.path().join("grok");
        // Exercise the real managed launch and terminal attachment without a
        // provider account or model turn. Unknown requests fail the fixture.
        std::fs::write(
            &executable,
            r#"#!/bin/sh
if [ "$1" = inspect ] || [ "$1" = mcp ]; then
  exec python3 - "$@" <<'PY'
import json, os, pathlib, sys
path = pathlib.Path(os.environ['GROK_HOME']) / 'config.toml'
args = sys.argv[1:]
if args == ['inspect', '--json']:
    entries = [{'name': 'cccc', 'transport': 'stdio', 'target': os.environ['CCCC_CLI'],
                'source': {'type': 'configToml', 'path': str(path)}}] if path.exists() else []
    print(json.dumps({'mcpServers': entries}))
elif args == ['mcp', 'list', '--json']:
    assert path.exists(), 'MCP must be registered before querying effective configuration'
    print(json.dumps([{'name': 'cccc', 'command': os.environ['CCCC_CLI'],
                       'args': ['mcp'], 'enabled': True, 'scope': 'user'}]))
else:
    assert args == ['mcp', 'add', '--scope', 'user', 'cccc', '--', '${CCCC_CLI:-cccc}', 'mcp']
    assert not path.exists(), 'matching registration must not be rewritten'
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("[mcp_servers.cccc]\ncommand='${CCCC_CLI:-cccc}'\nargs=['mcp']\n")
PY
fi
case " $* " in
  *" stdio "*)
    printf '%s' "$$" > "$(dirname "$0")/probe-pid"
    if [ "$AUDIT_MARKER" = profile ]; then touch "$(dirname "$0")/profile-env-applied"; fi
    ;;
esac
case " $* " in
  *" leader "*) exec sleep 60 ;;
esac
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":true}}}\n' "$id"
      ;;
    *'"method":"session/new"'*|*'"method":"session/load"'*)
      [ -f "$GROK_HOME/config.toml" ] || exit 1
      case "$line" in *'"mcpServers":[]'*) ;; *) exit 1 ;; esac
      if [ -f "$(dirname "$0")/reject-session" ]; then
        printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32000,"message":"fixture session rejected"}}\n' "$id"
      else
        printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"01a0623c-19b3-7ec3-b777-95e24279ec67"}}\n' "$id"
      fi
      ;;
    *) exit 1 ;;
  esac
done
"#,
        )
        .expect("write fake provider");
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
            .expect("executable");
        let store = GroupStore::new(home.clone()).expect("store");
        let group = store.create("voice", "").expect("group");
        store
            .mutate(&group.group_id, |doc| {
                let mut foreman = Actor::new("foreman");
                foreman.role = Some(ActorRole::Foreman);
                foreman.runtime = ActorRuntime::Grok;
                foreman.command = vec![executable.to_string_lossy().into_owned()];
                doc.actors.push(foreman);
                doc.scopes.push(Scope {
                    scope_key: "scope".into(),
                    url: workspace.to_string_lossy().into_owned(),
                    label: "workspace".into(),
                    git_remote: String::new(),
                });
                doc.active_scope_key = "scope".into();
                doc.running = true;
                Ok(())
            })
            .expect("running group");
        Self {
            home,
            group_id: group.group_id,
            _temp: temp,
        }
    }

    fn enable(&self) -> DaemonResponse {
        call(
            &self.home,
            "assistant_settings_update",
            json!({"group_id":self.group_id,"patch":{"enabled":true}}),
        )
    }

    fn secret_files(&self) -> std::collections::BTreeMap<std::ffi::OsString, Vec<u8>> {
        let directory = self
            .home
            .root()
            .join("state/secrets/actors")
            .join(&self.group_id);
        std::fs::read_dir(directory)
            .expect("secret directory")
            .map(|entry| entry.expect("secret entry"))
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
            .map(|entry| {
                (
                    entry.file_name(),
                    std::fs::read(entry.path()).expect("secret file"),
                )
            })
            .collect()
    }

    fn assert_running(&self, response: &DaemonResponse) -> u64 {
        assert!(response.ok, "startup failed: {:?}", response.error);
        let assistant = &response.result["assistant"];
        assert_eq!(assistant["enabled"], true);
        assert_eq!(assistant["lifecycle"], "running");
        assert_eq!(assistant["health"]["actor"]["running"], true);
        let pid = assistant["health"]["actor"]["pid"].as_u64().expect("pid");
        let store = GroupStore::new(self.home.clone()).expect("store");
        let group = store.load(&self.group_id).expect("group");
        let actor = group
            .actors
            .iter()
            .find(|a| a.id == "voice-secretary")
            .expect("actor retained");
        assert!(
            actor.enabled,
            "successful explicit enable must permit input delivery"
        );
        assert!(
            cccc_runtime::status(&self.group_id, &actor.id)
                .expect("terminal")
                .running
        );
        pid
    }
}

impl Drop for ManagedSecretary {
    fn drop(&mut self) {
        let stopped = call(
            &self.home,
            "assistant_settings_update",
            json!({"group_id":self.group_id,"patch":{"enabled":false}}),
        );
        if !std::thread::panicking() {
            assert!(stopped.ok, "fixture shutdown failed: {:?}", stopped.error);
            assert!(
                !cccc_runtime::status(&self.group_id, "voice-secretary")
                    .is_ok_and(|status| status.running)
            );
        }
    }
}

#[test]
fn managed_voice_secretary_starts_and_repeated_enable_preserves_the_session() {
    isolated(
        "managed_voice_secretary_starts_and_repeated_enable_preserves_the_session",
        || {
            let fixture = ManagedSecretary::new();
            let started = fixture.enable();
            let pid = fixture.assert_running(&started);
            assert_eq!(started.result["actor_started"], true);
            let again = fixture.enable();
            assert_eq!(fixture.assert_running(&again), pid);
            let state = ok(
                &fixture.home,
                "assistant_index",
                json!({"group_id":fixture.group_id}),
            );
            assert_eq!(fixture.assert_running(&state), pid);
        },
    );
}

#[test]
fn managed_voice_secretary_settings_do_not_restart_but_explicit_enable_does() {
    isolated(
        "managed_voice_secretary_settings_do_not_restart_but_explicit_enable_does",
        || {
            let fixture = ManagedSecretary::new();
            fixture.assert_running(&fixture.enable());
            ok(
                &fixture.home,
                "actor_stop",
                json!({"group_id":fixture.group_id,"actor_id":"voice-secretary","by":"user"}),
            );
            let store = GroupStore::new(fixture.home.clone()).expect("store");
            store
                .mutate(&fixture.group_id, |doc| {
                    doc.running = true;
                    Ok(())
                })
                .expect("keep group active");
            let saved = ok(
                &fixture.home,
                "assistant_settings_update",
                json!({"group_id":fixture.group_id,"patch":{"config":{"recognition_language":"zh-CN"}}}),
            );
            assert_eq!(saved.result["actor_started"], false);
            assert_eq!(
                saved.result["assistant"]["health"]["actor"]["running"],
                false
            );
            assert_eq!(saved.result["assistant"]["lifecycle"], "failed");
            let stopped = store.load(&fixture.group_id).expect("stopped group");
            assert!(
                !stopped
                    .actors
                    .iter()
                    .find(|a| a.id == "voice-secretary")
                    .expect("actor retained")
                    .enabled
            );
            fixture.assert_running(&fixture.enable());
        },
    );
}

#[test]
fn managed_voice_secretary_rejected_start_restores_settings_actor_and_secrets() {
    isolated(
        "managed_voice_secretary_rejected_start_restores_settings_actor_and_secrets",
        || {
            let fixture = ManagedSecretary::new();
            let store = GroupStore::new(fixture.home.clone()).expect("store");
            // Verify both creating an assistant and restarting an existing one.
            for existing in [false, true] {
                if existing {
                    fixture.assert_running(&fixture.enable());
                    ok(
                        &fixture.home,
                        "actor_stop",
                        json!({"group_id":fixture.group_id,"actor_id":"voice-secretary","by":"user"}),
                    );
                    store
                        .mutate(&fixture.group_id, |doc| {
                            doc.running = true;
                            Ok(())
                        })
                        .expect("active group");
                }
                let owner = if existing {
                    "voice-secretary"
                } else {
                    "foreman"
                };
                ok(
                    &fixture.home,
                    "actor_env_private_update",
                    json!({"group_id":fixture.group_id,"actor_id":owner,"set":{"VOICE_FIXTURE_SECRET":"fixture-only"}}),
                );
                let before = store.load(&fixture.group_id).expect("before");
                let secrets_before = fixture.secret_files();
                let rejection = fixture._temp.path().join("reject-session");
                std::fs::write(&rejection, "").expect("reject session");
                let failed = fixture.enable();
                let error = failed.error.expect("failed startup");
                assert_eq!(error.code, "voice_secretary_start_failed");
                assert!(
                    error.message.contains("fixture session rejected"),
                    "{}",
                    error.message
                );
                std::fs::remove_file(rejection).expect("allow next session");
                let after = store.load(&fixture.group_id).expect("after");
                assert_eq!(
                    after.extra.get("assistants"),
                    before.extra.get("assistants")
                );
                assert_eq!(after.actors, before.actors);
                assert!(
                    fixture.secret_files() == secrets_before,
                    "rollback must preserve private env exactly"
                );
                assert!(
                    !cccc_runtime::status(&fixture.group_id, "voice-secretary")
                        .is_ok_and(|status| status.running)
                );
                let state = ok(
                    &fixture.home,
                    "assistant_index",
                    json!({"group_id":fixture.group_id}),
                );
                assert_eq!(
                    state.result["assistant"]["health"]["actor"]["running"],
                    false
                );
            }
        },
    );
}

#[test]
fn managed_actor_profile_runtime_transitions_stop_the_registered_backend() {
    isolated(
        "managed_actor_profile_runtime_transitions_stop_the_registered_backend",
        || {
            let fixture = ManagedSecretary::new();
            GroupStore::new(fixture.home.clone())
                .expect("store")
                .mutate(&fixture.group_id, |doc| {
                    doc.actors[0].id = "lead-1".into();
                    Ok(())
                })
                .expect("fixture actor id");
            struct Cleanup<'a>(&'a ManagedSecretary);
            impl Drop for Cleanup<'_> {
                fn drop(&mut self) {
                    let _ = call(
                        &self.0.home,
                        "group_stop",
                        json!({"group_id":self.0.group_id}),
                    );
                }
            }
            let _cleanup = Cleanup(&fixture);
            let base = || json!({"group_id":fixture.group_id,"actor_id":"lead-1","by":"user"});
            let alive = |pid: u32| {
                std::process::Command::new("kill")
                    .args(["-0", &pid.to_string()])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .expect("kill probe")
                    .success()
            };
            let managed_pid = || {
                std::fs::read_to_string(fixture._temp.path().join("probe-pid"))
                    .expect("pid")
                    .parse::<u32>()
                    .expect("pid number")
            };
            let command = json!([fixture._temp.path().join("grok").to_string_lossy()]);
            ok(
                &fixture.home,
                "actor_env_private_update",
                json!({"group_id":fixture.group_id,"actor_id":"lead-1","set":{"AUDIT_MARKER":"old"}}),
            );
            ok(
                &fixture.home,
                "actor_profile_upsert",
                json!({"profile_id":"managed","name":"managed","runtime":"grok","command":command}),
            );
            ok(
                &fixture.home,
                "actor_profile_secret_update",
                json!({"profile_id":"managed","set":{"AUDIT_MARKER":"profile"}}),
            );
            ok(
                &fixture.home,
                "actor_update",
                json!({"group_id":fixture.group_id,"actor_id":"lead-1","profile_id":"managed","capability_autoload":[]}),
            );
            ok(&fixture.home, "actor_start", base());
            let old_pid = managed_pid();
            assert!(alive(old_pid));
            assert!(
                fixture._temp.path().join("profile-env-applied").exists(),
                "launch uses Profile environment"
            );
            ok(
                &fixture.home,
                "actor_update",
                json!({"group_id":fixture.group_id,"actor_id":"lead-1","profile_action":"convert_to_custom","capability_autoload":[]}),
            );
            let use_pty = || {
                ok(
                    &fixture.home,
                    "actor_update",
                    json!({"group_id":fixture.group_id,"actor_id":"lead-1","runtime":"custom","command":["sleep","60"]}),
                )
            };
            use_pty();
            ok(&fixture.home, "actor_start", base());
            assert_eq!(
                managed_pid(),
                old_pid,
                "start is idempotent until explicit restart"
            );
            assert!(alive(old_pid));
            ok(&fixture.home, "actor_restart", base());
            assert!(
                !alive(old_pid),
                "old managed process must be gone before PTY startup"
            );
            let pty = cccc_runtime::status(&fixture.group_id, "lead-1").expect("PTY");
            assert!(pty.running);
            ok(
                &fixture.home,
                "actor_update",
                json!({"group_id":fixture.group_id,"actor_id":"lead-1","profile_id":"managed"}),
            );
            ok(&fixture.home, "actor_start", base());
            assert_eq!(
                cccc_runtime::status(&fixture.group_id, "lead-1")
                    .expect("retained PTY")
                    .pid,
                pty.pid
            );
            ok(&fixture.home, "actor_restart", base());
            assert!(!alive(pty.pid.expect("PTY pid")));
            let second = managed_pid();
            assert!(alive(second));
            ok(
                &fixture.home,
                "actor_update",
                json!({"group_id":fixture.group_id,"actor_id":"lead-1","profile_action":"convert_to_custom"}),
            );
            use_pty();
            ok(&fixture.home, "actor_stop", base());
            assert!(
                !alive(second),
                "stop also follows actual ownership after saving a backend switch"
            );
        },
    );
}
