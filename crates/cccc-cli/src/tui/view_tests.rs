use super::{receipt, render};
use crate::host_state::{DaemonState, HostPhase, ShutdownReport, StopResult, WebState};
use crate::tui::app::{App, Notice, Screen};
use crate::tui::groups::{GroupList, GroupStatus, GroupSummary};
use crate::tui::test_support::{context, running, source};
use cccc_contracts::local_session::{LocalSessionRuntime, LocalSessionSource};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Color;
use std::time::{Duration, Instant};
use unicode_width::UnicodeWidthStr;

fn app() -> App {
    App::new(context(), running())
}

fn with_sessions(app: &mut App, sources: Vec<LocalSessionSource>, now: Instant) {
    app.sessions.record(Ok(sources), now);
}

fn group(group_id: &str, title: &str, status: GroupStatus, running: u64) -> GroupSummary {
    GroupSummary {
        group_id: group_id.into(),
        title: title.into(),
        status,
        state: "active".into(),
        running_actors: running,
    }
}

fn with_groups(app: &mut App, groups: Vec<GroupSummary>, now: Instant) {
    let mut list = GroupList::default();
    list.record(Ok(groups), now);
    app.set_groups(list);
}

fn draw(app: &App, width: u16, height: u16, now: Instant) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
    terminal
        .draw(|frame| render(frame, app, now))
        .expect("draw");
    terminal
}

/// Screen rows as text, with the cell after a wide character dropped so a row
/// reads the way it looks.
fn screen_rows(terminal: &Terminal<TestBackend>) -> Vec<String> {
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            let mut row = String::new();
            let mut x = 0;
            while x < buffer.area.width {
                let symbol = buffer[(x, y)].symbol();
                row.push_str(symbol);
                x += u16::try_from(symbol.width().max(1)).expect("cell width");
            }
            row.trim_end().to_owned()
        })
        .collect()
}

fn row_with<'a>(rows: &'a [String], needle: &str) -> &'a str {
    rows.iter()
        .find(|row| row.contains(needle))
        .unwrap_or_else(|| panic!("no row contains {needle:?}: {rows:#?}"))
}

#[test]
fn main_screen_groups_host_facts_and_compact_columns() {
    let now = Instant::now();
    let mut app = app();
    with_groups(
        &mut app,
        vec![
            group("g_docs", "docs", GroupStatus::Offline, 0),
            group("g_dev", "cccc-dev", GroupStatus::Running, 2),
        ],
        now,
    );
    with_sessions(
        &mut app,
        vec![
            source(LocalSessionRuntime::Codex, "/home/me/dev/app-a", "A", true),
            source(
                LocalSessionRuntime::Claude,
                "/home/me/dev/app-b",
                "B",
                false,
            ),
        ],
        now,
    );
    let rows = screen_rows(&draw(&app, 80, 24, now));
    assert_eq!(rows[0], " CCCC 0.4.45   Daemon started here; stops on quit");
    assert_eq!(rows[1], " Web   http://127.0.0.1:8848");
    assert_eq!(rows[2], " Home  ~/.cccc");
    assert_eq!(rows[4], " Groups  2 groups, 1 running");
    let heading = row_with(&rows, "NAME");
    assert!(heading.contains("STATUS") && heading.ends_with("RUNNING"));
    let selected = row_with(&rows, "cccc-dev");
    assert!(selected.starts_with(" > ") && selected.contains("Running") && selected.ends_with('2'));
    let offline = row_with(&rows, "docs");
    assert!(offline.contains("Offline") && offline.ends_with('0'));
    assert!(row_with(&rows, "Native sessions").contains("2 known, 1 can receive"));
    assert!(row_with(&rows, "app-a").ends_with("ready"));
    assert!(row_with(&rows, "app-b").ends_with("not receiving"));
    assert!(rows[21].contains("Enter details") && rows[23].contains("Type / for commands"));
}

#[test]
fn wide_screens_keep_metadata_and_actions_in_one_bounded_canvas() {
    let now = Instant::now();
    let mut app = app();
    with_groups(
        &mut app,
        vec![group("g", "日本語プロジェクト", GroupStatus::Running, 2)],
        now,
    );
    for width in [100, 122, 180] {
        let rows = screen_rows(&draw(&app, width, 24, now));
        let margin = usize::from(width.saturating_sub(100) / 2);
        assert_eq!(rows[0].find("CCCC"), Some(margin + 1));
        assert!(rows.iter().all(|row| row.width() <= margin + 100));
        assert!(
            row_with(&rows, "Running")
                .find("Running")
                .expect("status column")
                < margin + 60
        );
    }
}

#[test]
fn readiness_color_and_selected_marker_work_without_color() {
    let now = Instant::now();
    let mut app = app();
    with_groups(
        &mut app,
        vec![group("g", "alpha", GroupStatus::Running, 2)],
        now,
    );
    with_sessions(
        &mut app,
        vec![source(LocalSessionRuntime::Pi, "/srv/app", "", true)],
        now,
    );
    for color in [true, false] {
        app.context.color = color;
        let terminal = draw(&app, 60, 16, now);
        let rows = screen_rows(&terminal);
        let y = rows
            .iter()
            .position(|row| row.contains("ready"))
            .expect("ready row");
        let x = rows[y].find("ready").expect("readiness cell");
        assert_eq!(
            terminal.backend().buffer()[(x as u16, y as u16)].fg,
            if color { Color::Green } else { Color::Reset }
        );
        let y = rows
            .iter()
            .position(|row| row.contains("alpha"))
            .expect("selected row");
        assert!(rows[y].starts_with(" > "));
        assert!(
            terminal.backend().buffer()[(3, y as u16)]
                .modifier
                .contains(ratatui::style::Modifier::REVERSED)
        );
    }
}

#[test]
fn overflowing_lists_keep_routes_and_controls_on_the_smallest_screen() {
    let now = Instant::now();
    let mut app = app();
    with_groups(
        &mut app,
        (0..10)
            .map(|i| {
                group(
                    &format!("g_{i}"),
                    &format!("group-{i}"),
                    GroupStatus::Stopped,
                    0,
                )
            })
            .collect(),
        now,
    );
    with_sessions(
        &mut app,
        (0..10)
            .map(|i| source(LocalSessionRuntime::Pi, &format!("/srv/{i}"), "", true))
            .collect(),
        now,
    );
    let rows = screen_rows(&draw(&app, 60, 16, now));
    assert!(row_with(&rows, "/groups shows all").contains("more"));
    assert!(row_with(&rows, "/sessions shows all").contains("more"));
    assert!(row_with(&rows, "group-0").starts_with(" > "));
    assert!(rows[13].contains("Enter details"));
    assert!(rows[15].starts_with(" >"));
    for _ in 0..8 {
        app.handle(press(KeyCode::Down));
    }
    let rows = screen_rows(&draw(&app, 60, 16, now));
    assert!(row_with(&rows, "group-8").starts_with(" > "), "{rows:#?}");
}

#[test]
fn empty_lists_offer_setup_and_collaboration_without_faking_readiness() {
    let now = Instant::now();
    let mut app = app();
    with_groups(&mut app, Vec::new(), now);
    with_sessions(&mut app, Vec::new(), now);
    for width in [60, 80] {
        let rows = screen_rows(&draw(&app, width, 16, now));
        assert!(row_with(&rows, "Groups").contains("0 groups, 0 running"));
        assert!(row_with(&rows, "No groups yet").contains("Web"));
        assert!(row_with(&rows, "Native sessions").contains("0 known, 0 can receive"));
        assert!(row_with(&rows, "No sessions yet").contains("/setup"));
        assert!(rows[15].contains("Type / for commands"));
    }
}

#[test]
fn unavailable_and_stale_lists_expose_freshness_and_keep_old_data() {
    let first = Instant::now();
    let mut app = app();
    app.groups.record(Err("daemon unavailable".into()), first);
    app.sessions.record(Err("daemon unavailable".into()), first);
    let rows = screen_rows(&draw(&app, 80, 24, first));
    assert!(row_with(&rows, "Groups").ends_with("not available"));
    assert!(row_with(&rows, "Native sessions").ends_with("not available"));
    assert!(row_with(&rows, "Could not read groups").contains("daemon unavailable"));
    assert!(row_with(&rows, "Could not read sessions").contains("daemon unavailable"));
    assert!(!rows.iter().any(|row| row.contains("known")));
    with_groups(
        &mut app,
        vec![group("g", "alpha", GroupStatus::Running, 1)],
        first,
    );
    with_sessions(
        &mut app,
        vec![source(LocalSessionRuntime::Claude, "/srv/app", "", true)],
        first,
    );
    app.groups.record(Err("offline".into()), first);
    app.sessions.record(Err("offline".into()), first);
    let rows = screen_rows(&draw(&app, 80, 24, first + Duration::from_secs(12)));
    assert!(row_with(&rows, "Groups").contains("last ok 12s ago"));
    assert!(row_with(&rows, "Native sessions").contains("last ok 12s ago"));
    assert!(
        rows.iter().any(|row| row.contains("alpha"))
            && rows.iter().any(|row| row.contains("/srv/app"))
    );
    assert!(super::shows_age(&app));
}

#[test]
fn initial_reads_do_not_look_like_empty_lists() {
    let now = Instant::now();
    let rows = screen_rows(&draw(&app(), 80, 16, now));
    assert!(row_with(&rows, "Groups").ends_with("reading..."));
    assert!(row_with(&rows, "Native sessions").ends_with("reading..."));
    assert!(
        !rows
            .iter()
            .any(|row| row.contains("No sessions") || row.contains("No groups"))
    );
}

#[test]
fn unicode_and_terminal_controls_do_not_escape_their_columns() {
    let now = Instant::now();
    let mut app = app();
    with_sessions(
        &mut app,
        vec![
            source(
                LocalSessionRuntime::Codex,
                "/srv/日本語のとても長いプロジェクト名前/さらに深いディレクトリ",
                "",
                true,
            ),
            source(
                LocalSessionRuntime::Claude,
                "/srv/x\u{1b}[2J",
                "red\u{1b}[31m title\nnext",
                false,
            ),
        ],
        now,
    );
    let rows = screen_rows(&draw(&app, 60, 16, now));
    let wide = row_with(&rows, "/srv/日本語");
    assert!(wide.contains("...") && wide.ends_with("ready") && wide.width() <= 59);
    assert!(!rows.join("\n").contains('\u{1b}'));
    for ch in "/sessions".chars() {
        app.handle(press(KeyCode::Char(ch)));
    }
    app.handle(press(KeyCode::Enter));
    app.handle(press(KeyCode::Down));
    app.handle(press(KeyCode::Enter));
    let rows = screen_rows(&draw(&app, 60, 24, now));
    assert!(row_with(&rows, "red[31m title next").contains("title"));
    assert!(!rows.join("\n").contains('\u{1b}'));
}

#[test]
fn external_ownership_and_web_errors_remain_visible() {
    let now = Instant::now();
    let mut app = app();
    app.host.daemon = DaemonState::External { pid: Some(31337) };
    let rows = screen_rows(&draw(&app, 80, 16, now));
    assert!(rows[0].contains("external pid 31337; kept on quit"));
    app.host.daemon = DaemonState::External { pid: None };
    let rows = screen_rows(&draw(&app, 80, 16, now));
    assert!(rows[0].contains("external; kept on quit"));
    for (state, shown) in [
        (WebState::Starting, "starting..."),
        (WebState::Restarting, "restarting..."),
        (WebState::Stopped, "stopped"),
    ] {
        app.host.web = state;
        assert!(screen_rows(&draw(&app, 80, 16, now))[1].ends_with(shown));
    }
    app.host.web = WebState::Error {
        message: "port 8848 is in use".into(),
    };
    let rows = screen_rows(&draw(&app, 80, 16, now));
    assert!(rows[1].ends_with("error"));
    assert_eq!(rows[13], " Web error: port 8848 is in use");
}

#[test]
fn stopping_shows_the_existing_force_exit_rules() {
    let now = Instant::now();
    let mut app = app();
    app.host.phase = HostPhase::Stopping;
    let rows = screen_rows(&draw(&app, 80, 16, now));
    assert_eq!(
        rows[13],
        " Stopping CCCC... press Ctrl+C twice to force exit."
    );
    assert_eq!(rows[15], " >");
    app.interrupts = 1;
    let rows = screen_rows(&draw(&app, 80, 16, now));
    assert_eq!(
        rows[13],
        " Stopping CCCC... press Ctrl+C again to force exit."
    );
}

#[test]
fn command_menu_has_descriptions_and_keeps_the_notice_and_prompt() {
    let now = Instant::now();
    let mut app = app();
    app.notice = Some(Notice::Error("Unknown command /x. Try /help.".into()));
    for ch in "/s".chars() {
        app.handle(press(KeyCode::Char(ch)));
    }
    let mut terminal = draw(&app, 60, 16, now);
    let rows = screen_rows(&terminal);
    assert!(row_with(&rows, "/sessions").contains("Browse native sessions"));
    assert!(row_with(&rows, "/setup").contains("Connect Codex"));
    assert_eq!(rows[13], " Unknown command /x. Try /help.");
    assert_eq!(rows[15], " > /s");
    terminal.backend_mut().assert_cursor_position((5, 15));
}

#[test]
fn long_input_scrolls_to_the_cursor_not_just_the_end() {
    let now = Instant::now();
    let mut app = app();
    app.handle(Event::Paste(format!("/{}", "x".repeat(100))));
    let rows = screen_rows(&draw(&app, 60, 16, now));
    assert_eq!(rows[15], format!(" > {}", "x".repeat(56)));
    app.handle(press(KeyCode::Home));
    let mut terminal = draw(&app, 60, 16, now);
    assert!(screen_rows(&terminal)[15].starts_with(" > /xxx"));
    terminal.backend_mut().assert_cursor_position((3, 15));
}

#[test]
fn help_lists_commands_and_scrolls_to_the_headless_exit() {
    let now = Instant::now();
    let mut app = app();
    app.screen = Screen::Help;
    let rows = screen_rows(&draw(&app, 60, 16, now));
    assert!(row_with(&rows, "/groups").contains("Browse groups"));
    assert!(row_with(&rows, "/sessions").contains("Browse native sessions"));
    assert_eq!(rows[15], " Up/Down scroll   b back");
    app.handle(press(KeyCode::End));
    let rows = screen_rows(&draw(&app, 60, 16, now));
    assert_eq!(
        row_with(&rows, "cccc --headless"),
        "   cccc --headless (or cccc web)"
    );
    assert!(rows.iter().any(|row| row.contains("cccc sessions disable")));
    app.handle(press(KeyCode::Char('b')));
    assert_eq!(app.screen, Screen::Main);
}

#[test]
fn small_terminal_retains_an_explicit_way_out() {
    let now = Instant::now();
    let mut app = app();
    let rows = screen_rows(&draw(&app, 40, 10, now));
    assert_eq!(rows[0], "Terminal too small (need 60x16). Ctrl+C");
    assert_eq!(rows[1], "stops CCCC.");
    assert_eq!(rows[9], "Resize to continue.");
    assert!(screen_rows(&draw(&app, 80, 15, now))[0].starts_with("Terminal too small"));
    for screen in [
        Screen::Groups,
        Screen::Sessions,
        Screen::Tasks,
        Screen::Help,
        Screen::Setup,
    ] {
        app.screen = screen;
        let rows = screen_rows(&draw(&app, 40, 10, now));
        assert!(rows[0].contains("Ctrl+C"));
        assert!(
            !rows[9].starts_with(" >"),
            "a secondary page has no command input"
        );
    }
}

fn press(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new_with_kind(
        code,
        KeyModifiers::NONE,
        KeyEventKind::Press,
    ))
}

#[test]
fn the_receipt_reports_what_actually_stopped() {
    let log = std::path::PathBuf::from("/home/me/.cccc/daemon/ccccd.log");
    assert_eq!(
        receipt(&ShutdownReport {
            web: StopResult::Stopped,
            daemon: StopResult::Stopped,
            log_path: log.clone(),
        }),
        "CCCC stopped.\n  Web     stopped\n  Daemon  stopped"
    );
    assert_eq!(
        receipt(&ShutdownReport {
            web: StopResult::Stopped,
            daemon: StopResult::KeptExternal { pid: Some(7) },
            log_path: log.clone(),
        }),
        "CCCC stopped.\n  Web     stopped\n  Daemon  kept running (external, pid 7)"
    );
    assert_eq!(
        receipt(&ShutdownReport {
            web: StopResult::Stopped,
            daemon: StopResult::Failed {
                message: "did not stop\nin time".into(),
            },
            log_path: log,
        }),
        format!(
            "CCCC stopped with problems.\n  Web     stopped\n  Daemon  failed: did not stop in time\n  Log     {}",
            std::path::Path::new("/home/me/.cccc/daemon/ccccd.log").display()
        )
    );
}

#[test]
fn source_details_scroll_to_the_complete_id_without_truncating_it() {
    let now = Instant::now();
    let mut app = app();
    let mut native = source(
        LocalSessionRuntime::Claude,
        "/srv/a very long directory name/日本語のパス/another/segment",
        "A native conversation",
        true,
    );
    native.source_id = format!("claude:{}:1114701:30037069", "long-identity".repeat(8));
    native.native_session_id = "conversation-final-sentinel".into();
    let mut list = crate::tui::sessions::SessionList::default();
    list.record(Ok(vec![native]), now);
    app.set_sessions(list);
    for ch in "/sessions".chars() {
        app.handle(press(KeyCode::Char(ch)));
    }
    app.handle(press(KeyCode::Enter));
    app.handle(press(KeyCode::Enter));
    let rows = screen_rows(&draw(&app, 60, 16, now));
    assert!(row_with(&rows, "A native conversation").contains("conversation"));
    app.handle(press(KeyCode::End));
    let rows = screen_rows(&draw(&app, 60, 16, now));
    assert!(row_with(&rows, "conversation-final-sentinel").contains("sentinel"));
    assert!(rows[15].contains("b back"));
}
