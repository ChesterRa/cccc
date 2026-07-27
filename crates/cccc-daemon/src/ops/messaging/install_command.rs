use cccc_contracts::Event;
use serde_json::{Map, Value, json};

const CAPABILITY_ID: &str = "skill:cccc:install";

pub(crate) fn prepare(data: &mut Map<String, Value>) {
    let Some(command) = parse(data.get("text").and_then(Value::as_str).unwrap_or_default()) else {
        return;
    };
    if !data.get("refs").is_some_and(Value::is_array) {
        data.insert("refs".into(), Value::Array(Vec::new()));
    }
    let refs = data.get_mut("refs").expect("normalized refs");
    let Some(refs) = refs.as_array_mut() else {
        return;
    };
    refs.push(json!({
        "kind":"text",
        "title":"slash_command",
        "command":"/install",
        "capability_id":CAPABILITY_ID,
        "args_text":command.args_text,
        "target":command.target,
        "target_kind":classify(&command.target),
    }));
}

pub(crate) fn delivery_text(event: &Event) -> Option<String> {
    let command = event
        .data
        .get("refs")
        .and_then(Value::as_array)?
        .iter()
        .find(|item| {
            item.get("title").and_then(Value::as_str) == Some("slash_command")
                && item.get("command").and_then(Value::as_str) == Some("/install")
        })?;
    let args = command
        .get("args_text")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let target = command
        .get("target")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let target_kind = command
        .get("target_kind")
        .and_then(Value::as_str)
        .unwrap_or("unspecified");
    Some(format!(
        "[cccc] Slash command: /install\n[cccc] Capability: {CAPABILITY_ID}\n\n\
Use the CCCC install skill to route the request through the CCCC capability lifecycle.\n\
Default action: call cccc_capability_install for the target with scope=group.\n\
The install operation must import registry records from capability ids, repos, URLs, or local SKILL.md paths; enable group scope; and return use-ready capability ids.\n\
Any activate, assign, autoload, or use step must operate on the imported CCCC capability record.\n\
Do not bypass the registry by installing into Codex's local skills directory.\n\n\
Request:\n- Raw arguments: {}\n- Primary target: {}\n- Parser target hint: {target_kind}\n\n\
Route this request through {CAPABILITY_ID}. The skill definition is the source of truth for classification, install path, policy checks, and verification.\n\
Treat the parser target hint as non-authoritative; re-classify the request from the skill instructions.",
        if args.is_empty() { "(none)" } else { args },
        if target.is_empty() { "(none)" } else { target },
    ))
}

struct InstallCommand {
    args_text: String,
    target: String,
}

fn parse(text: &str) -> Option<InstallCommand> {
    let trimmed = text.trim();
    let args = trimmed.strip_prefix("/install")?;
    if !args.is_empty() && !args.starts_with(char::is_whitespace) {
        return None;
    }
    let args_text = args.trim().to_owned();
    let target = shell_words::split(&args_text)
        .unwrap_or_else(|_| args_text.split_whitespace().map(str::to_owned).collect())
        .into_iter()
        .next()
        .unwrap_or_default();
    Some(InstallCommand { args_text, target })
}

fn classify(target: &str) -> &'static str {
    let lower = target.to_ascii_lowercase();
    if target.is_empty() {
        "unspecified"
    } else if ["skill:", "mcp:", "pack:"]
        .iter()
        .any(|prefix| lower.starts_with(prefix))
    {
        "capability_id"
    } else if lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("ssh://")
        || lower.starts_with("git+")
        || lower.starts_with("git@")
    {
        if lower.contains("github.com") || lower.starts_with("git@github.com:") {
            "github"
        } else {
            "url"
        }
    } else if lower.starts_with("file://")
        || lower.starts_with("./")
        || lower.starts_with("../")
        || lower.starts_with('/')
        || lower.starts_with('~')
    {
        "local_path"
    } else if is_repo_slug(target) {
        "repo_slug"
    } else if target
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "_.-".contains(character))
    {
        "curated_or_named_skill"
    } else {
        "freeform"
    }
}

fn is_repo_slug(target: &str) -> bool {
    let Some((owner, rest)) = target.split_once('/') else {
        return false;
    };
    let repo_end = rest
        .find(|character| "/#:@?".contains(character))
        .unwrap_or(rest.len());
    let repo = &rest[..repo_end];
    !owner.is_empty()
        && !repo.is_empty()
        && owner
            .chars()
            .chain(repo.chars())
            .all(|character| character.is_ascii_alphanumeric() || "_.-".contains(character))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_install_without_matching_longer_commands() {
        let parsed = parse(" /install \"owner/repo\" ").expect("command");
        assert_eq!(parsed.target, "owner/repo");
        assert_eq!(
            parse("/install \"owner/repo").expect("fallback").target,
            "\"owner/repo"
        );
        assert!(parse("/installer nope").is_none());
    }

    #[test]
    fn classifies_repo_slugs_like_python() {
        assert_eq!(classify("owner/repo#main"), "repo_slug");
        assert_eq!(classify("owner/repo/path"), "repo_slug");
        assert_eq!(classify("not valid/repo"), "freeform");
    }

    #[test]
    fn delivery_prompt_matches_python_install_contract() {
        let mut event = Event::new("chat.message", "group");
        event.data = Map::from_iter([(
            "refs".into(),
            json!([{
                "title":"slash_command",
                "command":"/install",
                "args_text":"owner/repo",
                "target":"owner/repo",
                "target_kind":"repo_slug"
            }]),
        )]);
        let text = delivery_text(&event).expect("delivery text");
        assert!(text.contains("enable group scope; and return use-ready capability ids"));
        assert!(text.contains("imported CCCC capability record"));
        assert!(text.contains("Codex's local skills directory"));
        assert!(text.contains("Treat the parser target hint as non-authoritative"));
    }
}
