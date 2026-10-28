use super::interrupted_receipt;
use crate::commands::sessions::setup::SetupInspection;
use crate::tui::app::{App, Screen, SetupStep};
use crate::tui::setup::{InstallState, RUNTIMES, SetupProgress};
use crate::tui::test_support::{checks, context, found, missing, running, source};
use crate::tui::view::render;
use cccc_contracts::local_session::LocalSessionRuntime;
use crossterm::event::{Event, KeyCode, KeyEvent};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::time::Instant;

fn press(app: &mut App, code: KeyCode) {
    app.handle(Event::Key(KeyEvent::from(code)));
}

/// An app on the first setup step with the checks in.
fn picking() -> App {
    let mut app = App::new(context(), running());
    app.sessions.record(Ok(Vec::new()), Instant::now());
    app.input = "/setup".into();
    press(&mut app, KeyCode::Enter);
    app.setup = SetupProgress {
        generation: 1,
        checking: false,
        checks: checks(),
        installs: Vec::new(),
    };
    app
}

fn rows(app: &App, width: u16, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
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

#[test]
fn checking_shows_only_the_way_back() {
    let mut app = App::new(context(), running());
    app.input = "/setup".into();
    press(&mut app, KeyCode::Enter);
    let rows = rows(&app, 80, 16);
    assert!(rows[0].starts_with(" Connect native sessions"));
    assert!(rows[0].ends_with("1/3"));
    assert_eq!(rows[3], " Checking Codex, Claude Code, Pi and OpenCode...");
    assert_eq!(rows[15], " b back");
}

#[test]
fn every_app_is_listed_unpicked_with_what_the_check_found() {
    let app = picking();
    let rows = rows(&app, 80, 16);
    let sep = std::path::MAIN_SEPARATOR;
    assert_eq!(
        rows[3],
        format!(" > [ ] Codex        0.164.0     ~{sep}.codex")
    );
    assert_eq!(rows[4], "         needs setup or update");
    assert_eq!(
        rows[5],
        format!("   [ ] Claude Code  2.1.311     ~{sep}.claude")
    );
    assert_eq!(
        rows[7],
        format!("   [ ] Pi           1.4.0       ~{sep}.pi{sep}agent")
    );
    assert_eq!(rows[8], "         set up by this CCCC");
    assert_eq!(rows[9], "   [-] OpenCode     not found on PATH");
    assert_eq!(rows[10], "");
    assert_eq!(
        rows[15],
        " Up/Down move   Space pick   Enter continue   b back"
    );
}

#[test]
fn picking_pi_warns_that_its_open_sessions_disconnect() {
    let mut app = picking();
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Char(' '));
    let rows = rows(&app, 80, 16);
    assert!(rows[7].starts_with(" > [x] Pi"));
    assert_eq!(
        rows[9],
        "         Open Pi sessions disconnect until you reload them."
    );
}

#[test]
fn confirming_names_the_directories_and_the_way_back_out() {
    let mut app = picking();
    press(&mut app, KeyCode::Char(' '));
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Char(' '));
    press(&mut app, KeyCode::Enter);
    assert_eq!(
        app.flow.as_ref().map(|flow| flow.step),
        Some(SetupStep::Confirm)
    );
    let rows = rows(&app, 80, 16);
    let sep = std::path::MAIN_SEPARATOR;
    assert!(rows[0].ends_with("2/3"));
    assert!(rows.contains(&format!("   Codex        ~{sep}.codex")));
    assert!(rows.contains(&format!("   Pi           ~{sep}.pi{sep}agent")));
    assert!(rows.contains(
        &"                Open Pi sessions disconnect until you reload them.".to_owned()
    ));
    assert!(rows.contains(&" CCCC home  ~/.cccc".to_owned()));
    assert!(
        rows.iter()
            .any(|row| row.contains("cccc sessions disable --runtime <app>"))
    );
    assert_eq!(rows[15], " Enter connect   b back");
}

#[test]
fn the_last_step_reports_each_outcome_and_new_sessions_only() {
    let mut flow_app = App::new(context(), running());
    flow_app.sessions.record(
        Ok(vec![source(
            LocalSessionRuntime::Codex,
            "/srv/old",
            "",
            true,
        )]),
        Instant::now(),
    );
    flow_app.input = "/setup".into();
    press(&mut flow_app, KeyCode::Enter);
    flow_app.setup = SetupProgress {
        generation: 1,
        checking: false,
        checks: checks(),
        installs: Vec::new(),
    };
    press(&mut flow_app, KeyCode::Char(' '));
    press(&mut flow_app, KeyCode::Down);
    press(&mut flow_app, KeyCode::Char(' '));
    press(&mut flow_app, KeyCode::Enter);
    press(&mut flow_app, KeyCode::Enter);
    flow_app.setup.installs = vec![
        (
            LocalSessionRuntime::Codex,
            InstallState::Written {
                provider_dir: "/home/me/.codex".into(),
                warnings: Vec::new(),
            },
        ),
        (
            LocalSessionRuntime::Claude,
            InstallState::Failed("Claude Code 2.1.0 is older than 2.1.306".into()),
        ),
    ];
    let mut list = flow_app.sessions.clone();
    list.record(
        Ok(vec![
            source(LocalSessionRuntime::Codex, "/srv/old", "", true),
            source(LocalSessionRuntime::Codex, "/srv/new", "", false),
        ]),
        Instant::now(),
    );
    flow_app.set_sessions(list);
    let rows = rows(&flow_app, 80, 40);
    assert!(rows[0].starts_with(" Finish in each app"));
    let sep = std::path::MAIN_SEPARATOR;
    assert_eq!(
        rows[1],
        format!("   Codex        configuration written to ~{sep}.codex")
    );
    assert_eq!(
        rows[2],
        "                Start or restart Codex, approve the CCCC hooks when asked"
    );
    assert!(
        rows.iter()
            .any(|row| row.contains("/daemon > Restart background"))
    );
    assert!(rows.iter().any(|row| row == "   Claude Code  failed"));
    assert!(
        rows.iter()
            .any(|row| row.contains("Claude Code 2.1.0 is older than 2.1.306"))
    );
    assert!(
        rows.iter()
            .any(|row| row == " New sessions since setup started")
    );
    assert!(rows.iter().any(|row| row.contains("/srv/new")));
    assert!(!rows.iter().any(|row| row.contains("/srv/old")));
    assert!(
        rows.iter()
            .any(|row| row.starts_with(" Try it: in one session"))
    );
    assert_eq!(rows[39], " Enter back to status");
}

#[test]
fn a_running_install_offers_only_cancel() {
    let mut app = picking();
    press(&mut app, KeyCode::Char(' '));
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Enter);
    let rows_before = rows(&app, 80, 16);
    assert_eq!(rows_before[1], "   Starting...");
    assert_eq!(rows_before[15], " b cancel");
    app.setup.installs = vec![(LocalSessionRuntime::Codex, InstallState::Installing)];
    let rows = rows(&app, 80, 16);
    assert_eq!(rows[1], "   Codex        installing...");
    assert!(!rows.iter().any(|row| row.contains("Try it")));
    assert_eq!(app.screen, Screen::Setup);
}

#[test]
fn an_interrupted_install_is_reported_after_the_screen_closes() {
    let mut app = picking();
    assert_eq!(interrupted_receipt(&app), None);
    app.setup.installs = vec![
        (
            LocalSessionRuntime::Pi,
            InstallState::Written {
                provider_dir: "/home/me/.pi/agent".into(),
                warnings: Vec::new(),
            },
        ),
        (LocalSessionRuntime::Opencode, InstallState::Interrupted),
        (LocalSessionRuntime::Claude, InstallState::Skipped),
    ];
    assert_eq!(
        interrupted_receipt(&app).as_deref(),
        Some(
            "Setup was interrupted:\n  OpenCode     configuration may be partly written; retry with `cccc sessions enable --runtime opencode` and resolve any conflict it reports"
        )
    );
}

/// All four apps connected on a small screen, with warnings and new sessions
/// that make the last step taller than the terminal.
fn four_written() -> App {
    let mut app = App::new(context(), running());
    app.sessions.record(Ok(Vec::new()), Instant::now());
    app.input = "/setup".into();
    press(&mut app, KeyCode::Enter);
    let mut all_found = checks();
    all_found[3] = found(
        LocalSessionRuntime::Opencode,
        "2.1.4",
        "/home/me/.config/opencode",
    );
    app.setup = SetupProgress {
        generation: 1,
        checking: false,
        checks: all_found,
        installs: Vec::new(),
    };
    for _ in 0..4 {
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Down);
    }
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Enter);
    app.setup.installs = [
        (LocalSessionRuntime::Codex, "/home/me/.codex"),
        (LocalSessionRuntime::Claude, "/home/me/.claude"),
        (LocalSessionRuntime::Pi, "/home/me/.pi/agent"),
        (LocalSessionRuntime::Opencode, "/home/me/.config/opencode"),
    ]
    .into_iter()
    .map(|(runtime, directory)| {
        (
            runtime,
            InstallState::Written {
                provider_dir: directory.into(),
                warnings: vec![format!(
                    "{} kept an older MCP entry that this CCCC does not own; review it before relying on it.",
                    runtime.as_str()
                )],
            },
        )
    })
    .collect();
    let mut list = app.sessions.clone();
    list.record(
        Ok(vec![
            source(LocalSessionRuntime::Claude, "/srv/app-a", "", true),
            source(LocalSessionRuntime::Pi, "/srv/app-b", "", true),
        ]),
        Instant::now(),
    );
    app.set_sessions(list);
    app
}

#[test]
fn a_tall_last_step_scrolls_to_every_app_and_the_prompt() {
    let mut app = four_written();
    let top = rows(&app, 60, 16);
    assert!(top[0].starts_with(" Finish in each app"));
    assert!(top[1].starts_with("   Codex        configuration written to"));
    assert_eq!(top[15], " Up/Down scroll   Enter back to status");
    assert!(!top.iter().any(|row| row.contains("Try it")));

    press(&mut app, KeyCode::End);
    let bottom = rows(&app, 60, 16);
    assert!(
        bottom
            .iter()
            .any(|row| row.starts_with("   OpenCode     configuration written"))
    );
    assert!(
        bottom
            .iter()
            .any(|row| row.contains("Reopen the OpenCode session"))
    );
    assert!(bottom.iter().any(|row| row.contains("/srv/app-b")));
    assert_eq!(bottom[13], " should ask you which one.", "{bottom:#?}");
    assert_eq!(bottom[15], " Up/Down scroll   Enter back to status");

    press(&mut app, KeyCode::PageUp);
    press(&mut app, KeyCode::Home);
    assert_eq!(rows(&app, 60, 16)[1], top[1]);
    // Scrolling never moves past the last line.
    for _ in 0..50 {
        press(&mut app, KeyCode::Down);
    }
    assert_eq!(rows(&app, 60, 16), bottom);
    assert_eq!(app.screen, Screen::Setup);
}

#[test]
fn a_screen_that_fits_does_not_offer_scrolling() {
    let mut app = four_written();
    let rows = rows(&app, 100, 60);
    assert_eq!(rows[59], " Enter back to status");
    press(&mut app, KeyCode::Down);
    assert_eq!(app.flow.as_ref().map(|flow| flow.scroll.top), Some(0));
}

#[test]
fn an_app_whose_directory_is_unknown_cannot_be_picked() {
    let mut app = picking();
    app.setup.checks[3] = SetupInspection {
        provider_dir: None,
        issue: Some("Could not locate OpenCode's native configuration directory".into()),
        ..found(LocalSessionRuntime::Opencode, "2.1.4", "")
    };
    for _ in 0..3 {
        press(&mut app, KeyCode::Down);
    }
    press(&mut app, KeyCode::Char(' '));
    assert!(app.picked().is_empty());
    let rows = rows(&app, 80, 16);
    assert_eq!(rows[9], " > [-] OpenCode     2.1.4");
    assert_eq!(
        rows[10],
        "         Could not locate OpenCode's native configuration directory"
    );
}

#[test]
fn picking_nothing_explains_itself_above_the_keys() {
    let mut app = picking();
    press(&mut app, KeyCode::Enter);
    let rows = rows(&app, 80, 16);
    assert_eq!(rows[13], " Pick at least one app with Space.");
    assert_eq!(rows[14], format!(" {}", "-".repeat(78)));
    press(&mut app, KeyCode::Char(' '));
    assert_eq!(app.notice, None);
}

#[test]
fn no_app_on_path_says_what_to_do() {
    let mut app = picking();
    app.setup.checks = RUNTIMES.iter().map(|runtime| missing(*runtime)).collect();
    let rows = rows(&app, 80, 16);
    assert_eq!(rows[6], "   [-] OpenCode     not found on PATH");
    assert_eq!(
        rows[8],
        " None of these apps is on PATH. Install one, then run /setup again."
    );
}
