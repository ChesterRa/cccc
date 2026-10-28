//! Read-only source directory. This uses the same snapshot as the main
//! screen; no session mutation, provider query or history scan is needed.
use super::app::App;
use super::text;
use super::view::{
    INDENT, Theme, content_area, list_message, notice_line, runtime_label, separator,
    sessions_message, sessions_title, window_start,
};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use std::time::Instant;

pub(super) fn render(frame: &mut Frame, app: &App, theme: Theme, now: Instant) {
    let area = content_area(frame.area());
    let width = usize::from(area.width);
    frame.render_widget(
        Paragraph::new(sessions_title(app, theme, width, now)),
        row(area, 0, 1),
    );
    let body = row(area, 2, area.height.saturating_sub(5));
    if app.sessions_view.details {
        let mut lines = Vec::new();
        if let Some(source) = app.selected_source() {
            for (label, value) in [
                ("Title", source.title.as_str()),
                ("App", runtime_label(source.runtime)),
                (
                    "Receive",
                    if source.connected {
                        "ready to receive"
                    } else {
                        "not receiving"
                    },
                ),
                ("Directory", source.cwd.as_str()),
                ("Source ID", source.source_id.as_str()),
                ("Native ID", source.native_session_id.as_str()),
                ("Origin", source.origin.as_deref().unwrap_or("not reported")),
            ] {
                let value = text::one_line(value);
                let parts = text::wrap_exact(
                    if value.is_empty() {
                        "(not reported)"
                    } else {
                        &value
                    },
                    width.saturating_sub(13),
                );
                for (index, part) in parts.into_iter().enumerate() {
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!(" {:<11}", if index == 0 { label } else { "" }),
                            theme.dim(),
                        ),
                        Span::raw(part),
                    ]));
                }
            }
            lines.push(Line::default());
            lines.extend(text::wrap("Ready describes the receive channel, not model activity or access to any particular group.", width.saturating_sub(2)).into_iter().map(|part| Line::styled(format!(" {part}"), theme.dim())));
        } else {
            lines.push(Line::styled(
                " The selected session is no longer listed. Go back to choose another.",
                theme.error(),
            ));
        }
        let (top, _) = app.sessions_view.scroll.fit(lines.len(), body.height);
        frame.render_widget(Paragraph::new(lines).scroll((top, 0)), body);
    } else if app.sessions.sources.is_empty() {
        if let Some(message) = sessions_message(app, theme) {
            list_message(frame, message, body);
        }
    } else {
        let selected = app
            .sessions
            .sources
            .iter()
            .position(|source| Some(&source.source_id) == app.sessions_view.selected.as_ref());
        let rows = usize::from(body.height);
        app.sessions_view.page.set(body.height);
        let start = window_start(app.sessions.sources.len(), rows, selected);
        let name_width = width.saturating_sub(32).min(46);
        let lines: Vec<Line> = app
            .sessions
            .sources
            .iter()
            .enumerate()
            .skip(start)
            .take(rows)
            .map(|(index, source)| {
                let selected = Some(index) == selected;
                let name = if source.title.trim().is_empty() {
                    &source.cwd
                } else {
                    &source.title
                };
                let name = text::fit(&text::one_line(name), name_width);
                let pad = " ".repeat(name_width.saturating_sub(text::width(&name)));
                let (status, style) = if source.connected {
                    ("ready", theme.ok())
                } else {
                    ("not receiving", theme.dim())
                };
                Line::from(vec![
                    Span::raw(if selected { " > " } else { INDENT }),
                    Span::raw(format!("{:<13}", runtime_label(source.runtime))),
                    Span::raw(format!("{name}{pad}  ")),
                    Span::styled(status, if selected { Default::default() } else { style }),
                ])
                .style(if selected {
                    theme.selected()
                } else {
                    Default::default()
                })
            })
            .collect();
        frame.render_widget(Paragraph::new(lines), body);
    }
    let position = app
        .sessions
        .sources
        .iter()
        .position(|source| Some(&source.source_id) == app.sessions_view.selected.as_ref())
        .map(|index| index + 1);
    let hint = position.map_or_else(
        || " Choose a session with Up/Down.".to_owned(),
        |position| {
            if app.sessions_view.details {
                format!(" {position}/{}", app.sessions.sources.len())
            } else {
                format!(
                    " {position}/{}   Receive readiness is not model activity.",
                    app.sessions.sources.len()
                )
            }
        },
    );
    frame.render_widget(
        Paragraph::new(
            notice_line(app, theme, width)
                .unwrap_or_else(|| Line::styled(text::fit(&hint, width), theme.dim())),
        ),
        row(area, area.height - 3, 1),
    );
    frame.render_widget(
        Paragraph::new(separator(theme, width)),
        row(area, area.height - 2, 1),
    );
    let keys = if app.sessions_view.details {
        " Up/Down scroll   PgUp/PgDn   b back"
    } else {
        " Up/Down choose   Enter details   b back"
    };
    frame.render_widget(
        Paragraph::new(Line::styled(keys, theme.dim())),
        row(area, area.height - 1, 1),
    );
}

fn row(area: Rect, y: u16, height: u16) -> Rect {
    Rect {
        y: area.y + y,
        height,
        ..area
    }
}
