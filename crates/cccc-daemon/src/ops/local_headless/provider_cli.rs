use cccc_contracts::{Actor, ActorRuntime};

pub(super) fn uses_managed_provider_cli(actor: &Actor) -> bool {
    let command = base_command(actor);
    is_provider_binary(actor, &command)
}

pub(super) fn base_command(actor: &Actor) -> Vec<String> {
    if actor.command.is_empty() {
        cccc_runtime::default_command(actor.runtime)
    } else {
        actor.command.clone()
    }
}

pub(super) fn is_provider_binary(actor: &Actor, command: &[String]) -> bool {
    let expected = match actor.runtime {
        ActorRuntime::Codex => "codex",
        ActorRuntime::Claude => "claude",
        ActorRuntime::Grok => "grok",
        ActorRuntime::Opencode => "opencode",
        ActorRuntime::Kilo => "kilo",
        _ => return false,
    };
    command.first().is_some_and(|program| {
        std::path::Path::new(program)
            .file_stem()
            .and_then(|value| value.to_str())
            == Some(expected)
    })
}
