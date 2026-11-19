use super::*;
use cccc_contracts::ActorRuntime;
use std::fs;

#[test]
fn managed_launch_uses_the_platform_resolved_acp_executable() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cccc_home = temp.path().join("cccc-home");
    let dsh_home = cccc_home
        .join("runtimes/deepseek")
        .join(cccc_contracts::DEEPSEEK_RELEASE_VERSION);
    let bin = dsh_home.join("node_modules/.bin");
    fs::create_dir_all(&bin).expect("bin");
    let executable = bin.join(if cfg!(windows) { "dsh.cmd" } else { "dsh" });
    fs::write(&executable, b"launcher").expect("launcher");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).expect("permissions");
    }

    let mut actor = Actor::new("deepseek");
    actor.runtime = ActorRuntime::Deepseek;
    actor.command = vec!["dsh-acp-demo".into()];
    actor
        .env
        .insert("CCCC_HOME".into(), cccc_home.to_string_lossy().into_owned());
    actor.env.insert(
        "PATH".into(),
        std::env::join_paths([&bin])
            .expect("path")
            .to_string_lossy()
            .into_owned(),
    );

    let state = temp.path().join("actor-state");
    let workspace = temp.path().join("project");
    let cccc = temp.path().join("cccc");
    let mut env = actor.env.clone();
    env.insert("CCCC_GROUP_ID".into(), "g_test".into());
    env.insert("CCCC_ACTOR_ID".into(), "member".into());
    let launch = launch_command::prepare(&actor, &mut env, &state, &workspace, Some(&cccc))
        .expect("managed launch");
    assert_eq!(launch.command[0], executable.to_string_lossy());
    assert_eq!(&launch.command[1..4], ["--profile", "acp", "--patch"]);
    assert_eq!(launch.cwd, state.join("harness"));
    assert_eq!(env["DSH_HOME"], launch.cwd.to_string_lossy());
    assert_eq!(
        env["DEEPSEEK_BASE_URL"],
        "https://api.deepseek.com/anthropic"
    );
    assert_eq!(launch.mcp_servers[0]["command"], serde_json::json!(cccc));
    assert_eq!(launch.mcp_servers[0]["env"][2]["value"], "member");
    assert!(
        fs::read_to_string(&launch.command[4])
            .expect("Messages overlay")
            .contains("provider: deepseek-official")
    );

    env.insert(
        "DEEPSEEK_BASE_URL".into(),
        "https://api.deepseek.com/v1/".into(),
    );
    launch_command::prepare(&actor, &mut env, &state, &workspace, Some(&cccc))
        .expect("saved official API base");
    assert_eq!(
        env["DEEPSEEK_BASE_URL"],
        "https://api.deepseek.com/anthropic"
    );

    env.insert(
        "DEEPSEEK_BASE_URL".into(),
        "https://gateway.example/v1".into(),
    );
    let launch = launch_command::prepare(&actor, &mut env, &state, &workspace, Some(&cccc))
        .expect("custom gateway");
    let patch = fs::read_to_string(&launch.command[4]).expect("Chat overlay");
    assert!(patch.contains("api: openai-completions"));
    assert!(patch.contains(&format!(
        "maxTokens: {}",
        cccc_contracts::DEEPSEEK_MAX_OUTPUT_TOKENS
    )));
    assert!(!patch.contains("gateway.example"));
    assert_eq!(env["DEEPSEEK_BASE_URL"], "https://gateway.example/v1");
}
