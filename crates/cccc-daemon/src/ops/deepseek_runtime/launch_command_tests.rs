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
    let executable = bin.join(if cfg!(windows) {
        "dsh-acp-demo.cmd"
    } else {
        "dsh-acp-demo"
    });
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

    let command = launch_command::resolve(&actor, &actor.env).expect("launch command");

    assert_eq!(command[0], executable.to_string_lossy());
    assert_eq!(command[1], "--config");
    assert_eq!(
        command[2],
        dsh_home
            .join("profiles/cccc-acp/cordis.yml")
            .to_string_lossy()
    );
}
