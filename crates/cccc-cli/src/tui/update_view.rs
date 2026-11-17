//! The `/update` screen: the installed and latest versions from one check,
//! then either one confirmation that this CCCC carries out, or the command
//! that updates this installation. A failed check never reads as current.

use super::app::{App, ConfirmChoice, Notice, UpdateOffer};
use super::groups_view::choice_row;
use super::text;
use super::update::UpdateProgress;
use super::view::{Theme, separator};
use cccc_web::UpdateCheck;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

// "Installed" and "Latest" share one column, with room before the value.
const LABEL: usize = 13;

pub(super) fn render(frame: &mut Frame, app: &App, theme: Theme) {
    let area = super::view::content_area(frame.area());
    let width = usize::from(area.width);
    let progress = app.update_progress();
    let confirm = app.update_confirmable().map(|_| {
        app.update_flow
            .as_ref()
            .map_or(ConfirmChoice::Cancel, |flow| flow.choice)
    });
    let (lines, keys) = body(app, progress, theme, width);
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
    let choices = if confirm.is_some() { 2 } else { 0 };
    let body_area = Rect {
        height: area
            .height
            .saturating_sub(2 + u16::from(notice.is_some()) + choices),
        ..area
    };
    frame.render_widget(Paragraph::new(lines), body_area);
    if let Some(choice) = confirm {
        frame.render_widget(
            Paragraph::new(vec![
                choice_row("Cancel", choice == ConfirmChoice::Cancel, theme, width),
                choice_row(
                    "Update and restart agents",
                    choice == ConfirmChoice::Proceed,
                    theme,
                    width,
                ),
            ]),
            Rect {
                y: area.y + body_area.height,
                height: 2,
                ..area
            },
        );
    }
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
    let keys = if confirm.is_some() {
        "Up/Down choose   Enter select   Esc back"
    } else {
        keys
    };
    frame.render_widget(
        Paragraph::new(Line::styled(format!(" {keys}"), theme.dim())),
        Rect {
            y: area.y + area.height - 1,
            height: 1,
            ..area
        },
    );
}

fn body(
    app: &App,
    progress: Option<&UpdateProgress>,
    theme: Theme,
    width: usize,
) -> (Vec<Line<'static>>, &'static str) {
    let room = width.saturating_sub(2);
    let mut lines = vec![
        Line::from(vec![
            Span::raw(" "),
            Span::styled("Update CCCC", theme.bold()),
        ]),
        Line::default(),
        labelled("Installed", Span::raw(app.context.version), theme),
    ];
    let check = progress.and_then(|progress| progress.check.as_ref());
    let Some(check) = check.filter(|_| progress.is_some_and(|progress| !progress.checking)) else {
        lines.push(labelled(
            "Latest",
            Span::styled("Checking...", theme.dim()),
            theme,
        ));
        return (lines, "Esc back");
    };
    let Some(latest) = check.latest.as_deref() else {
        lines.push(labelled(
            "Latest",
            Span::styled("Check failed", theme.error()),
            theme,
        ));
        let reason = check.failure.as_deref().unwrap_or("no reason was given");
        for line in text::wrap(&text::one_line(reason), width.saturating_sub(LABEL + 2)) {
            lines.push(Line::styled(
                format!(" {}{line}", " ".repeat(LABEL)),
                theme.dim(),
            ));
        }
        return (lines, "r check again   Esc back");
    };
    lines.push(labelled("Latest", Span::raw(text::one_line(latest)), theme));
    let actionable = matches!(check.status.as_deref(), Some("newer" | "channel_switch"));
    if !actionable {
        lines.push(Line::default());
        let message = match check.status.as_deref() {
            Some("current") => "CCCC is up to date.",
            Some("installed_newer") => "This CCCC is newer than the latest release.",
            Some("no_package") => "No CCCC package is published for this platform.",
            _ => "There is nothing to update.",
        };
        lines.push(Line::raw(format!(" {}", text::fit(message, room))));
        return (lines, "r check again   Esc back");
    }
    if let Some(url) = &check.release_notes_url {
        lines.push(Line::styled(" Release notes", theme.dim()));
        for line in text::wrap_exact(&text::one_line(url), width.saturating_sub(4)) {
            lines.push(Line::raw(format!("   {line}")));
        }
    }
    lines.push(Line::default());
    let progress = progress.expect("a finished check belongs to this visit");
    let keys = if let Some(version) = &progress.preparing {
        paragraph(
            &mut lines,
            &format!(
                "Preparing CCCC {}: downloading and checking it. CCCC keeps running meanwhile.",
                text::one_line(version)
            ),
            Style::new(),
            room,
        );
        "Esc give up"
    } else if progress.handed_over {
        paragraph(&mut lines, "Updating...", Style::new(), room);
        ""
    } else if let Some(reason) = &progress.rejected {
        paragraph(
            &mut lines,
            &format!(
                "The update did not start: {}. CCCC was not updated and no agent was stopped.",
                text::one_line(reason).trim_end_matches('.')
            ),
            theme.error(),
            room,
        );
        "r check again   Esc back"
    } else if progress.given_up {
        paragraph(
            &mut lines,
            "The update was given up. CCCC was not updated and no agent was stopped.",
            Style::new(),
            room,
        );
        "r check again   Esc back"
    } else if app.context.update == UpdateOffer::InPlace {
        paragraph(
            &mut lines,
            &format!(
                "CCCC stops, installs {} and starts again here; running agents restart with it. The Web disconnects until then.",
                text::one_line(latest)
            ),
            Style::new(),
            room,
        );
        "r check again   Esc back"
    } else {
        by_hand(&mut lines, check, room);
        "r check again   Esc back"
    };
    (lines, keys)
}

/// A host that cannot replace itself names the command for this installation.
fn by_hand(lines: &mut Vec<Line<'static>>, check: &UpdateCheck, room: usize) {
    match check.command.as_deref() {
        Some(command) => {
            let lead = if check.installation == "pip" {
                "This CCCC was installed with pip. Stop CCCC, then run this in the Python environment that installed it:"
            } else {
                "Update it from a terminal with:"
            };
            paragraph(lines, lead, Style::new(), room);
            for line in text::wrap_exact(&text::one_line(command), room.saturating_sub(2)) {
                lines.push(Line::raw(format!("   {line}")));
            }
        }
        None => paragraph(
            lines,
            "This CCCC was not installed by the CCCC installer or pip; update it the way it was installed.",
            Style::new(),
            room,
        ),
    }
}

fn labelled(label: &str, value: Span<'static>, theme: Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!(" {label:<width$}", width = LABEL - 1), theme.dim()),
        Span::raw(" "),
        value,
    ])
}

fn paragraph(lines: &mut Vec<Line<'static>>, message: &str, style: Style, room: usize) {
    for line in text::wrap(message, room) {
        lines.push(Line::styled(format!(" {line}"), style));
    }
}

#[cfg(test)]
#[path = "update_view_tests.rs"]
mod tests;
