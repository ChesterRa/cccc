use cccc_contracts::Actor;
use std::path::Path;

pub(super) fn resolve(actor: &Actor) -> std::io::Result<Vec<String>> {
    let managed = actor
        .command
        .first()
        .and_then(|value| Path::new(value).file_stem())
        .and_then(|value| value.to_str())
        .is_some_and(|value| matches!(value.to_ascii_lowercase().as_str(), "dsh" | "dsh-acp-demo"));
    if !managed {
        return Ok(actor.command.clone());
    }

    let dsh_home = cccc_runtime::deepseek_home(&actor.env)
        .ok_or_else(|| std::io::Error::other("DSH_HOME cannot be inferred"))?;
    let executable = cccc_runtime::resolve_executable_in_path(
        "dsh-acp-demo",
        actor.env.get("PATH").map(String::as_str),
    )
    .ok_or_else(|| std::io::Error::other("deepseek executable not found: dsh-acp-demo"))?;
    Ok(vec![
        executable.to_string_lossy().into_owned(),
        "--config".into(),
        dsh_home
            .join("profiles/cccc-acp/cordis.yml")
            .to_string_lossy()
            .into_owned(),
    ])
}
