//! The shared confirmation and progress screen for `/update` and `/restart`.

use super::app::{App, Notice, UpdateChoice, UpdateOffer};
use super::groups_view::choice_row;
use super::text;
use super::update::UpdateProgress;
use super::view::{Theme, separator};
use cccc_web::{LocalBuild, UpdateCheck, UpdateCheckScope};
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
    let options = app.update_options();
    let choice = app
        .update_flow
        .as_ref()
        .map_or(UpdateChoice::Cancel, |flow| flow.choice);
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
    let choices = u16::try_from(options.len()).unwrap_or(u16::MAX);
    let body_area = Rect {
        height: area
            .height
            .saturating_sub(2 + u16::from(notice.is_some()))
            .saturating_sub(choices),
        ..area
    };
    frame.render_widget(Paragraph::new(lines), body_area);
    if !options.is_empty() {
        let rows = options
            .iter()
            .map(|&(option, enabled)| {
                if enabled {
                    choice_row(option_label(option), option == choice, theme, width)
                } else {
                    // Shown, so it is clear why it cannot be chosen; never selected.
                    let label = text::fit(option_label(option), width.saturating_sub(4));
                    Line::styled(format!("   {label}"), theme.dim())
                }
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(rows),
            Rect {
                y: area.y + body_area.height,
                height: choices.min(area.height.saturating_sub(body_area.height)),
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
    let keys = if !options.is_empty() {
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
    let restart = app
        .update_flow
        .as_ref()
        .is_some_and(|flow| flow.scope == UpdateCheckScope::Local);
    let title = if restart {
        "Restart CCCC"
    } else {
        "Update CCCC"
    };
    let mut lines = vec![
        Line::from(vec![Span::raw(" "), Span::styled(title, theme.bold())]),
        Line::default(),
        labelled("Installed", Span::raw(app.context.version), theme),
    ];
    let check = progress.and_then(|progress| progress.check.as_ref());
    let Some(check) = check.filter(|_| progress.is_some_and(|progress| !progress.checking)) else {
        lines.push(labelled(
            if restart { "Program" } else { "Latest" },
            Span::styled("Checking...", theme.dim()),
            theme,
        ));
        return (lines, "Esc back");
    };
    if restart && let Some(local) = &check.local {
        local_lines(&mut lines, local, theme, width);
    }
    let release = !restart && release_lines(&mut lines, check, theme, width);
    let local = app.offered_local(check);
    if !release && local.is_none() {
        return (lines, "r check again   Esc back");
    }
    lines.push(Line::default());
    let progress = progress.expect("a finished check belongs to this visit");
    let (what, done) = if restart {
        ("restart", "CCCC did not restart")
    } else {
        ("update", "CCCC was not updated")
    };
    let keys = if let Some(version) = &progress.preparing {
        let message = if restart {
            "Checking the program. CCCC keeps running meanwhile.".to_owned()
        } else {
            format!(
                "Preparing CCCC {}: downloading and checking it. CCCC keeps running meanwhile.",
                text::one_line(version)
            )
        };
        paragraph(&mut lines, &message, Style::new(), room);
        "Esc give up"
    } else if progress.handed_over {
        let message = if restart {
            "Restarting..."
        } else {
            "Updating..."
        };
        paragraph(&mut lines, message, Style::new(), room);
        ""
    } else if let Some(reason) = &progress.rejected {
        paragraph(
            &mut lines,
            &format!(
                "The {what} did not start: {}. {done} and no agent was stopped.",
                text::one_line(reason).trim_end_matches('.')
            ),
            theme.error(),
            room,
        );
        "r check again   Esc back"
    } else if progress.given_up {
        paragraph(
            &mut lines,
            &format!("The {what} was given up. {done} and no agent was stopped."),
            Style::new(),
            room,
        );
        "r check again   Esc back"
    } else {
        if let Some(local) = local {
            local_offer(&mut lines, local, theme, room);
        }
        if release {
            if local.is_some() {
                lines.push(Line::default());
            }
            let latest = check.latest.as_deref().unwrap_or_default();
            if app.context.update == UpdateOffer::InPlace && app.context.release_keep {
                paragraph(
                    &mut lines,
                    &format!(
                        "CCCC stops, installs {} and starts again here. The Web disconnects until then.",
                        text::one_line(latest)
                    ),
                    Style::new(),
                    room,
                );
                paragraph(
                    &mut lines,
                    "Keeping agents downloads and checks the release first; if they cannot be kept, CCCC and its agents keep running.",
                    theme.dim(),
                    room,
                );
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
            } else {
                by_hand(&mut lines, check, room);
            }
        }
        "r check again   Esc back"
    };
    (lines, keys)
}

/// The latest release, or why it is unknown; true when it is newer.
fn release_lines(
    lines: &mut Vec<Line<'static>>,
    check: &UpdateCheck,
    theme: Theme,
    width: usize,
) -> bool {
    let room = width.saturating_sub(2);
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
        return false;
    };
    lines.push(labelled("Latest", Span::raw(text::one_line(latest)), theme));
    if !matches!(check.status.as_deref(), Some("newer" | "channel_switch")) {
        lines.push(Line::default());
        let message = match check.status.as_deref() {
            Some("current") => "CCCC is up to date.",
            Some("installed_newer") => "This CCCC is newer than the latest release.",
            Some("no_package") => "No CCCC package is published for this platform.",
            _ => "There is nothing to update.",
        };
        lines.push(Line::raw(format!(" {}", text::fit(message, room))));
        return false;
    }
    if let Some(url) = &check.release_notes_url {
        lines.push(Line::styled(" Release notes", theme.dim()));
        for line in text::wrap_exact(&text::one_line(url), width.saturating_sub(4)) {
            lines.push(Line::raw(format!("   {line}")));
        }
    }
    true
}

/// What the local restart does, and why agents cannot keep
/// running through it this time, in CCCC's own words.
fn local_offer(lines: &mut Vec<Line<'static>>, local: &LocalBuild, theme: Theme, room: usize) {
    let Some(candidate) = local.candidate() else {
        return;
    };
    let version = text::one_line(&candidate.version);
    let program = match local {
        LocalBuild::Rebuilt { .. } => {
            let built = local
                .built_ago()
                .map_or_else(String::new, |ago| format!(", built {}", text::ago(ago)));
            format!("the new local build ({version}{built})")
        }
        _ => format!("the current program ({version})"),
    };
    paragraph(
        lines,
        &format!("CCCC starts again here from {program}. The Web disconnects until then."),
        Style::new(),
        room,
    );
    if !candidate.keep.allowed {
        paragraph(
            lines,
            "Agents cannot keep running this time:",
            theme.dim(),
            room,
        );
        for refusal in &candidate.keep.refusals {
            let reason = crate::update_host::refusal_text(refusal);
            for (index, line) in text::wrap(&text::one_line(&reason), room.saturating_sub(2))
                .into_iter()
                .enumerate()
            {
                let lead = if index == 0 { "- " } else { "  " };
                lines.push(Line::styled(format!(" {lead}{line}"), theme.dim()));
            }
        }
    }
}

fn option_label(option: UpdateChoice) -> &'static str {
    match option {
        UpdateChoice::Cancel => "Cancel",
        // What happens to agents leads, so a narrow terminal keeps the difference.
        UpdateChoice::KeepAgents => "Keep agents and restart CCCC",
        UpdateChoice::RestartAgents => "Restart agents with CCCC",
        UpdateChoice::ReleaseKeep => "Keep agents; update",
        UpdateChoice::Release => "Update and restart agents",
    }
}

/// The candidate at the launch path, shown as a separate line when changed.
fn local_lines(lines: &mut Vec<Line<'static>>, local: &LocalBuild, theme: Theme, width: usize) {
    match local {
        LocalBuild::Current { .. } => {}
        LocalBuild::Rebuilt { candidate, .. } => {
            let built = local
                .built_ago()
                .map_or_else(String::new, |ago| format!(" · built {}", text::ago(ago)));
            lines.push(labelled(
                "Local build",
                Span::raw(format!("{}{built}", text::one_line(&candidate.version))),
                theme,
            ));
        }
        LocalBuild::Incompatible { version } => {
            lines.push(labelled(
                "Local build",
                Span::styled(
                    format!(
                        "{} cannot complete a restart from this version",
                        text::one_line(version)
                    ),
                    theme.error(),
                ),
                theme,
            ));
        }
        LocalBuild::Unreadable { reason } => {
            lines.push(labelled(
                "Local build",
                Span::styled("Could not read the program", theme.error()),
                theme,
            ));
            for line in text::wrap(&text::one_line(reason), width.saturating_sub(LABEL + 2)) {
                lines.push(Line::styled(
                    format!(" {}{line}", " ".repeat(LABEL)),
                    theme.dim(),
                ));
            }
        }
    }
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
