//! Drawing. Quiet and terminal-native: the terminal's own foreground, bold
//! section titles, dim secondary text, ANSI green for ready and red for
//! errors, and no boxes. Fixed text is ASCII; names and paths from elsewhere
//! go through `text` before they reach a cell.

use super::app::{App, COMMANDS, Notice, Screen};
use super::groups::{GroupStatus, GroupSummary};
use super::text;
use crate::host_state::{DaemonState, HostPhase, ShutdownReport, StopResult, WebState};
use cccc_contracts::local_session::{LocalSessionRuntime, LocalSessionSource};
use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use std::path::Path;
use std::time::Instant;

pub(super) const MIN_WIDTH: u16 = 60;
pub(super) const MIN_HEIGHT: u16 = 16;
// "Claude Code" plus room before the directory.
const RUNTIME_COLUMN: usize = 13;
// "not receiving", so both states end in the same column.
const STATUS_COLUMN: usize = 13;
// Compact status and running-count columns; they do not stretch with the terminal.
const GROUP_STATUS_COLUMN: usize = 10;
const GROUP_DETAIL_COLUMN: usize = 8;
pub(super) const INDENT: &str = "   ";
// The main screen's header, the blank under it, two list titles with a blank
// between the lists, and the notice, separator and command rows.
const MAIN_FIXED_ROWS: u16 = 10;
const MAX_WIDTH: u16 = 100;

#[derive(Clone, Copy)]
pub(super) struct Theme {
    color: bool,
}

impl Theme {
    pub(super) fn bold(self) -> Style {
        Style::new().add_modifier(Modifier::BOLD)
    }

    pub(super) fn accent(self) -> Style {
        if self.color {
            self.bold().fg(Color::Cyan)
        } else {
            self.bold()
        }
    }

    pub(super) fn selected(self) -> Style {
        self.accent().add_modifier(Modifier::REVERSED)
    }

    pub(super) fn dim(self) -> Style {
        Style::new().add_modifier(Modifier::DIM)
    }

    pub(super) fn ok(self) -> Style {
        if self.color {
            Style::new().fg(Color::Green)
        } else {
            Style::new()
        }
    }

    // Without color an error still has to stand out from ordinary text.
    pub(super) fn error(self) -> Style {
        if self.color {
            Style::new().fg(Color::Red)
        } else {
            Style::new().add_modifier(Modifier::BOLD)
        }
    }
}

pub(super) fn render(frame: &mut Frame, app: &App, now: Instant) {
    let area = frame.area();
    let theme = Theme {
        color: app.context.color,
    };
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        too_small(frame, theme);
        return;
    }
    // Once stopping starts, only the status and how to force are left to show.
    match app.screen {
        _ if app.stopping() => main_screen(frame, app, theme, now),
        Screen::Main => main_screen(frame, app, theme, now),
        Screen::Help => help_screen(frame, app, theme),
        Screen::Setup => super::setup_view::render(frame, app, theme),
        Screen::Groups => super::groups_view::render(frame, app, theme, now),
        Screen::Sessions => super::sessions_view::render(frame, app, theme, now),
        Screen::Tasks => super::tasks_view::render(frame, app, theme, now),
    }
}

/// A single bounded canvas: metadata and tables never drift to opposite
/// edges just because the terminal is wide.
pub(super) fn content_area(area: Rect) -> Rect {
    let width = area.width.min(MAX_WIDTH);
    Rect {
        x: area.x + (area.width - width) / 2,
        width,
        ..area
    }
}

fn main_screen(frame: &mut Frame, app: &App, theme: Theme, now: Instant) {
    let area = content_area(frame.area());
    let width = usize::from(area.width);
    let daemon = match &app.host.daemon {
        DaemonState::StartedHere { .. } => "started here; stops on quit".to_owned(),
        DaemonState::External { pid: Some(pid) } => format!("external pid {pid}; kept on quit"),
        DaemonState::External { pid: None } => "external; kept on quit".to_owned(),
    };
    let header = vec![
        Line::from(vec![
            Span::raw(" "),
            Span::styled("CCCC", theme.accent()),
            Span::raw(format!(" {}   ", app.context.version)),
            Span::styled("Daemon ", theme.dim()),
            Span::raw(text::fit(&daemon, width.saturating_sub(24))),
        ]),
        version_row(app, theme, width),
        home_row(app, theme, width),
    ];
    frame.render_widget(Paragraph::new(header), row(area, 0, 3));
    let candidates = app.candidates();
    if !candidates.is_empty() && !app.stopping() {
        frame.render_widget(
            Paragraph::new(Line::styled(" Commands", theme.bold())),
            row(area, 4, 1),
        );
        let lines: Vec<Line> = candidates
            .iter()
            .enumerate()
            .map(|(position, index)| {
                let (name, description) = COMMANDS[*index];
                let selected = position == app.completion_index();
                Line::from(vec![
                    Span::raw(if selected { " > " } else { INDENT }),
                    Span::styled(format!("{name:<12}"), theme.accent()),
                    Span::raw(text::fit(description, width.saturating_sub(16))),
                ])
                .style(if selected {
                    theme.selected()
                } else {
                    Style::new()
                })
            })
            .collect();
        frame.render_widget(
            Paragraph::new(lines),
            row(area, 5, area.height.saturating_sub(8)),
        );
    } else {
        let table_rows = u16::from(!app.groups.groups.is_empty());
        let (group_rows, session_rows) = split(
            area.height.saturating_sub(MAIN_FIXED_ROWS + table_rows),
            rows_needed(
                app.groups.groups.len(),
                groups_message(app, theme),
                width.saturating_sub(4),
            ),
            rows_needed(
                app.sessions.sources.len(),
                sessions_message(app, theme),
                width.saturating_sub(4),
            ),
        );
        frame.render_widget(
            Paragraph::new(groups_title(app, theme, width, now)),
            row(area, 4, 1),
        );
        if table_rows > 0 {
            frame.render_widget(Paragraph::new(group_columns(theme, width)), row(area, 5, 1));
        }
        group_list(frame, app, theme, row(area, 5 + table_rows, group_rows));
        let sessions_at = 6 + table_rows + group_rows;
        frame.render_widget(
            Paragraph::new(sessions_title(app, theme, width, now)),
            row(area, sessions_at, 1),
        );
        session_list(frame, app, theme, row(area, sessions_at + 1, session_rows));
    }
    bottom(frame, app, theme);
}

/// Rows for the group and session lists out of `room`: what each needs when
/// both fit, and otherwise half each, a half one list does not need going to
/// the other.
fn split(room: u16, groups: u16, sessions: u16) -> (u16, u16) {
    if groups + sessions <= room {
        return (groups, sessions);
    }
    let half = room / 2;
    if groups <= half {
        (groups, room - groups)
    } else if sessions <= room - half {
        (room - sessions, sessions)
    } else {
        (half, room - half)
    }
}

// A list needs a row per item, or the rows its message wraps to.
fn rows_needed(items: usize, message: Option<(String, Style)>, width: usize) -> u16 {
    let rows = if items > 0 {
        items
    } else {
        message.map_or(0, |(message, _)| text::wrap(&message, width).len())
    };
    u16::try_from(rows).unwrap_or(u16::MAX)
}

/// Shows `message` in place of an empty list, indented like the rows.
pub(super) fn list_message(frame: &mut Frame, message: (String, Style), area: Rect) {
    let width = usize::from(area.width).saturating_sub(4);
    let lines: Vec<Line> = text::wrap(&message.0, width)
        .into_iter()
        .map(|line| Line::styled(format!("{INDENT}{line}"), message.1))
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
}

pub(super) fn groups_title(app: &App, theme: Theme, width: usize, now: Instant) -> Line<'static> {
    let groups = &app.groups;
    let status = match (&groups.error, groups.last_ok) {
        (Some(_), Some(last_ok)) => Span::styled(
            format!(
                "not updated (last ok {})",
                text::ago(now.saturating_duration_since(last_ok))
            ),
            theme.error(),
        ),
        (Some(_), None) => Span::styled("not available", theme.error()),
        (None, None) => Span::styled("reading...", theme.dim()),
        (None, Some(_)) => Span::raw(format!(
            "{}, {} running",
            count(groups.groups.len(), "group"),
            groups.running()
        )),
    };
    Line::from(vec![
        Span::raw(" "),
        Span::styled("Groups", theme.bold()),
        Span::raw("  "),
        clipped_status(status, width.saturating_sub(10)),
    ])
}

pub(super) fn groups_message(app: &App, theme: Theme) -> Option<(String, Style)> {
    let groups = &app.groups;
    if !groups.groups.is_empty() {
        return None;
    }
    match (&groups.error, groups.last_ok) {
        (Some(error), None) => Some((
            format!("Could not read groups: {}", text::one_line(error)),
            theme.error(),
        )),
        (_, Some(_)) => Some((
            "No groups yet. Create one in the Web, or ask a session to contact another.".to_owned(),
            Style::new(),
        )),
        (None, None) => None,
    }
}

fn group_list(frame: &mut Frame, app: &App, theme: Theme, area: Rect) {
    let groups = &app.groups.groups;
    if groups.is_empty() {
        if let Some(message) = groups_message(app, theme) {
            list_message(frame, message, area);
        }
        return;
    }
    let width = usize::from(area.width);
    let rows = usize::from(area.height);
    let shown = if groups.len() > rows {
        rows.saturating_sub(1)
    } else {
        groups.len()
    };
    app.groups_view
        .page
        .set(u16::try_from(shown).unwrap_or(u16::MAX));
    let selected = app
        .groups_view
        .selected
        .as_deref()
        .and_then(|id| app.groups.position(id));
    let start = window_start(groups.len(), shown, selected);
    let mut lines: Vec<Line> = groups
        .iter()
        .enumerate()
        .skip(start)
        .take(shown)
        .map(|(i, group)| group_row(group, Some(i) == selected, theme, width))
        .collect();
    let hidden = groups.len() - shown;
    if hidden > 0 {
        lines.push(Line::styled(
            format!("{INDENT}+{hidden} more   /groups shows all"),
            theme.dim(),
        ));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

pub(super) fn window_start(count: usize, rows: usize, selected: Option<usize>) -> usize {
    selected
        .unwrap_or(0)
        .saturating_sub(rows / 2)
        .min(count.saturating_sub(rows))
}

fn group_name_width(width: usize) -> usize {
    width
        .saturating_sub(INDENT.len() + 4 + GROUP_STATUS_COLUMN + GROUP_DETAIL_COLUMN + 1)
        .min(44)
}

pub(super) fn group_columns(theme: Theme, width: usize) -> Line<'static> {
    let name = group_name_width(width);
    Line::styled(
        format!(
            "{INDENT}{:<name$}  {:<GROUP_STATUS_COLUMN$}  {:>GROUP_DETAIL_COLUMN$}",
            "NAME", "STATUS", "RUNNING"
        ),
        theme.dim(),
    )
}

pub(super) fn group_row(
    group: &GroupSummary,
    selected: bool,
    theme: Theme,
    width: usize,
) -> Line<'static> {
    let name_width = group_name_width(width);
    let name = text::fit(&text::one_line(group.name()), name_width);
    let pad = " ".repeat(name_width.saturating_sub(text::width(&name)));
    let status_style = match group.status {
        _ if selected => Style::new(),
        GroupStatus::Running => theme.ok(),
        GroupStatus::Stopped | GroupStatus::Offline => theme.dim(),
        GroupStatus::Idle | GroupStatus::Paused => Style::new(),
    };
    Line::from(vec![
        Span::raw(if selected { " > " } else { INDENT }),
        Span::raw(format!("{name}{pad}  ")),
        Span::styled(
            format!("{:<GROUP_STATUS_COLUMN$}  ", group.status.label()),
            status_style,
        ),
        Span::raw(format!(
            "{:>GROUP_DETAIL_COLUMN$}",
            text::fit(&group.running_actors.to_string(), GROUP_DETAIL_COLUMN)
        )),
    ])
    .style(if selected {
        theme.selected()
    } else {
        Style::new()
    })
}

/// "1 group", "2 groups".
pub(super) fn count(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

fn version_row(app: &App, theme: Theme, width: usize) -> Line<'static> {
    let value = match &app.host.web {
        WebState::Starting => Span::styled("starting...", theme.dim()),
        WebState::Listening { url } => {
            Span::raw(text::fit(&text::one_line(url), width.saturating_sub(7)))
        }
        WebState::Restarting => Span::styled("restarting...", theme.dim()),
        WebState::Stopped => Span::styled("stopped", theme.dim()),
        WebState::Error { .. } => Span::styled("error", theme.error()),
    };
    Line::from(vec![Span::styled(" Web   ", theme.dim()), value])
}

fn home_row(app: &App, theme: Theme, width: usize) -> Line<'static> {
    Line::from(vec![
        Span::styled(" Home  ", theme.dim()),
        Span::raw(text::fit(
            &text::one_line(&app.context.home),
            width.saturating_sub(7),
        )),
    ])
}

pub(super) fn sessions_title(app: &App, theme: Theme, width: usize, now: Instant) -> Line<'static> {
    let sessions = &app.sessions;
    let status = match (&sessions.error, sessions.last_ok) {
        (Some(_), Some(last_ok)) => Span::styled(
            format!(
                "not updated (last ok {})",
                text::ago(now.saturating_duration_since(last_ok))
            ),
            theme.error(),
        ),
        (Some(_), None) => Span::styled("not available", theme.error()),
        (None, None) => Span::styled("reading...", theme.dim()),
        (None, Some(_)) => Span::raw(format!(
            "{} known, {} can receive",
            sessions.sources.len(),
            sessions.receiving()
        )),
    };
    Line::from(vec![
        Span::raw(" "),
        Span::styled("Native sessions", theme.bold()),
        Span::raw("  "),
        clipped_status(status, width.saturating_sub(19)),
    ])
}

pub(super) fn sessions_message(app: &App, theme: Theme) -> Option<(String, Style)> {
    let sessions = &app.sessions;
    if !sessions.sources.is_empty() {
        return None;
    }
    match (&sessions.error, sessions.last_ok) {
        (Some(error), None) => Some((
            format!("Could not read sessions: {}", text::one_line(error)),
            theme.error(),
        )),
        (_, Some(_)) => Some((
            "No sessions yet. Type /setup to connect Codex, Claude Code, Pi or OpenCode."
                .to_owned(),
            Style::new(),
        )),
        (None, None) => None,
    }
}

fn session_list(frame: &mut Frame, app: &App, theme: Theme, area: Rect) {
    let sessions = &app.sessions;
    if sessions.sources.is_empty() {
        if let Some(message) = sessions_message(app, theme) {
            list_message(frame, message, area);
        }
        return;
    }
    let width = usize::from(area.width);
    let rows = usize::from(area.height);
    let shown = if sessions.sources.len() > rows {
        rows.saturating_sub(1)
    } else {
        sessions.sources.len()
    };
    let user_home = app.context.user_home.as_deref();
    let mut lines: Vec<Line> = sessions.sources[..shown]
        .iter()
        .map(|source| session_row(source, user_home, theme, width))
        .collect();
    let hidden = sessions.sources.len() - shown;
    if hidden > 0 {
        lines.push(Line::styled(
            format!("{INDENT}+{hidden} more   /sessions shows all"),
            theme.dim(),
        ));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

pub(super) fn session_row(
    source: &LocalSessionSource,
    user_home: Option<&Path>,
    theme: Theme,
    width: usize,
) -> Line<'static> {
    let (status, status_style) = if source.connected {
        ("ready", theme.ok())
    } else {
        ("not receiving", theme.dim())
    };
    let room = width
        .saturating_sub(INDENT.len() + RUNTIME_COLUMN + 2 + 7 + 2 + STATUS_COLUMN + 1)
        .min(36);
    let place = text::fit(
        &text::home_relative(&text::one_line(&source.cwd), user_home),
        room,
    );
    let pad = " ".repeat(room.saturating_sub(text::width(&place)));
    let source_id = text::one_line(&source.source_id);
    let id = text::tail_within(&source_id, 6);
    Line::from(vec![
        Span::raw(INDENT),
        Span::raw(format!(
            "{:<RUNTIME_COLUMN$}",
            runtime_label(source.runtime)
        )),
        Span::raw(format!("{place}{pad}  ")),
        Span::styled(format!("#{id:>6}  "), theme.dim()),
        Span::styled(status, status_style),
    ])
}

pub(super) fn runtime_label(runtime: LocalSessionRuntime) -> &'static str {
    match runtime {
        LocalSessionRuntime::Codex => "Codex",
        LocalSessionRuntime::Claude => "Claude Code",
        LocalSessionRuntime::Pi => "Pi",
        LocalSessionRuntime::Opencode => "OpenCode",
    }
}

fn bottom(frame: &mut Frame, app: &App, theme: Theme) {
    let area = content_area(frame.area());
    let width = usize::from(area.width);
    let hint = if !app.candidates().is_empty() {
        " Up/Down choose   Tab complete   Enter run   Esc close"
    } else if app.input.is_empty() && app.selected_group().is_some() {
        " Up/Down select group   Enter details   / commands"
    } else if app.input.is_empty() {
        " / commands"
    } else {
        " Enter run   Ctrl+U clear   /help"
    };
    frame.render_widget(
        Paragraph::new(
            notice_line(app, theme, width)
                .unwrap_or_else(|| Line::styled(text::fit(hint, width), theme.dim())),
        ),
        row(area, area.height - 3, 1),
    );
    frame.render_widget(
        Paragraph::new(separator(theme, width)),
        row(area, area.height - 2, 1),
    );
    command_line(frame, app, theme, row(area, area.height - 1, 1));
}

pub(super) fn notice_line(app: &App, theme: Theme, width: usize) -> Option<Line<'static>> {
    let room = width.saturating_sub(2);
    if app.stopping() {
        let force = if app.interrupts > 0 { "again" } else { "twice" };
        return Some(Line::raw(format!(
            " {}",
            text::fit(
                &format!("Stopping CCCC... press Ctrl+C {force} to force exit."),
                room
            )
        )));
    }
    let (message, style) = match (&app.notice, &app.host.web) {
        (Some(Notice::Info(message)), _) => (message.clone(), Style::new()),
        (Some(Notice::Error(message)), _) => (message.clone(), theme.error()),
        (None, WebState::Error { message }) => (
            format!("Web error: {}", text::one_line(message)),
            theme.error(),
        ),
        // A group action keeps running after leaving `/groups`.
        (None, _) if app.screen == Screen::Main && app.busy_action().is_some() => {
            let busy = app.busy_action()?;
            (
                format!("{} {}...", busy.action.doing(), text::one_line(&busy.name)),
                theme.dim(),
            )
        }
        (None, _) => return None,
    };
    Some(Line::styled(
        format!(" {}", text::fit(&message, room)),
        style,
    ))
}

pub(super) fn separator(theme: Theme, width: usize) -> Line<'static> {
    Line::styled(
        format!(" {}", "-".repeat(width.saturating_sub(2))),
        theme.dim(),
    )
}

// The prompt keeps the end of what is typed in view, and gives up the
// command hints before it gives up the text.
fn command_line(frame: &mut Frame, app: &App, theme: Theme, area: Rect) {
    let width = usize::from(area.width);
    let prompt = " > ";
    if app.stopping() {
        frame.render_widget(Paragraph::new(Line::raw(prompt)), area);
        return;
    }
    let room = width.saturating_sub(prompt.len() + 1);
    let before = &app.input[..app.input_cursor];
    let head = text::tail_within(before, room);
    let start = app.input_cursor - head.len();
    let visible = text::prefix_within(&app.input[start..], room);
    let mut spans = vec![
        Span::styled(prompt, theme.accent()),
        Span::raw(visible.to_owned()),
    ];
    if app.input.is_empty() {
        spans.push(Span::styled("Type / for commands", theme.dim()));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
    let column = prompt.len() + text::width(head);
    if let Ok(column) = u16::try_from(column) {
        frame.set_cursor_position(Position::new(area.x + column, area.y));
    }
}

fn help_screen(frame: &mut Frame, app: &App, theme: Theme) {
    let area = content_area(frame.area());
    let width = usize::from(area.width);
    // Inset by a column, with every wrapped line laid out here so the body's
    // height is known for scrolling.
    let room = width.saturating_sub(2);
    let mut lines = vec![Line::default(), Line::styled("Commands", theme.bold())];
    for (name, meaning) in COMMANDS {
        let column = 14;
        for (index, part) in text::wrap(meaning, room.saturating_sub(column))
            .into_iter()
            .enumerate()
        {
            lines.push(if index == 0 {
                Line::from(vec![
                    Span::raw("  "),
                    Span::styled(format!("{name:<12}"), theme.bold()),
                    Span::raw(part),
                ])
            } else {
                Line::raw(format!("{}{part}", " ".repeat(column)))
            });
        }
    }
    // A command gets a line of its own, so wrapping never splits it.
    for (paragraph, command) in [
        (
            "In /groups, t opens the selected Group's tasks. Up/Down chooses a task, Enter reads its complete notes and checklist, d switches between open and done tasks, and PgUp/PgDn changes pages. In details, arrows and paging keys scroll. b or Esc returns one level. Task views are read-only; editing stays in the Web UI.",
            None,
        ),
        (
            "Main screen: Up/Down selects a group, Enter opens it. Type / to see commands. Tab completes without running; Enter runs the selected command. Esc closes the menu. Left/Right, Home/End, Delete and Ctrl+A/E/U edit the command. Bracketed pastes insert text without submitting.",
            None,
        ),
        (
            "RUNNING counts managed actors running and native members ready to receive from that group; it is not the number of busy models or total members. Offline means the group is not stopped or paused but no actors are running.",
            None,
        ),
        (
            "Quitting stops the Web server and the daemon started here. A daemon \
             that was already running keeps running. If stopping takes too long, \
             press Ctrl+C twice to force an exit.",
            None,
        ),
        (
            "Connected sessions work together when you ask one, in its own app, \
             to contact another through CCCC. To disconnect an app, run:",
            Some("cccc sessions disable --runtime <app>"),
        ),
        (
            "To start CCCC without this screen, run:",
            Some("cccc --headless (or cccc web)"),
        ),
    ] {
        lines.push(Line::default());
        lines.extend(text::wrap(paragraph, room).into_iter().map(Line::raw));
        if let Some(command) = command {
            lines.push(Line::raw(format!("  {command}")));
        }
    }
    let body = Rect {
        x: area.x + 1,
        width: area.width.saturating_sub(2),
        height: area.height - 2,
        ..area
    };
    let (top, scrolls) = app.help.fit(lines.len(), body.height);
    frame.render_widget(Paragraph::new(lines).scroll((top, 0)), body);
    frame.render_widget(
        Paragraph::new(separator(theme, width)),
        row(area, area.height - 2, 1),
    );
    let keys = if scrolls {
        " Up/Down scroll   b back"
    } else {
        " b back"
    };
    frame.render_widget(
        Paragraph::new(Line::styled(keys, theme.dim())),
        row(area, area.height - 1, 1),
    );
}

fn too_small(frame: &mut Frame, theme: Theme) {
    let area = frame.area();
    let message = format!("Terminal too small (need {MIN_WIDTH}x{MIN_HEIGHT}). Ctrl+C stops CCCC.");
    let message_area = Rect {
        height: area.height.saturating_sub(1).max(1),
        ..area
    };
    frame.render_widget(
        Paragraph::new(Line::raw(message)).wrap(Wrap { trim: true }),
        message_area,
    );
    if area.height >= 2 {
        frame.render_widget(
            Paragraph::new(Line::styled("Resize to continue.", theme.dim())),
            row(area, area.height - 1, 1),
        );
    }
}

/// What the terminal shows after the interface closes, from the host's
/// account of what actually stopped.
pub(super) fn receipt(report: &ShutdownReport) -> String {
    let failed = matches!(report.web, StopResult::Failed { .. })
        || matches!(report.daemon, StopResult::Failed { .. });
    let mut lines = vec![
        if failed {
            "CCCC stopped with problems.".to_owned()
        } else {
            "CCCC stopped.".to_owned()
        },
        format!("  Web     {}", outcome(&report.web)),
        format!("  Daemon  {}", outcome(&report.daemon)),
    ];
    if failed {
        lines.push(format!("  Log     {}", report.log_path.display()));
    }
    lines.join("\n")
}

fn outcome(result: &StopResult) -> String {
    match result {
        StopResult::Stopped => "stopped".to_owned(),
        StopResult::KeptExternal { pid: Some(pid) } => {
            format!("kept running (external, pid {pid})")
        }
        StopResult::KeptExternal { pid: None } => "kept running (external)".to_owned(),
        StopResult::Failed { message } => format!("failed: {}", text::one_line(message)),
    }
}

/// Whether an age on screen goes stale while nothing else changes.
pub(super) fn shows_age(app: &App) -> bool {
    let stale =
        |error: &Option<String>, last_ok: Option<Instant>| error.is_some() && last_ok.is_some();
    let sessions = stale(&app.sessions.error, app.sessions.last_ok);
    let groups = stale(&app.groups.error, app.groups.last_ok);
    let members = app
        .selected_members()
        .is_some_and(|members| stale(&members.error, members.last_ok));
    let tasks = app
        .selected_tasks()
        .is_some_and(|tasks| stale(&tasks.error, tasks.last_ok));
    app.host.phase == HostPhase::Running
        && match app.screen {
            Screen::Main => sessions || groups,
            Screen::Groups => groups || members || tasks,
            Screen::Tasks => tasks,
            Screen::Sessions => sessions,
            Screen::Help | Screen::Setup => false,
        }
}

fn row(area: Rect, y: u16, height: u16) -> Rect {
    Rect {
        x: area.x,
        y: area.y + y,
        width: area.width,
        height,
    }
}

fn clipped_status(span: Span<'static>, width: usize) -> Span<'static> {
    Span::styled(text::fit(&span.content, width), span.style)
}

fn spans_width(spans: &[Span]) -> usize {
    spans.iter().map(Span::width).sum()
}

/// One row with `left` at the start and `right` against the end. Callers fit
/// their text first; anything still too wide is cut at the screen edge.
pub(super) fn spread(
    left: Vec<Span<'static>>,
    right: Vec<Span<'static>>,
    width: usize,
) -> Line<'static> {
    let gap = width.saturating_sub(spans_width(&left) + spans_width(&right));
    let mut spans = left;
    spans.push(Span::raw(" ".repeat(gap)));
    spans.extend(right);
    Line::from(spans)
}

#[cfg(test)]
#[path = "view_tests.rs"]
mod tests;
