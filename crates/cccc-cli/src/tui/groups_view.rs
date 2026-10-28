//! The `/groups` screen: every group with its state, the selected group's
//! members, and starting or stopping that group once confirmed. Nothing here
//! says more than the daemon's last answer: a done action only means the
//! daemon accepted it, and the rows below show what it reports afterwards.

use super::app::App;
use super::groups::{GroupAction, GroupCommand, GroupSummary, Member, MemberState};
use super::text;
use super::view::{
    INDENT, Theme, count, group_columns, group_row, groups_message, groups_title, list_message,
    notice_line, separator, spread,
};
use crate::host_state::WebState;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use std::time::Instant;

// "Claude Code" and "foreman", each with room after it.
const RUNTIME_COLUMN: usize = 13;
const ROLE_COLUMN: usize = 9;
// "not receiving" and "access paused".
const STATE_COLUMN: usize = 13;
const NATIVE_TAG: &str = " native";
const ID_COLUMN_MAX: usize = 24;
// Rows besides the list and the details: the title and the blank under it,
// the blank between list and details, and the notice, separator and keys.
const FIXED_ROWS: u16 = 6;

pub(super) fn render(frame: &mut Frame, app: &App, theme: Theme, now: Instant) {
    let area = super::view::content_area(frame.area());
    let width = usize::from(area.width);
    frame.render_widget(
        Paragraph::new(groups_title(app, theme, width, now)),
        row(area, 0, 1),
    );
    let room = area.height.saturating_sub(FIXED_ROWS);
    if let Some(confirm) = &app.groups_view.confirm {
        // The question takes the whole body, so even the smallest screen
        // shows all of what Enter would do.
        frame.render_widget(
            Paragraph::new(confirmation(app, confirm, theme, width)),
            row(area, 2, room + 1),
        );
    } else {
        frame.render_widget(Paragraph::new(group_columns(theme, width)), row(area, 1, 1));
        let wanted = u16::try_from(app.groups.groups.len().max(1)).unwrap_or(u16::MAX);
        let list_rows = wanted.min((room / 2).saturating_sub(1).max(1));
        list(frame, app, theme, row(area, 2, list_rows));
        details(
            frame,
            app,
            theme,
            now,
            row(area, 3 + list_rows, room.saturating_sub(list_rows)),
        );
    }
    if let Some(notice) = notice_line(app, theme, width) {
        frame.render_widget(Paragraph::new(notice), row(area, area.height - 3, 1));
    }
    frame.render_widget(
        Paragraph::new(separator(theme, width)),
        row(area, area.height - 2, 1),
    );
    frame.render_widget(
        Paragraph::new(Line::styled(keys(app), theme.dim())),
        row(area, area.height - 1, 1),
    );
}

fn keys(app: &App) -> String {
    if let Some(confirm) = &app.groups_view.confirm {
        return format!(" Enter {}   b cancel", confirm.action.verb());
    }
    if let Some(busy) = app.busy_action() {
        return format!(
            " {} {}...   b back",
            busy.action.doing(),
            text::fit(&text::one_line(&busy.name), 32)
        );
    }
    " Up/Down group   t tasks   s start   x stop   b back".into()
}

// The window keeps the selected group in view, near the middle once the list
// is longer than the window.
fn list(frame: &mut Frame, app: &App, theme: Theme, area: Rect) {
    let groups = &app.groups.groups;
    app.groups_view.page.set(area.height);
    if groups.is_empty() {
        if let Some(message) = groups_message(app, theme) {
            list_message(frame, message, area);
        }
        return;
    }
    let rows = usize::from(area.height);
    let selected = app
        .groups_view
        .selected
        .as_deref()
        .and_then(|group_id| app.groups.position(group_id));
    let start = selected
        .unwrap_or(0)
        .saturating_sub(rows / 2)
        .min(groups.len().saturating_sub(rows));
    let width = usize::from(area.width);
    let lines: Vec<Line> = groups
        .iter()
        .enumerate()
        .skip(start)
        .take(rows)
        .map(|(index, group)| group_row(group, Some(index) == selected, theme, width))
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
}

fn details(frame: &mut Frame, app: &App, theme: Theme, now: Instant, area: Rect) {
    let width = usize::from(area.width);
    let rows = usize::from(area.height);
    let Some(group) = app.selected_group() else {
        if app.groups_view.selected.is_some() && !app.groups.groups.is_empty() {
            let message = (
                "The selected group is no longer listed. Choose another with Up/Down.".to_owned(),
                theme.error(),
            );
            list_message(frame, message, area);
        }
        return;
    };
    let room = width.saturating_sub(2);
    let title = text::one_line(group.name());
    let title_lines = text::wrap_exact(&title, room);
    // Keep identity, Web and a member row usable even for a very long name.
    let title_rows = rows.saturating_sub(5).max(1);
    let mut lines: Vec<Line> = title_lines
        .iter()
        .take(title_rows)
        .enumerate()
        .map(|(index, line)| {
            let line = if index + 1 == title_rows && title_lines.len() > title_rows {
                text::fit(&format!("{line}..."), room)
            } else {
                line.clone()
            };
            Line::styled(format!(" {line}"), theme.bold())
        })
        .collect();
    lines.push(Line::raw(text::fit(
        &format!(
            " {}  State {} | {} running",
            text::one_line(&group.group_id),
            text::one_line(&group.state),
            count(
                usize::try_from(group.running_actors).unwrap_or(usize::MAX),
                "actor"
            )
        ),
        width,
    )));
    if let WebState::Listening { url } = &app.host.web {
        lines.push(Line::from(vec![
            Span::styled(" Web  ", theme.dim()),
            Span::raw(text::fit(
                &text::one_line(&format!("{url}/?group={}", group.group_id)),
                width.saturating_sub(7),
            )),
        ]));
    }
    lines.push(super::tasks_view::summary(app, theme, width));
    let room = rows.saturating_sub(lines.len());
    lines.extend(members(app, group, theme, now, width, room));
    lines.truncate(rows);
    frame.render_widget(Paragraph::new(lines), area);
}

// What the action does, said before Enter sends it. A group that may have
// native members (or whose members are not read yet) is told what happens to
// them, since their apps are not CCCC's to start or stop.
fn confirmation(
    app: &App,
    confirm: &GroupCommand,
    theme: Theme,
    width: usize,
) -> Vec<Line<'static>> {
    let verb = match confirm.action {
        GroupAction::Start { resume: false } => "Start",
        GroupAction::Start { resume: true } => "Resume",
        GroupAction::Stop => "Stop",
    };
    // A long name is cut to keep the question on one line; the ID below
    // still says exactly which group it is.
    let name = text::fit(
        &text::one_line(&confirm.name),
        width.saturating_sub(verb.len() + 4),
    );
    let (effect, native) = match confirm.action {
        GroupAction::Start { resume: false } => (
            "Its enabled actors start; actors that are off stay off.",
            "Its enabled native members can receive from this group again.",
        ),
        GroupAction::Start { resume: true } => (
            "Delivery resumes and its enabled actors run; actors that are off stay off.",
            "Its enabled native members can receive from this group again.",
        ),
        GroupAction::Stop => (
            "Its managed actors stop.",
            "Native members stop receiving from this group. Their apps stay open, and work \
             they already received is not interrupted.",
        ),
    };
    let room = width.saturating_sub(2);
    let mut identity = vec![
        Span::raw(" "),
        Span::styled(
            text::fit(&text::one_line(&confirm.group_id), room),
            theme.dim(),
        ),
    ];
    if let Some(group) = app.groups.find(&confirm.group_id) {
        identity.push(Span::raw(text::fit(
            &format!(
                "  State {} | {} running",
                text::one_line(&group.state),
                count(
                    usize::try_from(group.running_actors).unwrap_or(usize::MAX),
                    "actor"
                )
            ),
            room.saturating_sub(text::width(&confirm.group_id) + 2),
        )));
    }
    let mut lines = vec![
        Line::styled(format!(" {verb} {name}?"), theme.bold()),
        Line::from(identity),
        Line::default(),
    ];
    let mut body = effect.to_owned();
    let may_have_native = app
        .selected_members()
        .is_none_or(|members| members.may_have_native());
    if may_have_native {
        body = format!("{body} {native}");
    }
    lines.extend(
        text::wrap(&body, room)
            .into_iter()
            .map(|line| Line::raw(format!(" {line}"))),
    );
    lines
}

fn members(
    app: &App,
    group: &GroupSummary,
    theme: Theme,
    now: Instant,
    width: usize,
    rows: usize,
) -> Vec<Line<'static>> {
    let Some(list) = app.selected_members() else {
        return vec![Line::styled(
            format!("{INDENT}reading members..."),
            theme.dim(),
        )];
    };
    debug_assert_eq!(list.group_id, group.group_id);
    let mut lines = Vec::new();
    match (&list.error, list.last_ok) {
        (None, None) => {
            return vec![Line::styled(
                format!("{INDENT}reading members..."),
                theme.dim(),
            )];
        }
        (Some(error), None) => {
            return vec![Line::styled(
                text::fit(
                    &format!("{INDENT}Could not read members: {}", text::one_line(error)),
                    width,
                ),
                theme.error(),
            )];
        }
        (Some(_), Some(last_ok)) => lines.push(Line::styled(
            format!(
                "{INDENT}members not updated (last ok {})",
                text::ago(now.saturating_duration_since(last_ok))
            ),
            theme.error(),
        )),
        (None, Some(_)) => {}
    }
    if list.members.is_empty() {
        lines.push(Line::raw(format!("{INDENT}No actors in this group.")));
        return lines;
    }
    let room = rows.saturating_sub(lines.len());
    let shown = if list.members.len() > room {
        room.saturating_sub(1)
    } else {
        list.members.len()
    };
    lines.extend(
        list.members[..shown]
            .iter()
            .map(|member| member_row(member, theme, width)),
    );
    let hidden = list.members.len() - shown;
    if hidden > 0 {
        lines.push(Line::styled(
            format!("{INDENT}+{hidden} more  open in the Web UI"),
            theme.dim(),
        ));
    }
    lines
}

fn member_row(member: &Member, theme: Theme, width: usize) -> Line<'static> {
    let fixed =
        INDENT.len() + 2 + RUNTIME_COLUMN + ROLE_COLUMN + STATE_COLUMN + NATIVE_TAG.len() + 1;
    let id_column = width.saturating_sub(fixed).min(ID_COLUMN_MAX);
    let id = text::fit(&text::one_line(&member.id), id_column);
    let pad = " ".repeat(id_column.saturating_sub(text::width(&id)) + 2);
    let runtime = text::fit(&text::one_line(&member.runtime), RUNTIME_COLUMN - 1);
    let role = text::fit(&text::one_line(&member.role), ROLE_COLUMN - 1);
    let state_style = match member.state {
        MemberState::Running | MemberState::Ready => theme.ok(),
        MemberState::Stopped
        | MemberState::Off
        | MemberState::NotReceiving
        | MemberState::AccessPaused => theme.dim(),
    };
    spread(
        vec![
            Span::raw(INDENT),
            Span::raw(format!("{id}{pad}")),
            Span::raw(format!("{runtime:<RUNTIME_COLUMN$}")),
            Span::styled(format!("{role:<ROLE_COLUMN$}"), theme.dim()),
        ],
        vec![
            Span::styled(
                format!("{:<STATE_COLUMN$}", member.state.label()),
                state_style,
            ),
            Span::styled(
                if member.native {
                    NATIVE_TAG.to_owned()
                } else {
                    " ".repeat(NATIVE_TAG.len())
                },
                theme.dim(),
            ),
            Span::raw(" "),
        ],
        width,
    )
}

/// What the terminal says, once the interface has closed, about a group
/// action the daemon had not answered.
pub(super) fn unanswered_receipt(app: &App) -> Option<String> {
    let running = app.action.running.as_ref()?;
    Some(format!(
        "{} {} had not finished when CCCC stopped; check the group's state.",
        running.action.doing(),
        text::one_line(&running.name)
    ))
}

fn row(area: Rect, y: u16, height: u16) -> Rect {
    Rect {
        x: area.x,
        y: area.y + y,
        width: area.width,
        height,
    }
}

#[cfg(test)]
#[path = "groups_view_tests.rs"]
mod tests;
