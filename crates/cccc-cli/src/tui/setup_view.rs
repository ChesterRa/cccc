//! The `/setup` screens: pick apps, confirm once, then follow each app's
//! next step. Nothing here claims a session is connected; only the session
//! list, read from the daemon, says what can receive.

use super::app::{App, Notice, SetupFlow, SetupStep};
use super::setup::{InstallState, RUNTIMES, pickable};
use super::text;
use super::view::{Theme, runtime_label, separator, session_row, spread};
use crate::commands::sessions::setup::SetupInspection;
use cccc_contracts::local_session::LocalSessionRuntime;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

const STEPS: usize = 3;
// Under an app's name in the last step.
const DETAIL_INDENT: usize = 16;

pub(super) fn render(frame: &mut Frame, app: &App, theme: Theme) {
    let Some(flow) = &app.flow else {
        return;
    };
    let area = super::view::content_area(frame.area());
    let width = usize::from(area.width);
    let (lines, keys) = match flow.step {
        SetupStep::Select => select(app, flow, theme, width),
        SetupStep::Confirm => confirm(app, flow, theme, width),
        SetupStep::Finish => finish(app, theme, width),
    };
    // A notice takes the row above the separator, as on the main screen.
    let notice = app.notice.as_ref().map(|notice| {
        let (message, style) = match notice {
            Notice::Info(message) => (message, Style::new()),
            Notice::Error(message) => (message, theme.error()),
        };
        Line::styled(
            format!(" {}", text::fit(message, width.saturating_sub(2))),
            style,
        )
    });
    let body = Rect {
        height: area.height - 2 - u16::from(notice.is_some()),
        ..area
    };
    // Picking fits the smallest screen; the later steps scroll when an app's
    // steps, warnings and new sessions run past the footer.
    let mut keys = keys;
    let mut top = 0;
    if flow.step != SetupStep::Select {
        let scrolls;
        (top, scrolls) = flow.scroll.fit(lines.len(), body.height);
        if scrolls {
            keys = format!("Up/Down scroll   {keys}");
        }
    }
    frame.render_widget(Paragraph::new(lines).scroll((top, 0)), body);
    if let Some(notice) = notice {
        frame.render_widget(
            Paragraph::new(notice),
            Rect {
                y: area.y + area.height - 3,
                height: 1,
                ..area
            },
        );
    }
    frame.render_widget(
        Paragraph::new(separator(theme, width)),
        Rect {
            y: area.y + area.height - 2,
            height: 1,
            ..area
        },
    );
    frame.render_widget(
        Paragraph::new(Line::styled(format!(" {keys}"), theme.dim())),
        Rect {
            y: area.y + area.height - 1,
            height: 1,
            ..area
        },
    );
}

fn title(name: &str, step: usize, theme: Theme, width: usize) -> Line<'static> {
    spread(
        vec![Span::raw(" "), Span::styled(name.to_owned(), theme.bold())],
        vec![Span::styled(format!("{step}/{STEPS} "), theme.dim())],
        width,
    )
}

fn select(app: &App, flow: &SetupFlow, theme: Theme, width: usize) -> (Vec<Line<'static>>, String) {
    let mut lines = vec![
        title("Connect native sessions", 1, theme, width),
        Line::raw(format!(
            " {}",
            text::fit(
                "Pick apps whose sessions may message each other via CCCC.",
                width.saturating_sub(2)
            )
        )),
        Line::default(),
    ];
    let Some(checks) = app.checks() else {
        lines.push(Line::styled(
            " Checking Codex, Claude Code, Pi and OpenCode...",
            theme.dim(),
        ));
        return (lines, "b back".into());
    };
    let user_home = app.context.user_home.as_deref();
    for (index, (runtime, check)) in RUNTIMES.iter().zip(checks).enumerate() {
        let cursor = if index == flow.cursor { ">" } else { " " };
        let found = check.executable.is_some();
        let mark = match (pickable(check), flow.picked[index]) {
            (false, _) => "[-]",
            (true, true) => "[x]",
            (true, false) => "[ ]",
        };
        let mut row = vec![
            Span::raw(format!(" {cursor} {mark} ")),
            Span::raw(format!("{:<13}", runtime_label(*runtime))),
        ];
        if found {
            let version = text::one_line(check.version.as_deref().unwrap_or("?"));
            row.push(Span::raw(format!("{:<12}", text::fit(&version, 11))));
            if let Some(directory) = &check.provider_dir {
                let directory = text::home_relative(&directory.to_string_lossy(), user_home);
                row.push(Span::raw(text::fit(&directory, width.saturating_sub(33))));
            }
        } else {
            row.push(Span::styled("not found on PATH", theme.dim()));
        }
        lines.push(Line::from(row));
        if found {
            let (status, style) = status(check, theme);
            lines.push(detail(&status, style, 9, width));
            if flow.picked[index] && check.reconfigure_disconnects {
                lines.push(detail(
                    &disconnect_warning(*runtime),
                    theme.bold(),
                    9,
                    width,
                ));
            }
        }
    }
    if checks.iter().all(|check| check.executable.is_none()) {
        lines.push(Line::default());
        for line in text::wrap(
            "None of these apps is on PATH. Install one, then run /setup again.",
            width.saturating_sub(2),
        ) {
            lines.push(Line::styled(format!(" {line}"), theme.dim()));
        }
    }
    (
        lines,
        "Up/Down move   Space pick   Enter continue   b back".into(),
    )
}

// What the check found, phrased without claiming more than it can know: a
// matching configuration does not mean open sessions have loaded it.
fn status(check: &SetupInspection, theme: Theme) -> (String, Style) {
    if let Some(issue) = &check.issue {
        return (text::one_line(issue), theme.error());
    }
    let status = if check.disabled {
        "turned off with `cccc sessions disable`; picking it turns it back on"
    } else {
        match check.configured {
            Some(true) => "set up by this CCCC",
            Some(false) => "needs setup or update",
            None => "setup state unknown",
        }
    };
    (status.to_owned(), theme.dim())
}

fn disconnect_warning(runtime: LocalSessionRuntime) -> String {
    format!(
        "Open {} sessions disconnect until you reload them.",
        runtime_label(runtime)
    )
}

fn confirm(
    app: &App,
    flow: &SetupFlow,
    theme: Theme,
    width: usize,
) -> (Vec<Line<'static>>, String) {
    let mut lines = vec![title("Connect native sessions", 2, theme, width)];
    for line in text::wrap(
        "Sessions of these apps, run by your OS user, will be able to send work to each other through this CCCC:",
        width.saturating_sub(2),
    ) {
        lines.push(Line::raw(format!(" {line}")));
    }
    lines.push(Line::default());
    let user_home = app.context.user_home.as_deref();
    let checks = app.checks().unwrap_or_default();
    for (index, runtime) in RUNTIMES.iter().enumerate() {
        if !flow.picked[index] {
            continue;
        }
        let check = checks.get(index);
        let directory = check
            .and_then(|check| check.provider_dir.as_ref())
            .map(|directory| text::home_relative(&directory.to_string_lossy(), user_home))
            .unwrap_or_default();
        lines.push(Line::raw(format!(
            "   {:<13}{}",
            runtime_label(*runtime),
            text::fit(&directory, width.saturating_sub(17))
        )));
        if check.is_some_and(|check| check.reconfigure_disconnects) {
            lines.push(detail(
                &disconnect_warning(*runtime),
                theme.bold(),
                DETAIL_INDENT,
                width,
            ));
        }
    }
    lines.push(Line::default());
    lines.push(Line::from(vec![
        Span::styled(" CCCC home  ", theme.dim()),
        Span::raw(text::fit(&app.context.home, width.saturating_sub(13))),
    ]));
    for line in text::wrap(
        "Turn it off later with `cccc sessions disable --runtime <app>`.",
        width.saturating_sub(2),
    ) {
        lines.push(Line::styled(format!(" {line}"), theme.dim()));
    }
    (lines, "Enter connect   b back".into())
}

fn finish(app: &App, theme: Theme, width: usize) -> (Vec<Line<'static>>, String) {
    let mut lines = vec![title("Finish in each app", 3, theme, width)];
    let checks = app.checks().unwrap_or_default();
    let installs = app.installs().unwrap_or_default();
    if installs.is_empty() {
        lines.push(Line::styled("   Starting...", theme.dim()));
    }
    let mut written = false;
    let user_home = app.context.user_home.as_deref();
    for (runtime, state) in installs {
        let check = RUNTIMES
            .iter()
            .position(|candidate| candidate == runtime)
            .and_then(|index| checks.get(index));
        let (state_text, state_style) = match state {
            InstallState::Waiting => ("waiting".to_owned(), theme.dim()),
            InstallState::Installing => ("installing...".to_owned(), theme.dim()),
            InstallState::Written { provider_dir, .. } => (
                format!(
                    "configuration written to {}",
                    text::home_relative(&provider_dir.to_string_lossy(), user_home)
                ),
                Style::new(),
            ),
            InstallState::Failed(_) => ("failed".to_owned(), theme.error()),
            InstallState::Interrupted => (
                "interrupted; its configuration may be partly written".to_owned(),
                theme.error(),
            ),
            InstallState::Skipped => ("skipped".to_owned(), theme.dim()),
        };
        lines.push(Line::from(vec![
            Span::raw(format!("   {:<13}", runtime_label(*runtime))),
            Span::styled(
                text::fit(&state_text, width.saturating_sub(DETAIL_INDENT + 1)),
                state_style,
            ),
        ]));
        match state {
            InstallState::Written { warnings, .. } => {
                written = true;
                let steps = next_steps(app, *runtime, check);
                lines.extend(details(&steps, Style::new(), width));
                for warning in warnings {
                    lines.extend(details(&text::one_line(warning), theme.dim(), width));
                }
            }
            InstallState::Failed(error) => {
                lines.extend(details(&text::one_line(error), theme.error(), width));
            }
            InstallState::Interrupted => {
                lines.extend(details(&retry_hint(*runtime), theme.dim(), width));
            }
            _ => {}
        }
    }
    let arrived = app.new_sessions();
    if !arrived.is_empty() {
        lines.push(Line::default());
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled("New sessions since setup started", theme.bold()),
        ]));
        for source in arrived {
            lines.push(session_row(source, user_home, theme, width));
        }
    }
    if written {
        lines.push(Line::default());
        for line in text::wrap(
            "Try it: in one session, ask it to list CCCC sessions first, then contact the one you mean; if several match, it should ask you which one.",
            width.saturating_sub(2),
        ) {
            lines.push(Line::raw(format!(" {line}")));
        }
    }
    let keys = if app.install_running() {
        "b cancel"
    } else {
        "Enter back to status"
    };
    (lines, keys.into())
}

// The step each app needs before its open sessions use the new
// configuration. Hints for receive problems appear only when a session of
// that app is listed and cannot receive.
fn next_steps(app: &App, runtime: LocalSessionRuntime, check: Option<&SetupInspection>) -> String {
    let was_set_up = check.is_some_and(|check| check.configured == Some(true));
    let stuck = app
        .sessions
        .sources
        .iter()
        .any(|source| source.runtime == runtime && !source.connected);
    match runtime {
        LocalSessionRuntime::Codex => {
            let mut steps = "Start or restart Codex, approve the CCCC hooks when asked (/hooks), then send any message.".to_owned();
            steps.push_str(" If upgrading CCCC in open Codex sessions, first let active and queued work finish, then use /daemon > Restart background server > Restart. This reloads MCP for every chat on that server; check cccc in /mcp and review hooks in /hooks.");
            if stuck && check.is_some_and(|check| check.codex_auto_start == Some(false)) {
                steps.push_str(" Receiving needs Codex's shared app-server (codex features enable daemon_auto_start).");
            }
            steps
        }
        LocalSessionRuntime::Claude => {
            let mut steps = if was_set_up {
                "In open sessions, reconnect the cccc MCP server and run /reload-plugins, or start a new session; then send any message.".to_owned()
            } else {
                "Run /reload-plugins (or start a new session), then send any message.".to_owned()
            };
            if stuck {
                steps.push_str(
                    " A session that stays not receiving needs the CCCC plugin's Monitor running.",
                );
            }
            steps
        }
        LocalSessionRuntime::Pi => "Run /reload in Pi, or start Pi.".to_owned(),
        LocalSessionRuntime::Opencode => {
            "Reopen the OpenCode session. A blank new session needs a first message.".to_owned()
        }
    }
}

fn retry_hint(runtime: LocalSessionRuntime) -> String {
    format!(
        "Retry with /setup or `cccc sessions enable --runtime {}`; resolve any conflict it reports.",
        runtime.as_str()
    )
}

fn detail(message: &str, style: Style, indent: usize, width: usize) -> Line<'static> {
    Line::styled(
        format!(
            "{}{}",
            " ".repeat(indent),
            text::fit(message, width.saturating_sub(indent + 1))
        ),
        style,
    )
}

fn details(message: &str, style: Style, width: usize) -> Vec<Line<'static>> {
    text::wrap(message, width.saturating_sub(DETAIL_INDENT + 1))
        .into_iter()
        .map(|line| Line::styled(format!("{}{line}", " ".repeat(DETAIL_INDENT)), style))
        .collect()
}

/// Receipt lines for installs that a stop interrupted, so the user learns
/// after the screen is gone which app may be half configured.
pub(super) fn interrupted_receipt(app: &App) -> Option<String> {
    let interrupted: Vec<String> = app
        .setup
        .installs
        .iter()
        .filter(|(_, state)| *state == InstallState::Interrupted)
        .map(|(runtime, _)| {
            format!(
                "  {:<13}configuration may be partly written; retry with `cccc sessions enable --runtime {}` and resolve any conflict it reports",
                runtime_label(*runtime),
                runtime.as_str()
            )
        })
        .collect();
    (!interrupted.is_empty()).then(|| format!("Setup was interrupted:\n{}", interrupted.join("\n")))
}

#[cfg(test)]
#[path = "setup_view_tests.rs"]
mod tests;
