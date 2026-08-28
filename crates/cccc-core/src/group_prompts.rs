use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;

use crate::{GroupStore, fs};

pub const PREAMBLE_FILENAME: &str = "CCCC_PREAMBLE.md";
pub const HELP_FILENAME: &str = "CCCC_HELP.md";
pub const DEFAULT_PREAMBLE_BODY: &str = "Startup:\n- On cold start or resume, use MCP tool `cccc_bootstrap`.\n- Call `cccc_help` only when you need a CCCC-specific route or a missing capability.";
pub const BUILTIN_HELP_MARKDOWN: &str = include_str!("../../../resources/cccc-help.md");
pub const MAX_PROMPT_BYTES: usize = 512 * 1024;
pub const CANONICAL_MESSAGE_DELIVERY_HEADING: &str = "Canonical Message Delivery";

pub struct PromptFile {
    pub path: PathBuf,
    pub found: bool,
    pub content: Option<String>,
}

pub fn read_preamble(store: &GroupStore, group_id: &str) -> io::Result<PromptFile> {
    read_prompt(store, group_id, PREAMBLE_FILENAME)
}

pub fn read_help(store: &GroupStore, group_id: &str) -> io::Result<PromptFile> {
    read_prompt(store, group_id, HELP_FILENAME)
}

fn read_prompt(store: &GroupStore, group_id: &str, filename: &str) -> io::Result<PromptFile> {
    let path = prompt_path(store, group_id, filename)?;
    if !path.is_file() {
        return Ok(PromptFile {
            path,
            found: false,
            content: None,
        });
    }
    let content = std::fs::read(&path).ok().map(|mut bytes| {
        bytes.truncate(MAX_PROMPT_BYTES);
        String::from_utf8_lossy(&bytes).into_owned()
    });
    Ok(PromptFile {
        path,
        found: true,
        content,
    })
}

pub fn write_preamble(store: &GroupStore, group_id: &str, content: &str) -> io::Result<()> {
    write_prompt(store, group_id, PREAMBLE_FILENAME, content)
}

pub fn write_help(store: &GroupStore, group_id: &str, content: &str) -> io::Result<()> {
    write_prompt(store, group_id, HELP_FILENAME, content)
}

fn write_prompt(
    store: &GroupStore,
    group_id: &str,
    filename: &str,
    content: &str,
) -> io::Result<()> {
    if content.len() > MAX_PROMPT_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("prompt content exceeds {MAX_PROMPT_BYTES} UTF-8 bytes"),
        ));
    }
    let path = prompt_path(store, group_id, filename)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    fs::atomic_write(&path, content.as_bytes())
}

pub fn delete_preamble(store: &GroupStore, group_id: &str) -> io::Result<()> {
    delete_prompt(store, group_id, PREAMBLE_FILENAME)
}

pub fn delete_help(store: &GroupStore, group_id: &str) -> io::Result<()> {
    delete_prompt(store, group_id, HELP_FILENAME)
}

fn delete_prompt(store: &GroupStore, group_id: &str, filename: &str) -> io::Result<()> {
    let path = prompt_path(store, group_id, filename)?;
    if !path.is_file() {
        return Ok(());
    }
    std::fs::remove_file(path)
}

fn prompt_path(store: &GroupStore, group_id: &str, filename: &str) -> io::Result<PathBuf> {
    store
        .group_dir(group_id)
        .map(|root| root.join("prompts").join(filename))
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HelpDocument {
    pub common: String,
    pub foreman: String,
    pub peer: String,
    pub voice_secretary: String,
    pub actor_notes: BTreeMap<String, String>,
    pub extra_tagged_blocks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HelpTag {
    Role(String),
    Actor(String, String),
    LegacyPet,
    VoiceSecretary,
}

fn normalized_lines(markdown: &str) -> Vec<&str> {
    markdown.lines().collect()
}

fn is_h2(line: &str) -> bool {
    line.strip_prefix("##").is_some_and(|rest| {
        !rest.starts_with('#') && rest.chars().next().is_some_and(char::is_whitespace)
    })
}

fn tag(line: &str) -> Option<HelpTag> {
    if !is_h2(line) {
        return None;
    }
    let heading = line[2..].trim();
    let folded = heading.to_ascii_lowercase();
    if let Some(rest) = folded.strip_prefix("@role:") {
        let role = rest.trim();
        if !role.is_empty() && !role.chars().any(char::is_whitespace) {
            return Some(HelpTag::Role(role.to_owned()));
        }
    }
    if folded.starts_with("@actor:") {
        let original = heading["@actor:".len()..].trim();
        let mut parts = original.splitn(2, char::is_whitespace);
        let actor_id = parts.next().unwrap_or_default().trim().to_owned();
        let inline = parts.next().unwrap_or_default().trim().to_owned();
        return Some(HelpTag::Actor(actor_id, inline));
    }
    if matches!(folded.as_str(), "@pet" | "@pet:") {
        return Some(HelpTag::LegacyPet);
    }
    if matches!(folded.as_str(), "@voice_secretary" | "@voice_secretary:") {
        return Some(HelpTag::VoiceSecretary);
    }
    None
}

fn sections(markdown: &str) -> Vec<String> {
    let normalized = markdown.replace("\r\n", "\n").replace('\r', "\n");
    if normalized.is_empty() {
        return vec![String::new()];
    }
    let mut out = Vec::new();
    let mut current = Vec::new();
    for line in normalized.split('\n') {
        if is_h2(line) && !current.is_empty() {
            out.push(current.join("\n"));
            current.clear();
        }
        current.push(line);
    }
    out.push(current.join("\n"));
    out
}

fn is_named_h2_section(section: &str, heading: &str) -> bool {
    section
        .trim()
        .lines()
        .next()
        .is_some_and(|line| line.trim().eq_ignore_ascii_case(&format!("## {heading}")))
}

pub fn canonical_message_delivery_section(markdown: &str) -> String {
    sections(markdown)
        .into_iter()
        .find(|section| is_named_h2_section(section, CANONICAL_MESSAGE_DELIVERY_HEADING))
        .map(|section| section.trim().to_owned())
        .expect("built-in CCCC help must contain the canonical message delivery section")
}

pub fn compose_effective_help_markdown(builtin: &str, overlay: &str) -> String {
    let canonical = canonical_message_delivery_section(builtin);
    let mut parts = vec![canonical];
    parts.extend(
        sections(overlay)
            .into_iter()
            .filter(|section| {
                !section.trim().is_empty()
                    && !is_named_h2_section(section, CANONICAL_MESSAGE_DELIVERY_HEADING)
            })
            .map(|section| section.trim().to_owned()),
    );
    parts.join("\n\n").trim().to_owned() + "\n"
}

pub fn parse_help_markdown(markdown: &str) -> HelpDocument {
    let mut parsed = HelpDocument::default();
    let mut common = Vec::new();
    for section in sections(markdown) {
        let raw = section.trim();
        if raw.is_empty() {
            continue;
        }
        let first = raw.lines().next().unwrap_or_default();
        let body = raw.lines().skip(1).collect::<Vec<_>>().join("\n");
        match tag(first) {
            Some(HelpTag::Role(role)) if role == "foreman" => {
                parsed.foreman = body.trim().to_owned();
            }
            Some(HelpTag::Role(role)) if role == "peer" => {
                parsed.peer = body.trim().to_owned();
            }
            Some(HelpTag::Actor(actor_id, inline)) if !actor_id.is_empty() => {
                let note = [inline.as_str(), body.as_str()]
                    .into_iter()
                    .filter(|value| !value.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n")
                    .trim()
                    .to_owned();
                parsed.actor_notes.insert(actor_id, note);
            }
            Some(HelpTag::VoiceSecretary) => {
                parsed.voice_secretary = body.trim().to_owned();
            }
            Some(HelpTag::Role(_)) | Some(HelpTag::Actor(_, _)) | Some(HelpTag::LegacyPet) => {
                parsed.extra_tagged_blocks.push(raw.to_owned())
            }
            None => common.push(raw.to_owned()),
        }
    }
    parsed.common = common.join("\n\n").trim().to_owned();
    parsed
}

pub fn build_help_markdown(document: &HelpDocument, actor_order: &[String]) -> String {
    let mut parts = Vec::new();
    if !document.common.trim().is_empty() {
        parts.push(document.common.trim().to_owned());
    }
    if !document.foreman.trim().is_empty() {
        parts.push(format!("## @role: foreman\n\n{}", document.foreman.trim()));
    }
    if !document.peer.trim().is_empty() {
        parts.push(format!("## @role: peer\n\n{}", document.peer.trim()));
    }
    if !document.voice_secretary.trim().is_empty() {
        parts.push(format!(
            "## @voice_secretary\n\n{}",
            document.voice_secretary.trim()
        ));
    }
    let mut ordered = Vec::new();
    for actor_id in actor_order {
        if !actor_id.trim().is_empty() && !ordered.contains(actor_id) {
            ordered.push(actor_id.clone());
        }
    }
    for actor_id in document.actor_notes.keys() {
        if !ordered.contains(actor_id) {
            ordered.push(actor_id.clone());
        }
    }
    for actor_id in ordered {
        let body = document
            .actor_notes
            .get(&actor_id)
            .map(String::as_str)
            .unwrap_or_default()
            .trim();
        if !body.is_empty() {
            parts.push(format!("## @actor: {actor_id}\n\n{body}"));
        }
    }
    parts.extend(
        document
            .extra_tagged_blocks
            .iter()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .map(str::to_owned),
    );
    let rendered = parts.join("\n\n").trim().to_owned();
    if rendered.is_empty() {
        String::new()
    } else {
        rendered + "\n"
    }
}

pub fn update_actor_help_note(
    markdown: &str,
    actor_id: &str,
    note: &str,
    actor_order: &[String],
) -> String {
    let mut document = parse_help_markdown(markdown);
    let actor_id = actor_id.trim();
    if !actor_id.is_empty() {
        if note.trim().is_empty() {
            document.actor_notes.remove(actor_id);
        } else {
            document
                .actor_notes
                .insert(actor_id.to_owned(), note.trim().to_owned());
        }
    }
    build_help_markdown(&document, actor_order)
}

pub fn select_help_markdown(
    markdown: &str,
    role: Option<&str>,
    actor_id: Option<&str>,
    include_voice_secretary: bool,
) -> String {
    if markdown.trim().is_empty() {
        return markdown.to_owned();
    }
    let role = role.unwrap_or_default().trim().to_ascii_lowercase();
    let actor_id = actor_id.unwrap_or_default().trim();
    let keep_trailing_newline = markdown.ends_with('\n');
    let mut out = Vec::new();
    let mut buffer = Vec::new();
    let mut active_tag: Option<HelpTag> = None;

    let flush = |out: &mut Vec<String>, buffer: &mut Vec<String>, active: &Option<HelpTag>| {
        let include = match active {
            None => true,
            Some(HelpTag::Role(value)) => role.is_empty() || role == value.to_ascii_lowercase(),
            Some(HelpTag::Actor(value, _)) => !actor_id.is_empty() && actor_id == value,
            Some(HelpTag::LegacyPet) => false,
            Some(HelpTag::VoiceSecretary) => include_voice_secretary,
        };
        if include {
            out.append(buffer);
        } else {
            buffer.clear();
        }
    };

    for mut line in normalized_lines(markdown).into_iter().map(str::to_owned) {
        if let Some(next_tag) = tag(&line) {
            flush(&mut out, &mut buffer, &active_tag);
            line = match &next_tag {
                HelpTag::Role(value) if value == "foreman" => "## Foreman".into(),
                HelpTag::Role(value) if value == "peer" => "## Peer".into(),
                HelpTag::Role(value) => format!("## Role: {value}"),
                HelpTag::Actor(_, _) => "## Notes for you".into(),
                HelpTag::LegacyPet => line,
                HelpTag::VoiceSecretary => "## Voice Secretary Operating Contract".into(),
            };
            active_tag = Some(next_tag);
            buffer.push(line);
        } else {
            if is_h2(&line) {
                flush(&mut out, &mut buffer, &active_tag);
                active_tag = None;
            }
            buffer.push(line);
        }
    }
    flush(&mut out, &mut buffer, &active_tag);
    let mut rendered = out.join("\n");
    if keep_trailing_newline {
        rendered.push('\n');
    }
    rendered
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_PROMPT_BYTES, compose_effective_help_markdown, parse_help_markdown, read_preamble,
        select_help_markdown, update_actor_help_note, write_preamble,
    };
    use crate::{GroupStore, HomeLayout};

    #[test]
    fn preamble_write_rejects_oversized_utf8_without_replacing_content() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let store = GroupStore::new(home).expect("store");
        let group = store.create("test", "").expect("group");
        write_preamble(&store, &group.group_id, "existing").expect("initial preamble");

        let error = write_preamble(&store, &group.group_id, &"界".repeat(MAX_PROMPT_BYTES))
            .expect_err("oversized UTF-8 preamble");

        assert!(error.to_string().contains("524288 UTF-8 bytes"));
        assert_eq!(
            read_preamble(&store, &group.group_id)
                .expect("read preamble")
                .content
                .as_deref(),
            Some("existing")
        );
    }

    #[test]
    fn actor_note_update_preserves_every_other_help_section() {
        let original = concat!(
            "# Help\n\n## Common\n\nShared.\n\n",
            "## @role: foreman\n\nLead.\n\n",
            "## @role: peer\n\nPeer.\n\n",
            "## @voice_secretary\n\nVoice.\n\n",
            "## @actor: old\n\nOld note.\n\n",
            "## @role: specialist\n\nKeep unknown.\n",
        );
        let updated = update_actor_help_note(
            original,
            "peer",
            "Use receipts.",
            &["old".into(), "peer".into()],
        );
        let parsed = parse_help_markdown(&updated);

        assert!(parsed.common.contains("Shared."));
        assert_eq!(parsed.foreman, "Lead.");
        assert_eq!(parsed.peer, "Peer.");
        assert_eq!(parsed.voice_secretary, "Voice.");
        assert_eq!(parsed.actor_notes["old"], "Old note.");
        assert_eq!(parsed.actor_notes["peer"], "Use receipts.");
        assert_eq!(
            parsed.extra_tagged_blocks,
            ["## @role: specialist\n\nKeep unknown."]
        );
    }

    #[test]
    fn effective_help_selects_role_actor_and_voice_blocks() {
        let markdown = concat!(
            "# Help\n\n",
            "## @role: foreman\n\nLead.\n\n",
            "## @role: peer\n\nPeer.\n\n",
            "## @actor: peer-a\n\nPrivate A.\n\n",
            "## @actor: peer-b\n\nPrivate B.\n\n",
            "## @voice_secretary\n\nVoice.\n",
        );

        let peer = select_help_markdown(markdown, Some("peer"), Some("peer-a"), false);
        assert!(peer.contains("## Peer"));
        assert!(peer.contains("Private A."));
        assert!(!peer.contains("Lead."));
        assert!(!peer.contains("Private B."));
        assert!(!peer.contains("Voice."));

        let voice = select_help_markdown(markdown, Some("voice_secretary"), Some("voice"), true);
        assert!(voice.contains("Voice Secretary Operating Contract"));
        assert!(!voice.contains("Private A."));
    }

    #[test]
    fn effective_help_keeps_builtin_delivery_contract_and_strips_overlay_copy() {
        let builtin =
            "# Help\n\n## Canonical Message Delivery\n\nUse Mail first.\n\n## Other\n\nBuilt-in.";
        let overlay =
            "# Group\n\n## Canonical Message Delivery\n\nAlways interrupt.\n\n## Notes\n\nLocal.";

        let effective = compose_effective_help_markdown(builtin, overlay);

        assert!(effective.contains("Use Mail first."));
        assert!(!effective.contains("Always interrupt."));
        assert!(effective.contains("Local."));
        assert_eq!(
            effective.matches("## Canonical Message Delivery").count(),
            1
        );
    }
}
