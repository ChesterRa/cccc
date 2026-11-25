//! Startup input policy for ordinary Actor terminals. A ready screen releases
//! only this Session; it is not a provider admission receipt or native consent.
use cccc_contracts::ActorRuntime;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_ONBOARDING_BYTES: u64 = 64 * 1024;

/// Hermes is not listed: its terminal views the member's ACP session, and
/// CCCC delivers through ACP without typing into it.
pub fn required(runtime: ActorRuntime) -> bool {
    matches!(
        runtime,
        ActorRuntime::Kiro
            | ActorRuntime::Amp
            | ActorRuntime::Auggie
            | ActorRuntime::Cursor
            | ActorRuntime::Devin
            | ActorRuntime::Droid
            | ActorRuntime::Copilot
            | ActorRuntime::Cline
            | ActorRuntime::Kimi
    )
}

/// Runtimes whose primary PTY can hold automatic first input behind the
/// startup-input gate (unconditionally or, for Antigravity, until first-run
/// onboarding completes in the Actor's effective HOME).
pub(crate) fn supports_gate(runtime: ActorRuntime) -> bool {
    required(runtime) || runtime == ActorRuntime::Antigravity
}

/// Resolve Antigravity's effective user home directory for the child process's
/// environment. Shared by MCP setup and the first-run onboarding gate so both
/// inspect the exact same HOME on Unix and Windows (including mixed-case
/// Windows overrides such as `userprofile` over inherited `USERPROFILE`).
pub fn resolve_antigravity_home(
    cwd: &Path,
    env: &BTreeMap<String, String>,
    clear_environment: bool,
) -> Option<PathBuf> {
    let inherited = if clear_environment {
        Vec::new()
    } else {
        crate::unicode_environment().collect()
    };
    resolve_antigravity_home_from(cwd, env, inherited, cfg!(windows))
}

pub fn resolve_antigravity_home_from(
    cwd: &Path,
    env: &BTreeMap<String, String>,
    inherited: impl IntoIterator<Item = (String, String)>,
    windows: bool,
) -> Option<PathBuf> {
    let home = if windows {
        let mut effective = BTreeMap::new();
        for (key, value) in inherited {
            effective.insert(key.to_ascii_uppercase(), value);
        }
        for (key, value) in env {
            effective.insert(key.to_ascii_uppercase(), value.clone());
        }
        ["USERPROFILE", "HOME"].into_iter().find_map(|key| {
            effective
                .get(key)
                .filter(|value| !value.is_empty())
                .cloned()
        })
    } else {
        let mut effective: BTreeMap<String, String> = inherited.into_iter().collect();
        for (key, value) in env {
            effective.insert(key.clone(), value.clone());
        }
        ["HOME", "USERPROFILE"].into_iter().find_map(|key| {
            effective
                .get(key)
                .filter(|value| !value.is_empty())
                .cloned()
        })
    }?;
    let home = Path::new(&home);
    Some(if home.is_absolute() {
        home.to_owned()
    } else {
        cwd.join(home)
    })
}

/// Resolve Antigravity's first-run onboarding state file for the child
/// process's effective environment. Read-only: CCCC never creates or modifies
/// this file so Terms of Service consent stays strictly user-driven.
pub(crate) fn antigravity_onboarding_path(
    cwd: &Path,
    env: &BTreeMap<String, String>,
    clear_environment: bool,
) -> Option<PathBuf> {
    resolve_antigravity_home(cwd, env, clear_environment).map(|home| {
        home.join(".gemini")
            .join("antigravity-cli")
            .join("cache")
            .join("onboarding.json")
    })
}

fn open_regular_onboarding_file(path: &Path) -> Option<std::fs::File> {
    #[cfg(unix)]
    let file = {
        use std::os::unix::fs::OpenOptionsExt;
        // O_NONBLOCK prevents blocking indefinitely if path is a FIFO/named pipe.
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(nix::libc::O_NONBLOCK)
            .open(path)
            .ok()?
    };
    #[cfg(not(unix))]
    let file = {
        let meta = std::fs::metadata(path).ok()?;
        if !meta.is_file() || meta.len() > MAX_ONBOARDING_BYTES {
            return None;
        }
        std::fs::File::open(path).ok()?
    };
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.len() > MAX_ONBOARDING_BYTES {
        return None;
    }
    Some(file)
}

/// True only when Antigravity's onboarding state file is a regular file,
/// parses as JSON, and explicitly reports `"onboardingComplete": true`.
pub(crate) fn antigravity_onboarding_complete(path: &Path) -> bool {
    let Some(file) = open_regular_onboarding_file(path) else {
        return false;
    };
    let mut bytes = Vec::new();
    let Ok(read) = file.take(MAX_ONBOARDING_BYTES + 1).read_to_end(&mut bytes) else {
        return false;
    };
    if read as u64 > MAX_ONBOARDING_BYTES {
        return false;
    }
    serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()
        .and_then(|value| {
            value
                .get("onboardingComplete")
                .and_then(serde_json::Value::as_bool)
        })
        == Some(true)
}

/// True when the visible terminal still displays Antigravity's interactive
/// first-run onboarding wizard (e.g. before repaint after `[Done]`, or when
/// onboarding was completed in another shell while this PTY stays on the wizard).
pub(crate) fn antigravity_onboarding_wizard_visible(rows: &[String]) -> bool {
    let compact = compact_rows(rows);
    [
        "Welcome to Antigravity CLI",
        "Choose your color scheme",
        "Terms of Service & Data Use",
    ]
    .into_iter()
    .any(|marker| compact.contains(&strip_whitespace(marker)))
}

/// After `onboardingComplete` becomes true for a gated fresh-HOME Antigravity
/// session, require positive evidence of Antigravity's bordered chat input
/// region with no active wizard or trust prompt before releasing automatic
/// input. Unknown/redesigned screens stay held until explicit user release.
pub(crate) fn antigravity_screen_ready(rows: &[String]) -> bool {
    let lines = rows.iter().map(|line| line.trim()).collect::<Vec<_>>();
    let compact = compact_rows(rows);
    let has = |text: &str| compact.contains(&strip_whitespace(text));
    bordered_input(&lines, ">")
        && !antigravity_onboarding_wizard_visible(rows)
        && !has("Trust this workspace")
        && !has("Trust this folder")
        && !has("Do you trust")
}

fn compact_rows(rows: &[String]) -> String {
    rows.iter()
        .flat_map(|line| line.chars())
        .filter(|ch| !ch.is_whitespace())
        .collect()
}

fn strip_whitespace(text: &str) -> String {
    text.chars().filter(|ch| !ch.is_whitespace()).collect()
}

/// Recognize the input region, rather than interpreting a banner, native cursor,
/// MCP handshake or quiet output as readiness. Unknown screens stay pending and
/// can be released explicitly by the user for this Session.
pub(crate) fn ready(runtime: ActorRuntime, rows: &[String]) -> bool {
    let lines = rows.iter().map(|line| line.trim()).collect::<Vec<_>>();
    // Native prompts can wrap after a terminal resize. Whitespace must not
    // make an active known modal disappear behind an intact input region.
    let compact = lines
        .iter()
        .flat_map(|line| line.chars())
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    let has = |text: &str| {
        compact.contains(
            &text
                .chars()
                .filter(|ch| !ch.is_whitespace())
                .collect::<String>(),
        )
    };
    let input = |text: &str| lines.contains(&text);
    match runtime {
        ActorRuntime::Kiro => {
            has("ask a question or describe a task")
                && lines.iter().any(|line| line.starts_with('›'))
                && !has("Trust this workspace?")
                && !has("Kiro is running in trust all tools mode")
        }
        ActorRuntime::Amp => {
            has("ctrl+o for commands")
                && !has("Space to continue")
                && !has("Enter to get started")
                && empty_rounded_box(&lines)
        }
        ActorRuntime::Auggie => {
            bordered_input(&lines, "›") && !has("Workspace Indexing Permission")
        }
        ActorRuntime::Cursor => {
            // Cursor can retain the old trust box above its newly drawn input.
            // That residue must not override the complete new input region.
            input("→ Plan, search, build anything")
        }
        ActorRuntime::Devin => {
            has("Ask Devin")
                && lines.iter().any(|line| line.starts_with('❭'))
                && !has("Do you trust the authors of this directory?")
        }
        ActorRuntime::Droid => {
            // Droid picks its empty-input hint at random ("Try …" or "Type …").
            // The left part of its footer changes with help, update state and
            // language; the MCP status after the separator does not.
            lines
                .iter()
                .any(|line| line.starts_with("> Try \"") || line.starts_with("> Type \""))
                && has("| MCP")
                && !has("Trust this folder?")
        }
        ActorRuntime::Copilot => bordered_input(&lines, "❯") && !has("Confirm folder trust"),
        ActorRuntime::Cline => {
            // The empty input's hint follows Cline's act or plan mode.
            (bordered_input(&lines, "❯ What can I do for you?")
                || bordered_input(&lines, "❯ Plan something..."))
                && !has("Press Enter to open, any other key to close")
        }
        ActorRuntime::Kimi => {
            lines.windows(3).any(|frame| {
                frame[0].starts_with('╭')
                    && frame[1].starts_with("│ >")
                    && frame[1]
                        .trim_start_matches("│ >")
                        .trim_end_matches('│')
                        .trim()
                        .is_empty()
                    && frame[2].starts_with('╰')
            }) && !has("Trust this folder?")
        }
        _ => false,
    }
}

/// A rounded box with only empty rows between its borders. Amp draws its input
/// box with one or more such rows, depending on the terminal.
fn empty_rounded_box(lines: &[&str]) -> bool {
    lines.iter().enumerate().any(|(top, line)| {
        if !(line.starts_with('╭') && line.ends_with('╮')) {
            return false;
        }
        let inner = lines[top + 1..]
            .iter()
            .take_while(|row| {
                row.starts_with('│')
                    && row.ends_with('│')
                    && row.trim_matches('│').trim().is_empty()
            })
            .count();
        inner > 0
            && lines
                .get(top + 1 + inner)
                .is_some_and(|bottom| bottom.starts_with('╰') && bottom.ends_with('╯'))
    })
}

fn bordered_input(lines: &[&str], input: &str) -> bool {
    lines.windows(3).any(|region| {
        horizontal_rule(region[0]) && region[1] == input && horizontal_rule(region[2])
    })
}

fn horizontal_rule(line: &str) -> bool {
    line.chars().take(8).count() == 8 && line.chars().all(|ch| matches!(ch, '─' | '━'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(runtime: ActorRuntime, text: &str) -> bool {
        ready(
            runtime,
            &text.lines().map(str::to_owned).collect::<Vec<_>>(),
        )
    }

    #[test]
    fn startup_options_and_one_time_promotions_are_not_input() {
        for (runtime, text) in [
            (
                ActorRuntime::Kiro,
                "Trust this workspace?\n❯ Trust, continue\nDon't trust, exit",
            ),
            (
                ActorRuntime::Kiro,
                "Warning: Kiro is running in trust all tools mode\n❯ No, exit\nYes, I accept",
            ),
            (ActorRuntime::Amp, "Welcome to Amp\n1/5 · Space to continue"),
            (
                ActorRuntime::Amp,
                "Threads have no length limit\n5/5 · Enter to get started",
            ),
            (
                ActorRuntime::Auggie,
                "Workspace Indexing Permission\n→ [1] Always index this workspace",
            ),
            (
                ActorRuntime::Cursor,
                "Workspace Trust Required\n▶ [a] Trust this workspace\n[q] Quit",
            ),
            (
                ActorRuntime::Devin,
                "Do you trust the authors of this directory?\n❭ 1 Yes, trust",
            ),
            (
                ActorRuntime::Droid,
                "Trust this folder?\n> 1. Trust this folder",
            ),
            (ActorRuntime::Copilot, "Confirm folder trust\n❯ 1. Yes"),
            (
                ActorRuntime::Cline,
                "Try ClinePass\nPress Enter to open, any other key to close",
            ),
            (
                ActorRuntime::Kimi,
                "Trust this folder?\n❯ Trust and continue\nExit",
            ),
        ] {
            assert!(!screen(runtime, text), "{runtime:?}");
            assert!(!screen(runtime, ""), "before first paint: {runtime:?}");
        }
    }

    #[test]
    fn input_regions_release_the_current_baselines_without_model_names() {
        for (runtime, text) in [
            (ActorRuntime::Kiro, "›  ask a question or describe a task ↵"),
            (
                ActorRuntime::Amp,
                "ctrl+o for commands · ? for shortcuts\n╭──── model ────╮\n│              │\n╰──────────────╯",
            ),
            (ActorRuntime::Auggie, "──────────\n›\n──────────"),
            (
                ActorRuntime::Cursor,
                "Workspace Trust Required\n▶ [a] Trust this workspace\n[q] Quit\nTrusting workspace...\n→ Plan, search, build anything\nRun Everything",
            ),
            (
                ActorRuntime::Devin,
                "❭ Ask Devin to build features, fix bugs, or work on your code",
            ),
            (
                ActorRuntime::Droid,
                "> Try \"A suggested task\"\n? for help | MCP ✓",
            ),
            (ActorRuntime::Copilot, "──────────\n❯\n──────────"),
            (
                ActorRuntime::Cline,
                "──────────\n❯ What can I do for you?\n──────────",
            ),
            (
                ActorRuntime::Kimi,
                "╭──────────╮\n│ >        │\n╰──────────╯",
            ),
        ] {
            assert!(screen(runtime, text), "{runtime:?}");
        }
    }

    #[test]
    fn a_narrow_devin_input_and_cursor_without_yolo_are_ready() {
        assert!(screen(
            ActorRuntime::Devin,
            "❭ Ask Devin to build\nfeatures, fix bugs, or work on your code"
        ));
        assert!(screen(
            ActorRuntime::Cursor,
            "→ Plan, search, build anything\nAsk for approval"
        ));
        assert!(!screen(
            ActorRuntime::Devin,
            "Do you trust the authors of this directory?\n❭ 1 Yes, trust\nAsk Devin"
        ));
    }

    #[test]
    fn droid_input_hints_and_footer_states_release_only_without_its_trust_prompt() {
        // Droid 1.13: one of its thirteen hints starts with "Type", and the
        // footer's left part shows update progress or a translated help hint.
        for text in [
            "> Type \"Review this code for security issues\"\n? for help | MCP ✓",
            "> Try \"Fix all lint warnings\"\n✓ v1.13.1 ready (restart to apply) | MCP ✓",
            "> Try \"Fix all lint warnings\"\n? 查看帮助 | MCP ✓",
        ] {
            assert!(screen(ActorRuntime::Droid, text), "{text}");
        }
        assert!(!screen(
            ActorRuntime::Droid,
            "Trust this folder?\n> 1. Trust this folder\n> Type \"Review this code for security issues\"\n✓ v1.13.1 ready (restart to apply) | MCP ✓"
        ));
        assert!(!screen(
            ActorRuntime::Droid,
            "Skills (…)  MCPs (1) ✓  Connectors (0) ✗\n? for help | MCP ✓"
        ));
    }

    #[test]
    fn amp_input_box_is_ready_at_any_height_but_not_while_holding_text() {
        // Amp on a 120x40 terminal (seen 11-18): three empty rows between the borders.
        let tall = "ctrl+o for commands · ? for shortcuts\n╭──────── opus 5.5 ─╮\n│                  │\n│                  │\n│                  │\n╰──── project ─╯";
        assert!(screen(ActorRuntime::Amp, tall));
        assert!(!screen(
            ActorRuntime::Amp,
            "ctrl+o for commands · ? for shortcuts\n╭──────── opus 5.5 ─╮\n│ draft            │\n│                  │\n╰──── project ─╯"
        ));
        assert!(!screen(
            ActorRuntime::Amp,
            "ctrl+o for commands · ? for shortcuts\n╭──────── opus 5.5 ─╮\n│                  │\n│                  │"
        ));
    }

    #[test]
    fn cline_plan_mode_input_is_ready_unless_a_promotion_covers_it() {
        let plan = "──────────\n❯ Plan something...\n──────────";
        assert!(screen(ActorRuntime::Cline, plan));
        assert!(!screen(
            ActorRuntime::Cline,
            &format!("{plan}\nPress Enter to open, any other key to close")
        ));
    }

    #[test]
    fn hermes_terminal_is_a_viewer_that_holds_no_input() {
        assert!(!required(ActorRuntime::Hermes));
    }

    #[test]
    fn a_wrapped_native_confirmation_still_covers_the_chat_input() {
        assert!(!screen(
            ActorRuntime::Copilot,
            "Confirm folder\ntrust\n──────────\n❯\n──────────"
        ));
        assert!(!screen(
            ActorRuntime::Cline,
            "──────────\n❯ What can I do for you?\n──────────\nPress Enter to open,\nany other key to close"
        ));
    }

    #[test]
    fn a_promotion_over_a_ready_input_still_holds_every_business_byte() {
        assert!(!screen(
            ActorRuntime::Cline,
            "──────────\n❯ What can I do for you?\n──────────\nPress Enter to open, any other key to close"
        ));
        assert!(!screen(
            ActorRuntime::Copilot,
            "Confirm folder trust\n──────────\n❯\n──────────"
        ));
        assert!(!screen(ActorRuntime::Custom, "──────────\n❯\n──────────"));
    }

    #[test]
    fn antigravity_onboarding_path_respects_overrides_platform_precedence_and_relative_home() {
        let cwd = Path::new("/workspace/project");
        let inherited = vec![
            ("HOME".to_owned(), "/inherited/home".to_owned()),
            ("USERPROFILE".to_owned(), "/inherited/profile".to_owned()),
        ];

        // Unix prefers HOME over USERPROFILE and allows spec.env overrides.
        let mut env = BTreeMap::new();
        assert_eq!(
            resolve_antigravity_home_from(cwd, &env, inherited.clone(), false),
            Some(PathBuf::from("/inherited/home"))
        );
        env.insert("HOME".into(), "actor-home".into());
        assert_eq!(
            resolve_antigravity_home_from(cwd, &env, inherited.clone(), false),
            Some(PathBuf::from("/workspace/project/actor-home"))
        );

        // Windows prefers USERPROFILE over HOME and matches keys case-insensitively.
        let mut win_env = BTreeMap::new();
        win_env.insert("userprofile".into(), "/override/win-profile".into());
        assert_eq!(
            resolve_antigravity_home_from(cwd, &win_env, inherited.clone(), true),
            Some(PathBuf::from("/override/win-profile"))
        );

        // Empty override falls through; missing env yields None (fail-closed).
        let mut empty_home = BTreeMap::new();
        empty_home.insert("HOME".into(), String::new());
        assert_eq!(
            resolve_antigravity_home_from(cwd, &empty_home, Vec::new(), false),
            None
        );
    }

    #[test]
    fn antigravity_onboarding_complete_requires_regular_file_and_explicit_true() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("onboarding.json");
        assert!(!antigravity_onboarding_complete(&path));
        assert!(
            !path.exists(),
            "checking onboarding state must never create onboarding.json"
        );

        // Directories are not regular files and must fail closed.
        let dir_path = temp.path().join("dir-onboarding.json");
        std::fs::create_dir_all(&dir_path).expect("dir");
        assert!(!antigravity_onboarding_complete(&dir_path));

        for invalid in [
            "{}",
            r#"{"onboardingComplete": false}"#,
            r#"{"onboardingComplete": "true"}"#,
            r#"{"consumerOnboardingComplete": true}"#,
            "not json",
            "[]",
        ] {
            std::fs::write(&path, invalid).expect("write fixture");
            assert!(
                !antigravity_onboarding_complete(&path),
                "must reject {invalid}"
            );
        }

        // Oversized files are rejected safely.
        std::fs::write(&path, vec![b' '; (MAX_ONBOARDING_BYTES + 16) as usize])
            .expect("write oversized");
        assert!(!antigravity_onboarding_complete(&path));

        std::fs::write(
            &path,
            r#"{"consumerOnboardingComplete": true, "enterpriseOnboardingComplete": false, "onboardingComplete": true}"#,
        )
        .expect("write complete");
        assert!(antigravity_onboarding_complete(&path));
    }

    #[cfg(unix)]
    #[test]
    fn antigravity_onboarding_complete_never_blocks_on_a_fifo() {
        let temp = tempfile::tempdir().expect("tempdir");
        let fifo = temp.path().join("onboarding.json");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo");
        assert!(status.success(), "mkfifo failed");
        assert!(
            !antigravity_onboarding_complete(&fifo),
            "FIFO must fail closed immediately without blocking"
        );
    }

    #[test]
    fn antigravity_screen_ready_requires_bordered_chat_input_without_wizard_or_trust() {
        let to_rows = |text: &str| text.lines().map(str::to_owned).collect::<Vec<_>>();
        assert!(!antigravity_screen_ready(&[]));
        assert!(!antigravity_screen_ready(&to_rows("   \n\t\n")));
        for held in [
            // Onboarding wizard screens (normal and wrapped)
            "Welcome to Antigravity CLI\nChoose your color scheme\n[Next]",
            "Welcome to\nAntigravity CLI\n[Next]",
            "Choose your\ncolor scheme\nDark  Light",
            "Terms of Service & Data Use\n[Previous] [Done]",
            "Terms of Service\n& Data Use\n[Done]",
            // Workspace trust screens (without or with bordered selection)
            "Trust this workspace?\n> 1. Yes, trust\n2. No, exit",
            "Do you trust the authors of this folder?\n──────────\n> 1. Yes\n──────────",
            "Trust this workspace?\n──────────\n>\n──────────",
            "Trust this folder?\n──────────\n>\n──────────",
            "Do you trust this workspace?\n──────────\n>\n──────────",
            // Banner-only, buttons, or partial redraws
            "Antigravity CLI 1.5.16\n[Done]",
            "Antigravity CLI 1.5.16\n>\n? for shortcuts",
            "──────────\n>",
            "An entirely different UI\n>",
            // Wizard lingering above a bordered input
            "Terms of Service & Data Use\n──────────\n>\n──────────",
        ] {
            assert!(
                !antigravity_screen_ready(&to_rows(held)),
                "must hold non-chat screen: {held}"
            );
        }
        for ready_screen in [
            "Antigravity CLI 1.5.16\n──────────\n>\n──────────\n? for shortcuts",
            "━━━━━━━━━━━━━━━━━━━━\n  >  \n━━━━━━━━━━━━━━━━━━━━",
        ] {
            assert!(
                antigravity_screen_ready(&to_rows(ready_screen)),
                "expected ready bordered input for: {ready_screen}"
            );
        }
        assert!(!required(ActorRuntime::Antigravity));
        assert!(supports_gate(ActorRuntime::Antigravity));
    }
}
