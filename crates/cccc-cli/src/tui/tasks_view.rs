//! One focus per page: task rows, or one task's scrollable complete contents.
use super::app::App;
use super::tasks::{self, Counts, Data, Snapshot, Task};
use super::text;
use super::view::{Theme, content_area, notice_line, separator, window_start};
use crate::host_state::WebState;
use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
};
use std::time::Instant;

pub(super) fn render(frame: &mut Frame, app: &App, theme: Theme, now: Instant) {
    let area = content_area(frame.area());
    let width = usize::from(area.width);
    let view = &app.tasks_view;
    let snapshot = app.selected_tasks();
    let title = if view.details {
        format!(" Task {}", view.selected.as_deref().unwrap_or_default())
    } else {
        format!(" Tasks - {}", view.group_name)
    };
    frame.render_widget(
        Paragraph::new(Line::styled(
            text::fit(&text::one_line(&title), width),
            theme.bold(),
        )),
        row(area, 0, 1),
    );
    let identity = if view.details {
        format!(" Group {}", view.group_id)
    } else {
        format!(
            " {} | {} | Page {}  PgUp/PgDn pages",
            view.group_id,
            if view.done { "Done" } else { "Open" },
            view.page.saturating_add(1)
        )
    };
    frame.render_widget(
        Paragraph::new(Line::styled(text::fit(&identity, width), theme.dim())),
        row(area, 1, 1),
    );

    if view.details {
        let body = row(area, 2, area.height.saturating_sub(5));
        if let Some(Data::Detail(task)) = snapshot.and_then(|s| s.data.as_ref()) {
            let lines = details(task, theme, width);
            let (top, _) = view.scroll.fit(lines.len(), body.height);
            frame.render_widget(Paragraph::new(lines).scroll((top, 0)), body);
        } else {
            message(frame, snapshot, theme, body);
        }
    } else {
        let body = row(area, 5, area.height.saturating_sub(8));
        if let Some(page) = snapshot.and_then(Snapshot::page) {
            frame.render_widget(
                Paragraph::new(Line::styled(counts(&page.counts), theme.dim())),
                row(area, 2, 1),
            );
            frame.render_widget(
                Paragraph::new(Line::styled(
                    text::fit(&attention(&page.counts), width),
                    theme.dim(),
                )),
                row(area, 3, 1),
            );
            frame.render_widget(Paragraph::new(columns(theme, width)), row(area, 4, 1));
            if page.tasks.is_empty() {
                let line = if view.page > 0 {
                    " No tasks on this page. PgUp returns to the previous page."
                } else if view.done {
                    " No completed tasks in this Group."
                } else {
                    " No active or planned tasks. Press d to read completed tasks."
                };
                frame.render_widget(
                    Paragraph::new(
                        text::wrap(line, width)
                            .into_iter()
                            .map(Line::raw)
                            .collect::<Vec<_>>(),
                    ),
                    body,
                );
            } else {
                let selected = page
                    .tasks
                    .iter()
                    .position(|task| Some(&task.id) == view.selected.as_ref());
                let start = window_start(page.tasks.len(), usize::from(body.height), selected);
                let lines: Vec<_> = page
                    .tasks
                    .iter()
                    .enumerate()
                    .skip(start)
                    .take(usize::from(body.height))
                    .map(|(index, task)| task_row(task, selected == Some(index), theme, width))
                    .collect();
                frame.render_widget(Paragraph::new(lines), body);
            }
        } else {
            message(frame, snapshot, theme, body);
        }
    }
    let footer = notice_line(app, theme, width)
        .or_else(|| stale(snapshot, theme, now, width))
        .unwrap_or_else(|| {
            let url = match &app.host.web {
                WebState::Listening { url } => format!(" Web {url}/?group={}", view.group_id),
                _ => " Read-only. Edit tasks in the Web UI.".into(),
            };
            Line::styled(text::fit(&url, width), theme.dim())
        });
    frame.render_widget(Paragraph::new(footer), row(area, area.height - 3, 1));
    frame.render_widget(
        Paragraph::new(separator(theme, width)),
        row(area, area.height - 2, 1),
    );
    let keys = if view.details {
        " Up/Down scroll   PgUp/PgDn   Home/End   b tasks".into()
    } else {
        format!(
            " Up/Down task   Enter details   d {}   b groups",
            if view.done { "open" } else { "done" }
        )
    };
    frame.render_widget(
        Paragraph::new(Line::styled(keys, theme.dim())),
        row(area, area.height - 1, 1),
    );
}

pub(super) fn summary(app: &App, theme: Theme, width: usize) -> Line<'static> {
    let snapshot = app.selected_tasks();
    let (label, style) = if let Some(page) = snapshot.and_then(Snapshot::page) {
        let stale = snapshot.is_some_and(|s| s.error.is_some());
        (
            format!(
                " Tasks: {} active  {} planned  {} done{}  t view",
                page.counts.active,
                page.counts.planned,
                page.counts.done,
                if stale { " (stale)" } else { "" }
            ),
            if stale { theme.error() } else { theme.dim() },
        )
    } else if snapshot.is_some_and(|s| s.error.is_some()) {
        (
            " Tasks unavailable; t opens details of the error".into(),
            theme.error(),
        )
    } else {
        (" Tasks: reading...  t view".into(), theme.dim())
    };
    Line::styled(text::fit(&label, width), style)
}

fn message(frame: &mut Frame, snapshot: Option<&Snapshot>, theme: Theme, area: Rect) {
    let (message, style) = if let Some(error) = snapshot.and_then(|s| s.error.as_deref()) {
        (
            format!(" Could not read tasks: {}", text::one_line(error)),
            theme.error(),
        )
    } else {
        (" Reading tasks...".into(), theme.dim())
    };
    let lines: Vec<_> = text::wrap(&message, usize::from(area.width).saturating_sub(1))
        .into_iter()
        .map(|line| Line::styled(line, style))
        .collect();
    frame.render_widget(Paragraph::new(lines), area);
}

fn stale(
    snapshot: Option<&Snapshot>,
    theme: Theme,
    now: Instant,
    width: usize,
) -> Option<Line<'static>> {
    let snapshot = snapshot?;
    snapshot.error.as_ref()?;
    let at = snapshot.last_ok?;
    Some(Line::styled(
        text::fit(
            &format!(
                " Tasks not updated; last ok {}. Retrying.",
                text::ago(now.saturating_duration_since(at))
            ),
            width,
        ),
        theme.error(),
    ))
}

fn counts(counts: &Counts) -> String {
    format!(
        " Active {}  Planned {}  Done {}",
        counts.active, counts.planned, counts.done
    )
}

fn attention(counts: &Counts) -> String {
    let mut parts = Vec::new();
    for (label, count) in [
        ("Waiting user", counts.waiting_user),
        ("Blocked", counts.blocked),
        ("Handoff", counts.handoffs),
    ] {
        if count > 0 {
            parts.push(format!("{label} {count}"));
        }
    }
    format!(" {}", parts.join("  "))
}

fn columns(theme: Theme, width: usize) -> Line<'static> {
    let name = width.saturating_sub(36);
    Line::styled(
        format!(
            "   {:<7} {:<8} {:<name$} {}",
            "ID", "STATUS", "TASK", "OWNER"
        ),
        theme.dim(),
    )
}

fn task_row(task: &Task, selected: bool, theme: Theme, width: usize) -> Line<'static> {
    let name_width = width.saturating_sub(36);
    let id = text::fit(&text::one_line(&task.id), 7);
    let name = text::fit(&text::one_line(task.name()), name_width);
    let owner = if task.assignee.is_empty() {
        "unassigned"
    } else {
        &task.assignee
    };
    let line = format!(
        "{}{}{} {:<8} {}{} {}",
        if selected { " > " } else { "   " },
        id,
        " ".repeat(7usize.saturating_sub(text::width(&id))),
        tasks::status_label(&task.status),
        name,
        " ".repeat(name_width.saturating_sub(text::width(&name))),
        text::fit(&text::one_line(owner), 14)
    );
    Line::styled(
        text::fit(&line, width),
        if selected {
            theme.selected()
        } else {
            Default::default()
        },
    )
}

fn details(task: &Task, theme: Theme, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let fields = &task.fields;
    for (label, value) in [
        ("Title", task.name().to_owned()),
        ("Status", tasks::status_label(&task.status).to_owned()),
        (
            "Owner",
            if task.assignee.is_empty() {
                "unassigned".into()
            } else {
                task.assignee.clone()
            },
        ),
        ("Priority", tasks::string(fields, "priority")),
        ("Waiting on", tasks::string(fields, "waiting_on")),
        (
            "Blocked by",
            fields
                .get("blocked_by")
                .and_then(serde_json::Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default(),
        ),
        ("Handoff to", tasks::string(fields, "handoff_to")),
        ("Parent", tasks::string(fields, "parent_id")),
        ("Updated", tasks::string(fields, "updated_at")),
    ] {
        if value.is_empty() || label == "Waiting on" && value == "none" {
            continue;
        }
        for (index, part) in text::wrap_exact(&text::one_line(&value), width.saturating_sub(13))
            .into_iter()
            .enumerate()
        {
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {:<11}", if index == 0 { label } else { "" }),
                    theme.dim(),
                ),
                Span::raw(part),
            ]));
        }
    }
    let checklist = fields
        .get("checklist")
        .and_then(serde_json::Value::as_array);
    if let Some(items) = checklist.filter(|items| !items.is_empty()) {
        let total = items.len();
        let done = items.iter().filter(|item| item["status"] == "done").count();
        lines.push(Line::default());
        lines.push(Line::styled(
            format!(" Checklist {done}/{total} done"),
            theme.bold(),
        ));
        for item in items {
            let mark = match item["status"].as_str() {
                Some("done") => "[x]",
                Some("in_progress") => "[~]",
                _ => "[ ]",
            };
            for (index, part) in text::wrap_exact(
                &text::one_line(item["text"].as_str().unwrap_or_default()),
                width.saturating_sub(7),
            )
            .into_iter()
            .enumerate()
            {
                lines.push(Line::raw(format!(
                    " {} {part}",
                    if index == 0 { mark } else { "   " }
                )));
            }
        }
    }
    for (label, key, empty) in [
        ("Notes", "notes", "(no notes)"),
        ("Outcome", "outcome", "(not recorded)"),
    ] {
        lines.push(Line::default());
        lines.push(Line::styled(format!(" {label}"), theme.bold()));
        let value = tasks::string(fields, key);
        let value = if value.is_empty() { empty } else { &value };
        for line in value.lines() {
            for part in text::wrap_exact(&text::one_line(line), width.saturating_sub(2)) {
                lines.push(Line::raw(format!(" {part}")));
            }
        }
    }
    lines
}

fn row(area: Rect, y: u16, height: u16) -> Rect {
    Rect {
        y: area.y + y,
        height,
        ..area
    }
}
