//! Startup input policy for ordinary Actor terminals. A ready screen releases
//! only this Session; it is not a provider admission receipt or native consent.
use cccc_contracts::ActorRuntime;

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
}
