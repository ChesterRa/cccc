//! The selected group's details, tasks, members and confirmed actions. Nothing here
//! says more than the daemon's last answer: a done action only means the
//! daemon accepted it, and the rows below show what it reports afterwards.

use super::app::{App, ConfirmChoice, GroupChoice};
use super::groups::{GroupAction, GroupCommand, GroupSummary, Member, MemberState};
use super::text;
use super::view::{
    INDENT, STATUS_BLOCK, Theme, count, flexible, groups_message, list_message, notice_line,
    separator, status_block,
};
use crate::host_state::WebState;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use std::time::Instant;

// The longest runtime label, "ChatGPT Dots", and "foreman", each with room
// after it. On the narrowest screens the runtime gives way before an id gets
// shorter than ID_COLUMN_MIN.
const RUNTIME_COLUMN: usize = 13;
const ROLE_COLUMN: usize = 9;
const ID_COLUMN_MIN: usize = 10;
pub(super) fn render(frame: &mut Frame, app: &App, theme: Theme, now: Instant) {
    let area = super::view::content_area(frame.area());
    let width = usize::from(area.width);
    if let Some(confirm) = &app.groups_view.confirm {
        frame.render_widget(
            Paragraph::new(
                freshness(app, theme, now, width)
                    .unwrap_or_else(|| Line::styled(" Group", theme.bold())),
            ),
            row(area, 0, 1),
        );
        // The question takes the whole body, so even the smallest screen
        // shows all of what Enter would do.
        frame.render_widget(
            Paragraph::new(confirmation(app, confirm, theme, width)),
            row(area, 2, area.height.saturating_sub(7)),
        );
        let action = match confirm.action {
            GroupAction::Start { resume: false } => "Start group",
            GroupAction::Start { resume: true } => "Resume group",
            GroupAction::Stop => "Stop group",
        };
        frame.render_widget(
            Paragraph::new(vec![
                choice_row(
                    "Cancel",
                    app.groups_view.confirm_choice == ConfirmChoice::Cancel,
                    theme,
                    width,
                ),
                choice_row(
                    action,
                    app.groups_view.confirm_choice == ConfirmChoice::Proceed,
                    theme,
                    width,
                ),
            ]),
            row(area, area.height - 5, 2),
        );
    } else {
        details(
            frame,
            app,
            theme,
            now,
            row(area, 0, area.height.saturating_sub(3)),
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
    if app.groups_view.confirm.is_some() {
        return " Up/Down choose   Enter select   Esc cancel".into();
    }
    if let Some(busy) = app.busy_action() {
        return format!(
            " {} {}...   Esc back",
            busy.action.doing(),
            text::fit(&text::one_line(&busy.name), 32)
        );
    }
    " Up/Down choose   Enter open   Esc back".into()
}

fn details(frame: &mut Frame, app: &App, theme: Theme, now: Instant, area: Rect) {
    let width = usize::from(area.width);
    let rows = usize::from(area.height);
    let Some(group) = app.selected_group() else {
        if app.groups_view.selected.is_some() {
            let message = (
                "The selected group is no longer listed. Esc returns to the group list.".to_owned(),
                theme.error(),
            );
            list_message(frame, message, area);
        } else if let Some(message) = groups_message(app, theme) {
            list_message(frame, message, area);
        } else {
            list_message(frame, ("Reading groups...".into(), theme.dim()), area);
        }
        return;
    };
    let choices = app.group_choices();
    let stale = freshness(app, theme, now, width);
    // Reserve the identity, Web address, all actions and room for a member
    // and an overflow indicator before spending rows on a long group name.
    let reserved = 1
        + usize::from(matches!(&app.host.web, WebState::Listening { .. }))
        + usize::from(stale.is_some())
        + choices.len()
        + 5;
    let title_rows = rows.saturating_sub(reserved).max(1);
    let title_lines = text::wrap_exact(&text::one_line(group.name()), width.saturating_sub(2));
    let mut lines: Vec<Line> = title_lines
        .iter()
        .take(title_rows)
        .enumerate()
        .map(|(index, line)| {
            let line = if index + 1 == title_rows && title_lines.len() > title_rows {
                text::fit(&format!("{line}..."), width.saturating_sub(2))
            } else {
                line.clone()
            };
            Line::styled(format!(" {line}"), theme.bold())
        })
        .collect();
    if let Some(stale) = stale {
        lines.push(stale);
    }
    lines.push(Line::raw(text::fit(
        &format!(
            " {}  {} | State {} | {} running",
            text::one_line(&group.group_id),
            group.status.label(),
            text::one_line(&group.state),
            count(
                usize::try_from(group.running_actors).unwrap_or(usize::MAX),
                "actor"
            ),
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
    lines.push(Line::default());
    for choice in choices {
        let label = match choice {
            GroupChoice::Tasks => super::tasks_view::summary(app),
            GroupChoice::Start => "Start group".into(),
            GroupChoice::Resume => "Resume group".into(),
            GroupChoice::Stop => "Stop group".into(),
            GroupChoice::Pending => match app.pending_summary.count(&group.group_id) {
                Some(super::pending::Count::Fresh(count)) => format!("Waiting for you: {count}"),
                Some(super::pending::Count::Stale(count)) if count > 0 => {
                    format!("Waiting for you: ~{count} (stale)")
                }
                _ => "Waiting for you: unknown".into(),
            },
        };
        lines.push(choice_row(
            &label,
            choice == app.groups_view.choice,
            theme,
            width,
        ));
    }
    lines.push(Line::default());
    lines.push(Line::styled(" Members", theme.bold()));
    let room = rows.saturating_sub(lines.len());
    lines.extend(members(app, group, theme, now, width, room));
    lines.truncate(rows);
    frame.render_widget(Paragraph::new(lines), area);
}

fn freshness(app: &App, theme: Theme, now: Instant, width: usize) -> Option<Line<'static>> {
    app.groups.error.as_ref()?;
    let status = match app.groups.last_ok {
        Some(last_ok) => format!(
            "Group not updated (last ok {})",
            text::ago(now.saturating_duration_since(last_ok))
        ),
        None => "Group data not available".into(),
    };
    Some(Line::styled(
        format!(" {}", text::fit(&status, width.saturating_sub(2))),
        theme.error(),
    ))
}

pub(super) fn choice_row(label: &str, selected: bool, theme: Theme, width: usize) -> Line<'static> {
    let label = text::fit(label, width.saturating_sub(4));
    let padding = " ".repeat(width.saturating_sub(4 + text::width(&label)));
    Line::styled(
        format!(" {} {label}{padding}", if selected { ">" } else { " " }),
        if selected {
            theme.selected()
        } else {
            Style::new()
        },
    )
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
            "Its enabled actors that are not running start; actors that are off stay off.",
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
    // The id and runtime share what the role and status block leave, so
    // member states use the same column as statuses on the main screen.
    let room = flexible(width, INDENT.len() + 2 + ROLE_COLUMN + STATUS_BLOCK);
    let runtime_column = RUNTIME_COLUMN.min(room.saturating_sub(ID_COLUMN_MIN));
    let id_column = room - runtime_column;
    let id = text::fit(&text::one_line(&member.id), id_column);
    let pad = " ".repeat(id_column.saturating_sub(text::width(&id)) + 2);
    let runtime = text::fit(
        &text::one_line(&member.runtime),
        runtime_column.saturating_sub(1),
    );
    let runtime_pad = " ".repeat(runtime_column.saturating_sub(text::width(&runtime)));
    let role = text::fit(&text::one_line(&member.role), ROLE_COLUMN - 1);
    let state_style = match member.state {
        MemberState::Running | MemberState::Ready => theme.ok(),
        MemberState::Stopped
        | MemberState::Off
        | MemberState::NotReceiving
        | MemberState::AccessPaused => theme.dim(),
    };
    let mut spans = vec![
        Span::raw(INDENT),
        Span::raw(format!("{id}{pad}{runtime}{runtime_pad}")),
        Span::styled(format!("{role:<ROLE_COLUMN$}"), theme.dim()),
    ];
    spans.extend(status_block(
        member.state.label(),
        state_style,
        if member.native { "native" } else { "" },
        theme.dim(),
    ));
    Line::from(spans)
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
