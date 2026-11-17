use crate::host_state::UpdateResult;
use crate::tui::app::{App, Effect, Notice, Screen, UpdateOffer};
use crate::tui::test_support::{context, running};
use crate::tui::update::{UpdateCommand, UpdateProgress};
use crate::tui::view::render;
use cccc_web::UpdateCheck;
use crossterm::event::{Event, KeyCode, KeyEvent};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::time::Instant;

fn press(app: &mut App, code: KeyCode) -> Option<Effect> {
    app.handle(Event::Key(KeyEvent::from(code)))
}

fn rows(app: &App) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
    terminal
        .draw(|frame| render(frame, app, Instant::now()))
        .expect("draw");
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect()
}

fn shows(app: &App, text: &str) -> bool {
    rows(app).iter().any(|row| row.contains(text))
}

fn newer() -> UpdateCheck {
    UpdateCheck {
        current: "0.4.45".into(),
        channel: "stable".into(),
        installation: "standalone".into(),
        latest: Some("0.4.46".into()),
        status: Some("newer".into()),
        failure: None,
        release_notes_url: Some("https://github.com/ChesterRa/cccc/releases/tag/v0.4.46".into()),
        command: Some("cccc update --channel stable".into()),
    }
}

/// `/update` opened on a host with `offer`, its check answered with `check`.
fn checked(offer: UpdateOffer, check: Option<UpdateCheck>) -> App {
    let mut context = context();
    context.update = offer;
    let mut app = App::new(context, running());
    app.input = "/update".into();
    let Some(Effect::Update(UpdateCommand::Check { generation })) = press(&mut app, KeyCode::Enter)
    else {
        panic!("opening /update checks");
    };
    app.update = UpdateProgress {
        generation,
        checking: check.is_none(),
        check,
        ..UpdateProgress::default()
    };
    app
}

#[test]
fn opening_checks_once_and_a_failed_check_never_reads_as_current() {
    let app = checked(UpdateOffer::InPlace, None);
    assert_eq!(app.screen, Screen::Update);
    assert!(shows(&app, "Installed    0.4.45"));
    assert!(shows(&app, "Checking..."));

    let failed = UpdateCheck {
        latest: None,
        status: None,
        failure: Some("release index timed out".into()),
        release_notes_url: None,
        command: None,
        ..newer()
    };
    let mut app = checked(UpdateOffer::InPlace, Some(failed));
    assert!(shows(&app, "Check failed"));
    assert!(shows(&app, "release index timed out"));
    assert!(!shows(&app, "up to date"));
    assert!(!shows(&app, "Update and restart agents"));
    // Checking again is a new visit, so a late answer to the first is ignored.
    let Some(Effect::Update(UpdateCommand::Check { generation })) =
        press(&mut app, KeyCode::Char('r'))
    else {
        panic!("r checks again");
    };
    assert_eq!(app.update_progress(), None);
    assert!(generation > app.update.generation);
    assert_eq!(
        press(&mut app, KeyCode::Esc),
        Some(Effect::Update(UpdateCommand::Cancel))
    );
    assert_eq!(app.screen, Screen::Main);
}

#[test]
fn a_newer_release_is_confirmed_once_and_cancel_is_the_default() {
    let mut app = checked(UpdateOffer::InPlace, Some(newer()));
    assert!(shows(&app, "Latest       0.4.46"));
    assert!(shows(
        &app,
        "https://github.com/ChesterRa/cccc/releases/tag/v0.4.46"
    ));
    assert!(shows(&app, "running agents restart"));
    assert!(shows(&app, "> Cancel"));
    assert!(app.confirming());

    // Enter on the default does nothing but leave.
    assert_eq!(press(&mut app, KeyCode::Enter), None);
    assert_eq!(app.screen, Screen::Main);

    let mut app = checked(UpdateOffer::InPlace, Some(newer()));
    let generation = app.update.generation;
    assert_eq!(press(&mut app, KeyCode::Down), None);
    assert!(shows(&app, "> Update and restart agents"));
    assert_eq!(
        press(&mut app, KeyCode::Enter),
        Some(Effect::Update(UpdateCommand::Start {
            generation,
            version: "0.4.46".into(),
        }))
    );
    // Sent once: a second Enter before the worker answers starts nothing.
    assert_eq!(press(&mut app, KeyCode::Enter), None);

    // While the release is prepared, Esc gives it up and nothing else acts.
    app.update.preparing = Some("0.4.46".into());
    assert!(shows(&app, "Preparing CCCC 0.4.46"));
    assert!(shows(&app, "Esc give up"));
    assert_eq!(press(&mut app, KeyCode::Char('r')), None);
    assert_eq!(
        press(&mut app, KeyCode::Esc),
        Some(Effect::Update(UpdateCommand::Cancel))
    );

    app.update.preparing = None;
    app.update.rejected = Some("another update holds this CCCC".into());
    assert!(shows(
        &app,
        "The update did not start: another update holds"
    ));
    assert!(!shows(&app, "Update and restart agents"));
}

#[test]
fn a_host_that_cannot_replace_itself_names_the_command() {
    let pip = UpdateCheck {
        installation: "pip".into(),
        command: Some("python -m pip install --upgrade \"cccc-pair==0.4.46\"".into()),
        ..newer()
    };
    let mut app = checked(UpdateOffer::CheckOnly, Some(pip));
    assert!(shows(&app, "installed with pip"));
    assert!(shows(
        &app,
        "python -m pip install --upgrade \"cccc-pair==0.4.46\""
    ));
    assert!(!shows(&app, "Update and restart agents"));
    assert!(!app.confirming());
    assert_eq!(press(&mut app, KeyCode::Enter), None);
    assert_eq!(app.screen, Screen::Update);

    let current = UpdateCheck {
        latest: Some("0.4.45".into()),
        status: Some("current".into()),
        command: None,
        ..newer()
    };
    let app = checked(UpdateOffer::InPlace, Some(current));
    assert!(shows(&app, "CCCC is up to date."));
    assert!(!shows(&app, "Update and restart agents"));

    let mut context = context();
    context.update = UpdateOffer::Unavailable;
    let mut app = App::new(context, running());
    app.input = "/update".into();
    assert_eq!(press(&mut app, KeyCode::Enter), None);
    assert_eq!(app.screen, Screen::Main);
    assert!(
        matches!(&app.notice, Some(Notice::Error(message)) if message.contains("cccc update --check"))
    );
}

#[test]
fn an_update_result_is_shown_once_when_the_host_learns_it() {
    let mut app = App::new(context(), running());
    assert_eq!(app.notice, None);
    let mut host = running();
    host.update = Some(UpdateResult {
        transaction: "t1".into(),
        failed: false,
        message: "Updated CCCC 0.4.45 to 0.4.46.".into(),
    });
    app.set_host(host.clone());
    assert_eq!(
        app.notice,
        Some(Notice::Info("Updated CCCC 0.4.45 to 0.4.46.".into()))
    );
    // Dismissed, it stays dismissed while the host repeats the same result.
    app.notice = None;
    app.set_host(host);
    assert_eq!(app.notice, None);

    // A result known at start, such as a given-up update, opens with it.
    let mut host = running();
    host.update = Some(UpdateResult {
        transaction: "t2".into(),
        failed: true,
        message: "CCCC was not updated to 0.4.46 (stop: fixture); CCCC 0.4.45 started again."
            .into(),
    });
    let app = App::new(context(), host);
    assert!(
        matches!(&app.notice, Some(Notice::Error(message)) if message.contains("started again"))
    );
}

#[test]
fn a_confirmed_update_can_only_be_given_up_until_it_is_answered() {
    let mut app = checked(UpdateOffer::InPlace, Some(newer()));
    let generation = app.update.generation;
    press(&mut app, KeyCode::Down);
    assert!(matches!(
        press(&mut app, KeyCode::Enter),
        Some(Effect::Update(UpdateCommand::Start { .. }))
    ));
    // The worker has not taken the start up yet: checking again would start a
    // new visit while the update is prepared, so it does nothing.
    assert_eq!(press(&mut app, KeyCode::Char('r')), None);
    assert_eq!(app.update.generation, generation);
    assert_eq!(
        press(&mut app, KeyCode::Esc),
        Some(Effect::Update(UpdateCommand::Cancel))
    );
    assert_eq!(app.screen, Screen::Update);

    // Once the update is answered, checking again works.
    app.update.given_up = true;
    assert!(matches!(
        press(&mut app, KeyCode::Char('r')),
        Some(Effect::Update(UpdateCommand::Check { .. }))
    ));
}

#[test]
fn a_handover_the_host_could_not_carry_out_returns_with_its_result() {
    let mut app = checked(UpdateOffer::InPlace, Some(newer()));
    let mut host = running();
    // The same failure twice: each is a new update, so each wait ends.
    for transaction in ["t1", "t2"] {
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        app.update.handed_over = true;
        assert!(shows(&app, "Updating..."));
        assert_eq!(press(&mut app, KeyCode::Esc), None);

        // The host could not arm the update, so it serves again and says why.
        host.update = Some(UpdateResult {
            transaction: transaction.into(),
            failed: true,
            message: "The update to CCCC 0.4.46 did not start (fixture). CCCC was not updated and no agent was stopped.".into(),
        });
        app.set_host(host.clone());
        assert_eq!(app.screen, Screen::Main, "{transaction}");
        assert!(
            matches!(&app.notice, Some(Notice::Error(message)) if message.contains("did not start"))
        );

        // A later /update is a new visit with a newer release to confirm.
        app.input = "/update".into();
        let Some(Effect::Update(UpdateCommand::Check { generation })) =
            press(&mut app, KeyCode::Enter)
        else {
            panic!("a later /update checks again");
        };
        app.update = UpdateProgress {
            generation,
            check: Some(newer()),
            ..UpdateProgress::default()
        };
    }
}
