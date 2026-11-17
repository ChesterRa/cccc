use crate::host_state::UpdateResult;
use crate::tui::app::{App, Effect, Notice, Screen, UpdateOffer};
use crate::tui::test_support::{context, running};
use crate::tui::update::{UpdateCommand, UpdateProgress};
use crate::tui::view::render;
use cccc_web::{
    KeepPreview, KeepRefusal, LocalBuild, UpdateCheck, UpdateMode, UpdateRequest, UpdateSource,
};
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

/// Whether the screen reads `text`, even where it wraps across rows.
fn reads(app: &App, text: &str) -> bool {
    let rows = rows(app);
    let lines = rows.iter().map(|row| row.trim()).collect::<Vec<_>>();
    lines.join(" ").contains(text)
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
        local: None,
    }
}

/// `/update` opened on a host with `offer`, its check answered with `check`.
fn checked(offer: UpdateOffer, check: Option<UpdateCheck>) -> App {
    checked_on(offer, false, check)
}

/// The same, on a host that also restarts into a rebuilt program.
fn checked_local(offer: UpdateOffer, check: Option<UpdateCheck>) -> App {
    checked_on(offer, true, check)
}

fn checked_on(offer: UpdateOffer, local_restart: bool, check: Option<UpdateCheck>) -> App {
    let mut context = context();
    context.update = offer;
    context.local_restart = local_restart;
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
fn a_host_that_keeps_agents_offers_it_for_a_release_checked_once_downloaded() {
    let mut context = context();
    context.release_keep = true;
    let mut app = App::new(context, running());
    app.input = "/update".into();
    let Some(Effect::Update(UpdateCommand::Check { generation })) = press(&mut app, KeyCode::Enter)
    else {
        panic!("opening /update checks");
    };
    app.update = UpdateProgress {
        generation,
        check: Some(newer()),
        ..UpdateProgress::default()
    };
    assert!(shows(
        &app,
        "Keeping agents downloads and checks the release first"
    ));
    assert!(!shows(&app, "running agents restart"));
    assert_eq!(press(&mut app, KeyCode::Down), None);
    assert!(shows(&app, "> Keep agents; update"));
    assert!(shows(&app, "   Update and restart agents"));
    assert_eq!(
        press(&mut app, KeyCode::Enter),
        Some(Effect::Update(UpdateCommand::Start {
            generation,
            request: UpdateRequest {
                source: UpdateSource::Release {
                    version: "0.4.46".into()
                },
                mode: UpdateMode::Keep,
            },
            version: "0.4.46".into(),
        }))
    );
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
            request: UpdateRequest {
                source: UpdateSource::Release {
                    version: "0.4.46".into()
                },
                mode: UpdateMode::Restart,
            },
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

#[test]
fn a_rebuilt_local_program_shows_whatever_the_release_check_answered() {
    let written = std::time::SystemTime::now() - std::time::Duration::from_secs(180);
    let rebuilt = LocalBuild::rebuilt(
        "0.4.46".into(),
        "a".repeat(64),
        written,
        "fixture-token".into(),
        cccc_web::KeepPreview {
            allowed: false,
            refusals: Vec::new(),
        },
    );
    let app = checked(
        UpdateOffer::InPlace,
        Some(UpdateCheck {
            local: Some(rebuilt.clone()),
            ..newer()
        }),
    );
    assert!(shows(&app, "Local build  0.4.46 · built 3m ago"));
    assert!(shows(&app, "Latest       0.4.46"));

    // A failed release check still shows it.
    let failed = UpdateCheck {
        latest: None,
        status: None,
        failure: Some("release index timed out".into()),
        local: Some(rebuilt),
        ..newer()
    };
    let app = checked(UpdateOffer::InPlace, Some(failed));
    assert!(shows(&app, "Local build  0.4.46"));
    assert!(shows(&app, "Check failed"));
    // A host that cannot restart into it offers nothing for it.
    assert!(!app.confirming());
    assert!(!shows(&app, "Keep agents; restart into the local build"));
    assert!(!shows(&app, "Restart agents; use the local build"));

    // One that could not be read says so, with the reason, and is not current.
    let unreadable = UpdateCheck {
        local: Some(LocalBuild::Unreadable {
            reason: "the program cannot check itself (exit status: 3)".into(),
        }),
        ..newer()
    };
    let app = checked(UpdateOffer::InPlace, Some(unreadable));
    assert!(shows(&app, "Local build  Changed, could not be read"));
    assert!(shows(&app, "exit status: 3"));
}

/// A rebuilt program written three minutes ago, through which agents can or
/// cannot keep running.
fn rebuilt(allowed: bool) -> LocalBuild {
    let refusals = if allowed {
        Vec::new()
    } else {
        vec![KeepRefusal {
            code: "mcp_not_observed".into(),
            group_id: Some("g_fixture".into()),
            actor_id: Some("peer".into()),
            owner_id: None,
            display_name: Some("Peer".into()),
            message: "Its CCCC tools have not connected yet.".into(),
        }]
    };
    LocalBuild::rebuilt(
        "0.4.45".into(),
        "a".repeat(64),
        std::time::SystemTime::now() - std::time::Duration::from_secs(180),
        "fixture-token".into(),
        KeepPreview { allowed, refusals },
    )
}

fn local_start(generation: u64, mode: UpdateMode) -> Option<Effect> {
    Some(Effect::Update(UpdateCommand::Start {
        generation,
        request: UpdateRequest {
            source: UpdateSource::Local {
                token: "fixture-token".into(),
                source_id: "a".repeat(64),
            },
            mode,
        },
        version: "0.4.45".into(),
    }))
}

#[test]
fn a_rebuilt_program_is_restarted_into_once_and_keeping_agents_waits_for_every_one() {
    // The release is current; a host that restarts into a rebuilt program
    // still offers it, whatever installed this CCCC.
    let current = UpdateCheck {
        latest: Some("0.4.45".into()),
        status: Some("current".into()),
        local: Some(rebuilt(false)),
        ..newer()
    };
    let mut app = checked_local(UpdateOffer::CheckOnly, Some(current));
    assert!(shows(&app, "CCCC is up to date."));
    assert!(shows(&app, "from the rebuilt program on disk"));
    assert!(shows(&app, "built 3m ago"));
    assert!(shows(&app, "Agents cannot keep running this time:"));
    assert!(shows(
        &app,
        "- Peer: Its CCCC tools have not connected yet."
    ));
    assert!(shows(&app, "> Cancel"));
    assert!(shows(&app, "   Keep agents; restart into the local build"));
    assert!(app.confirming());

    // Keeping agents cannot be chosen, so Down passes it; the last option
    // stays chosen.
    press(&mut app, KeyCode::Down);
    assert!(shows(&app, "> Restart agents; use the local build"));
    press(&mut app, KeyCode::Down);
    assert!(shows(&app, "> Restart agents; use the local build"));
    let generation = app.update.generation;
    assert_eq!(
        press(&mut app, KeyCode::Enter),
        local_start(generation, UpdateMode::Restart)
    );
    // Sent once.
    assert_eq!(press(&mut app, KeyCode::Enter), None);

    // The host checks the rebuilt program before it stops anything.
    app.update.preparing = Some("0.4.45".into());
    app.update.local = true;
    assert!(shows(
        &app,
        "Checking the rebuilt program. CCCC keeps running meanwhile."
    ));
    assert!(shows(&app, "Esc give up"));
    app.update.preparing = None;
    app.update.rejected = Some("the program changed since it was checked".into());
    assert!(reads(
        &app,
        "The restart did not start: the program changed since it was checked. CCCC did not restart and no agent was stopped."
    ));
    assert!(!shows(&app, "> Cancel"));
    app.update.rejected = None;
    app.update.given_up = true;
    assert!(shows(
        &app,
        "The restart was given up. CCCC did not restart"
    ));
    app.update.given_up = false;
    app.update.handed_over = true;
    assert!(shows(&app, "Restarting..."));
}

#[test]
fn agents_are_kept_when_every_one_can_be_and_a_newer_release_is_offered_beside() {
    let check = UpdateCheck {
        local: Some(rebuilt(true)),
        ..newer()
    };
    let mut app = checked_local(UpdateOffer::InPlace, Some(check));
    assert!(!shows(&app, "Agents cannot keep running"));
    assert!(shows(&app, "installs 0.4.46"));
    // In order: Cancel, then the rebuilt program two ways, then the release.
    press(&mut app, KeyCode::Down);
    assert!(shows(&app, "> Keep agents; restart into the local build"));
    press(&mut app, KeyCode::Down);
    assert!(shows(&app, "> Restart agents; use the local build"));
    press(&mut app, KeyCode::Down);
    assert!(shows(&app, "> Update and restart agents"));
    press(&mut app, KeyCode::Up);
    press(&mut app, KeyCode::Up);
    let generation = app.update.generation;
    assert_eq!(
        press(&mut app, KeyCode::Enter),
        local_start(generation, UpdateMode::Keep)
    );
}
