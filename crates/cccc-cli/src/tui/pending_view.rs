//! One list and one answer at a time; no option is selected implicitly.
use super::app::{App, ReplySelection};
use super::pending::Status;
use super::text;
use super::view::{Theme, content_area, notice_line, separator, window_start};
use crate::host_state::WebState;
use cccc_contracts::reply_question::{Question, QuestionOption};
use ratatui::{Frame, layout::Rect, text::Line, widgets::Paragraph};

pub(super) fn render(frame: &mut Frame, app: &App, theme: Theme) {
    let area = content_area(frame.area());
    let width = usize::from(area.width);
    let view = &app.pending_view;
    let title = format!(" Waiting for you - {}", text::one_line(&view.group_name));
    frame.render_widget(
        Paragraph::new(Line::styled(text::fit(&title, width - 1), theme.bold())),
        row(area, 0, 1),
    );
    let id = format!(" Group {}", text::one_line(&view.group_id));
    frame.render_widget(
        Paragraph::new(Line::styled(text::fit(&id, width - 1), theme.dim())),
        row(area, 1, 1),
    );
    if let Some(request) = &view.detail {
        if view.editing {
            editor(frame, app, theme, row(area, 2, area.height - 5));
        } else {
            detail(frame, app, request, theme, row(area, 2, area.height - 5));
        }
    } else {
        let body = row(area, 2, area.height - 5);
        if let Some(page) = app
            .selected_pending()
            .and_then(|snapshot| snapshot.page.as_ref())
        {
            let mut lines = vec![Line::styled(
                format!(" {} open  |  page {}", page.total, view.cursors.len()),
                theme.dim(),
            )];
            if page.rows.is_empty() {
                lines.push(Line::raw(" No requests on this page."));
            } else {
                let selected = page
                    .rows
                    .iter()
                    .position(|item| Some(&item.event.id) == view.selected.as_ref());
                let room = usize::from(body.height).saturating_sub(1);
                let start = window_start(page.rows.len(), room, selected);
                for (index, item) in page.rows.iter().enumerate().skip(start).take(room) {
                    let prompt = item
                        .question
                        .as_ref()
                        .map_or_else(|| item.text(), |question| question.prompt.as_str());
                    let label = format!(
                        "{}: {}",
                        text::one_line(&item.event.by),
                        text::one_line(prompt)
                    );
                    lines.push(super::groups_view::choice_row(
                        &label,
                        selected == Some(index),
                        theme,
                        width,
                    ));
                }
            }
            frame.render_widget(Paragraph::new(lines), body);
        } else {
            let text = if app
                .selected_pending()
                .is_some_and(|snapshot| snapshot.error.is_some())
            {
                " Requests unavailable. Retrying..."
            } else {
                " Reading requests..."
            };
            frame.render_widget(Paragraph::new(Line::styled(text, theme.dim())), body);
        }
    }
    let draft_error = app
        .current_reply_draft()
        .and_then(|draft| draft.error.as_ref())
        .map(|error| {
            let message = match error.as_str() {
                "requester_unavailable" | "requester_unknown" => {
                    "Original requester cannot be verified."
                }
                "unsupported_request_source" => "Reply to this request in the Web conversation.",
                "invalid_source_event" => "Original request not available; check in Web.",
                "question_unavailable" => "Question changed; enter an answer in your own words.",
                "invalid_answer" => "Answer was rejected. Check the selection or text.",
                _ => error.as_str(),
            };
            Line::styled(
                text::fit(&format!(" {}", text::one_line(message)), width - 1),
                theme.error(),
            )
        });
    let footer = notice_line(app, theme, width)
        .or(draft_error)
        .unwrap_or_else(|| {
            let value = match &app.host.web {
                WebState::Listening { url } => {
                    format!(" Web {url}/?group={}", text::one_line(&view.group_id))
                }
                _ => " Answers are recorded; delivery may be delayed.".to_owned(),
            };
            Line::styled(text::fit(&value, width - 1), theme.dim())
        });
    frame.render_widget(Paragraph::new(footer), row(area, area.height - 3, 1));
    frame.render_widget(
        Paragraph::new(separator(theme, width)),
        row(area, area.height - 2, 1),
    );
    let keys = if view.editing {
        " Left/Right edit   Ctrl+J newline   Enter done   Esc back"
    } else if let Some(request) = &view.detail {
        let draft = app.current_reply_draft();
        let status = draft
            .and_then(|draft| draft.closed)
            .unwrap_or(request.status);
        if status == Status::Pending || draft.is_some_and(|draft| draft.uncertain || draft.sending)
        {
            " Up/Down choose   PgUp/PgDn scroll   Enter   Esc back"
        } else {
            " Up/Down scroll   PgUp/PgDn   Esc back"
        }
    } else {
        " Up/Down choose   Enter open   PgUp/PgDn pages   Esc back"
    };
    frame.render_widget(
        Paragraph::new(Line::styled(keys, theme.dim())),
        row(area, area.height - 1, 1),
    );
}

fn detail(frame: &mut Frame, app: &App, request: &super::pending::Row, theme: Theme, area: Rect) {
    let width = usize::from(area.width);
    let draft = app.current_reply_draft();
    let status = draft
        .and_then(|draft| draft.closed)
        .unwrap_or(request.status);
    let may_answer =
        status == Status::Pending || draft.is_some_and(|draft| draft.uncertain || draft.sending);
    let options = request
        .question
        .as_ref()
        .map_or(&[][..], |question| question.options.as_slice());
    let action_count = if may_answer { options.len() + 2 } else { 1 };
    let body_height = area
        .height
        .saturating_sub(u16::try_from(action_count).unwrap_or(u16::MAX));
    let mut lines = vec![Line::styled(
        format!(" From {}", text::one_line(&request.event.by)),
        theme.bold(),
    )];
    let body = match (&request.question, may_answer) {
        (Some(question), true) => question_body(question, width, false),
        (Some(question), false) if matches!(status, Status::Answered | Status::Withdrawn) => {
            question_body(question, width, true)
        }
        _ => request.text().to_owned(),
    };
    for line in body.split('\n') {
        let clean = text::one_line(line);
        let wrapped = text::wrap_exact(&clean, width.saturating_sub(2));
        if wrapped.is_empty() {
            lines.push(Line::raw(" "));
        } else {
            lines.extend(
                wrapped
                    .into_iter()
                    .map(|part| Line::raw(format!(" {part}"))),
            );
        }
    }
    if status != Status::Pending
        && let Some(draft) = draft.filter(|draft| {
            !draft.confirmed
                && !draft.sending
                && !draft.uncertain
                && (draft.selection.is_some() || !draft.text.is_empty())
        })
    {
        lines.push(Line::styled(" Saved draft:", theme.bold()));
        if let Some(ReplySelection::Option(id)) = &draft.selection {
            let label = request
                .question
                .as_ref()
                .and_then(|question| question.option(id))
                .map(|option| format!("{}. {}", id, option.label))
                .unwrap_or_else(|| id.clone());
            lines.extend(
                text::wrap_exact(&text::one_line(&label), width.saturating_sub(2))
                    .into_iter()
                    .map(|part| Line::raw(format!(" {part}"))),
            );
        }
        for line in draft.text.split('\n') {
            let clean = text::one_line(line);
            let wrapped = text::wrap_exact(&clean, width.saturating_sub(2));
            if wrapped.is_empty() {
                lines.push(Line::raw(" "));
            } else {
                lines.extend(
                    wrapped
                        .into_iter()
                        .map(|part| Line::raw(format!(" {part}"))),
                );
            }
        }
    }
    if let Some(error) = draft.and_then(|draft| draft.error.as_ref()) {
        lines.extend(
            text::wrap(&text::one_line(error), width.saturating_sub(2))
                .into_iter()
                .map(|line| Line::styled(format!(" {line}"), theme.error())),
        );
    }
    let (top, _) = app.pending_view.scroll.fit(lines.len(), body_height);
    frame.render_widget(
        Paragraph::new(lines).scroll((top, 0)),
        row(area, 0, body_height),
    );
    let mut choices = Vec::new();
    if may_answer {
        for (index, option) in options.iter().enumerate() {
            let selected = draft.and_then(|draft| draft.selection.as_ref())
                == Some(&ReplySelection::Option(option.id.clone()));
            let recommended = request
                .question
                .as_ref()
                .and_then(|question| question.recommended.as_ref())
                .is_some_and(|recommended| recommended.option_id == option.id);
            let label = option_label(option, selected, recommended);
            choices.push(super::groups_view::choice_row(
                &label,
                app.pending_view.cursor == index,
                theme,
                width,
            ));
        }
        let selected =
            draft.and_then(|draft| draft.selection.as_ref()) == Some(&ReplySelection::Other);
        choices.push(super::groups_view::choice_row(
            &format!("[{}] Other (type answer)", if selected { "x" } else { " " }),
            app.pending_view.cursor == options.len(),
            theme,
            width,
        ));
        let send = if draft.is_some_and(|draft| draft.sending) {
            "Sending..."
        } else if draft.is_some_and(|draft| draft.uncertain) {
            "Retry the same answer"
        } else {
            "Send answer"
        };
        choices.push(super::groups_view::choice_row(
            send,
            app.pending_view.cursor == options.len() + 1,
            theme,
            width,
        ));
    } else {
        let label = match status {
            Status::Answered => {
                let option = draft
                    .filter(|draft| draft.confirmed)
                    .and_then(|draft| draft.selection.as_ref())
                    .and_then(|selection| match selection {
                        ReplySelection::Option(id) => request
                            .question
                            .as_ref()
                            .and_then(|question| question.option(id)),
                        ReplySelection::Other => None,
                    });
                option.map_or_else(
                    || " Answer recorded. Delivery may be delayed.".to_owned(),
                    |option| {
                        format!(
                            " Answer recorded: {}. {}",
                            option.id,
                            text::one_line(&option.label)
                        )
                    },
                )
            }
            Status::Withdrawn => " Request withdrawn.".to_owned(),
            Status::Unavailable => " Original requester unavailable; check in Web.".to_owned(),
            Status::RequesterUnknown => {
                " Cannot verify the original requester; check in Web.".to_owned()
            }
            Status::Unsupported => {
                " Cross-group request: reply in the Web conversation.".to_owned()
            }
            Status::Unknown => " Request status unknown; retrying read.".to_owned(),
            Status::Pending => unreachable!(),
        };
        choices.push(Line::styled(text::fit(&label, width - 1), theme.dim()));
    }
    frame.render_widget(
        Paragraph::new(choices),
        row(
            area,
            body_height,
            u16::try_from(action_count).unwrap_or(u16::MAX),
        ),
    );
}

fn option_label(option: &QuestionOption, selected: bool, recommended: bool) -> String {
    format!(
        "[{}] {}. {}{}",
        if selected { "x" } else { " " },
        option.id,
        if recommended { "(recommended) " } else { "" },
        text::one_line(&option.label)
    )
}

// The options remain visible in the fixed choice rows. Put their consequences
// in the scrollable body, and repeat a label only when its row truncates it.
fn question_body(question: &Question, width: usize, include_labels: bool) -> String {
    let mut lines = Vec::new();
    if !question.context.is_empty() {
        lines.push(question.context.clone());
        lines.push(String::new());
    }
    lines.push(question.prompt.clone());
    for option in &question.options {
        let recommended = question
            .recommended
            .as_ref()
            .is_some_and(|value| value.option_id == option.id);
        let truncated = include_labels
            || text::width(&option_label(option, false, recommended)) > width.saturating_sub(4);
        if truncated || !option.consequence.is_empty() {
            let description = if truncated {
                let mut description = format!("{}. {}", option.id, option.label);
                if !option.consequence.is_empty() {
                    description.push_str(" — ");
                    description.push_str(&option.consequence);
                }
                description
            } else {
                format!("{}: {}", option.id, option.consequence)
            };
            lines.push(description);
        }
        if let Some(value) = question
            .recommended
            .as_ref()
            .filter(|value| value.option_id == option.id)
        {
            lines.push(format!("Recommended {} — {}", option.id, value.reason));
        }
    }
    lines.join("\n")
}

fn editor(frame: &mut Frame, app: &App, theme: Theme, area: Rect) {
    let width = usize::from(area.width);
    let Some(draft) = app.current_reply_draft() else {
        return;
    };
    let mut lines = vec![Line::styled(" Your answer (| is the cursor)", theme.bold())];
    let prefix = &draft.text[..draft.input_cursor];
    let cursor_line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let cursor_offset = prefix.rsplit('\n').next().unwrap_or("").len();
    let mut cursor_row = 1;
    for (index, line) in draft.text.split('\n').enumerate() {
        let mut clean = text::one_line(line);
        let cursor_byte = (index == cursor_line).then(|| {
            let position = text::one_line(&line[..cursor_offset]).len();
            clean.insert(position, '|');
            position
        });
        let wrapped = text::wrap_exact(&clean, width.saturating_sub(2));
        if wrapped.is_empty() {
            lines.push(Line::raw(" "));
        } else {
            let mut offset = 0;
            for part in wrapped {
                if cursor_byte
                    .is_some_and(|position| position >= offset && position < offset + part.len())
                {
                    cursor_row = lines.len();
                }
                offset += part.len();
                lines.push(Line::raw(format!(" {part}")));
            }
        }
    }
    let height = usize::from(area.height);
    // Keep the cursor visible even when editing the beginning of a long draft.
    let start = cursor_row
        .saturating_add(1)
        .saturating_sub(height)
        .min(lines.len().saturating_sub(height));
    frame.render_widget(
        Paragraph::new(lines).scroll((u16::try_from(start).unwrap_or(u16::MAX), 0)),
        area,
    );
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
mod tests {
    use super::question_body;
    use cccc_contracts::reply_question::Question;
    use serde_json::json;

    #[test]
    fn question_body_keeps_reasons_and_only_repeats_truncated_labels() {
        let long = "Long ".repeat(16);
        let question = Question::from_request(
            &json!({"v":1, "prompt":"Which?", "options":[
                {"label":"Short", "consequence":"Short consequence"},
                {"label":long, "consequence":"Long consequence"}
            ], "recommended":{"option_id":"B", "reason":"Reason"}}),
            "Context",
        )
        .expect("question");
        let body = question_body(&question, 60, false);
        assert!(body.contains("Context\n\nWhich?"));
        assert!(body.contains("A: Short consequence"));
        assert!(!body.contains("A. Short"));
        assert!(body.contains(&format!(
            "B. {} — Long consequence",
            question.options[1].label
        )));
        assert!(body.contains("Recommended B — Reason"));
        assert!(!body.contains("Reply with"));
        assert!(question_body(&question, 60, true).contains("A. Short — Short consequence"));
    }
}
