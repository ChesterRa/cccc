use super::{App, Effect, Notice, Screen, SetupStep};
use crate::host_state::{HostIntent, HostPhase};
use crate::tui::sessions::SessionList;
use crate::tui::setup::{InstallState, SetupCommand, SetupProgress};
use crate::tui::test_support::{checks, context, running, source};
use cccc_contracts::local_session::LocalSessionRuntime;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

fn app() -> App {
    App::new(context(), running())
}

fn key(code: KeyCode, kind: KeyEventKind) -> Event {
    Event::Key(KeyEvent::new_with_kind(code, KeyModifiers::NONE, kind))
}

fn press(code: KeyCode) -> Event {
    key(code, KeyEventKind::Press)
}

fn control(letter: char, kind: KeyEventKind) -> Event {
    Event::Key(KeyEvent::new_with_kind(
        KeyCode::Char(letter),
        KeyModifiers::CONTROL,
        kind,
    ))
}

fn type_text(app: &mut App, typed: &str) {
    for ch in typed.chars() {
        assert_eq!(app.handle(press(KeyCode::Char(ch))), None);
    }
}

fn submit(app: &mut App, typed: &str) -> Option<Effect> {
    type_text(app, typed);
    app.handle(press(KeyCode::Enter))
}

#[test]
fn quit_asks_the_host_once_and_then_ignores_typing() {
    let mut app = app();
    assert!(app.want().sessions);
    assert_eq!(
        submit(&mut app, "/quit"),
        Some(Effect::Host(HostIntent::Quit))
    );
    assert!(app.stopping());
    assert!(!app.want().sessions);
    assert_eq!(submit(&mut app, "/quit"), None);
    assert_eq!(app.input, "");
}

#[test]
fn exit_and_capitals_mean_quit_too() {
    assert_eq!(
        submit(&mut app(), "/exit"),
        Some(Effect::Host(HostIntent::Quit))
    );
    assert_eq!(
        submit(&mut app(), " /QUIT "),
        Some(Effect::Host(HostIntent::Quit))
    );
}

#[test]
fn every_ctrl_c_press_goes_to_the_host_which_counts_them() {
    let mut app = app();
    assert_eq!(
        app.handle(control('c', KeyEventKind::Press)),
        Some(Effect::Host(HostIntent::Interrupt))
    );
    assert!(app.stopping());
    assert_eq!(
        app.handle(control('c', KeyEventKind::Press)),
        Some(Effect::Host(HostIntent::Interrupt))
    );
    assert_eq!(app.interrupts, 2);
}

#[test]
fn a_held_or_released_key_never_repeats_an_action() {
    let mut app = app();
    assert_eq!(app.handle(control('c', KeyEventKind::Repeat)), None);
    assert_eq!(app.handle(control('c', KeyEventKind::Release)), None);
    assert_eq!(app.interrupts, 0);

    type_text(&mut app, "/quit");
    assert_eq!(app.handle(key(KeyCode::Enter, KeyEventKind::Repeat)), None);
    assert_eq!(app.handle(key(KeyCode::Enter, KeyEventKind::Release)), None);
    assert!(!app.stopping());
    assert_eq!(app.input, "/quit");
}

#[test]
fn holding_a_key_still_types_and_deletes() {
    let mut app = app();
    app.handle(key(KeyCode::Char('/'), KeyEventKind::Press));
    app.handle(key(KeyCode::Char('h'), KeyEventKind::Repeat));
    app.handle(key(KeyCode::Char('h'), KeyEventKind::Release));
    assert_eq!(app.input, "/h");
    app.handle(key(KeyCode::Backspace, KeyEventKind::Repeat));
    assert_eq!(app.input, "/");
}

#[test]
fn help_covers_the_lists_and_b_comes_back() {
    let mut app = app();
    assert_eq!(submit(&mut app, "/help"), None);
    assert_eq!(app.screen, Screen::Help);
    assert!(!app.want().sessions && !app.want().groups);
    app.handle(press(KeyCode::Char('x')));
    assert_eq!(app.screen, Screen::Help);
    app.handle(press(KeyCode::Char('b')));
    assert_eq!(app.screen, Screen::Main);
    assert!(app.want().sessions && app.want().groups);
    assert_eq!(app.want().members, None);

    submit(&mut app, "/help");
    app.handle(press(KeyCode::Esc));
    assert_eq!(app.screen, Screen::Main);
}

#[test]
fn ctrl_c_still_stops_from_help() {
    let mut app = app();
    submit(&mut app, "/help");
    assert_eq!(
        app.handle(control('c', KeyEventKind::Press)),
        Some(Effect::Host(HostIntent::Interrupt))
    );
}

#[test]
fn unknown_input_explains_itself() {
    let mut app = app();
    submit(&mut app, "/stop");
    assert_eq!(
        app.notice,
        Some(Notice::Error("Unknown command /stop. Try /help.".into()))
    );
    submit(&mut app, "quit");
    assert_eq!(
        app.notice,
        Some(Notice::Error("Commands start with /. Try /help.".into()))
    );
    assert_eq!(submit(&mut app, ""), None);
    assert_eq!(app.notice, None);
}

#[test]
fn tab_completes_the_visible_selection_without_running_it() {
    let mut app = app();
    type_text(&mut app, "/q");
    app.handle(press(KeyCode::Tab));
    assert_eq!(app.input, "/quit");

    app.handle(control('u', KeyEventKind::Press));
    assert_eq!(app.input, "");
    type_text(&mut app, "/");
    app.handle(press(KeyCode::Down));
    app.handle(press(KeyCode::Tab));
    assert_eq!(app.input, "/sessions");
    assert_eq!(app.screen, Screen::Main);
}

#[test]
fn a_pasted_line_break_cannot_submit() {
    let mut app = app();
    assert_eq!(
        app.handle(Event::Paste("/quit\n/help\u{1b}[2J".into())),
        None
    );
    assert_eq!(app.input, "/quit /help[2J");
    assert!(!app.stopping());
}

#[test]
fn input_is_bounded() {
    let mut app = app();
    app.handle(Event::Paste("x".repeat(500)));
    assert_eq!(app.input.chars().count(), 200);
}

#[test]
fn altgr_characters_are_text_not_chords() {
    let mut app = app();
    app.handle(Event::Key(KeyEvent::new(
        KeyCode::Char('/'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    )));
    assert_eq!(app.input, "/");
}

#[test]
fn suspend_is_explained_and_ctrl_l_repaints() {
    let mut app = app();
    assert_eq!(app.handle(control('z', KeyEventKind::Press)), None);
    assert_eq!(
        app.notice,
        Some(Notice::Info(
            "Suspend is not supported here; use /quit.".into()
        ))
    );
    assert_eq!(
        app.handle(control('l', KeyEventKind::Press)),
        Some(Effect::Repaint)
    );
}

#[test]
fn a_stopping_host_hides_the_list_and_takes_no_commands() {
    let mut app = app();
    app.host.phase = HostPhase::Stopping;
    assert!(app.stopping());
    assert!(!app.want().sessions);
    assert_eq!(submit(&mut app, "/help"), None);
    assert_eq!(app.screen, Screen::Main);
    assert_eq!(
        app.handle(control('c', KeyEventKind::Press)),
        Some(Effect::Host(HostIntent::Interrupt))
    );
}

fn open_setup(app: &mut App) -> Option<Effect> {
    submit(app, "/setup")
}

fn checks_arrive(app: &mut App) {
    let generation = app.flow.as_ref().expect("setup flow").generation;
    app.setup = SetupProgress {
        generation,
        checking: false,
        checks: checks(),
        installs: Vec::new(),
    };
}

fn step(app: &App) -> Option<SetupStep> {
    app.flow.as_ref().map(|flow| flow.step)
}

#[test]
fn setup_starts_a_check_for_this_visit() {
    let mut app = app();
    assert_eq!(
        open_setup(&mut app),
        Some(Effect::Setup(SetupCommand::Check { generation: 1 }))
    );
    assert_eq!(app.screen, Screen::Setup);
    assert!(app.want().sessions);
    assert_eq!(app.checks(), None);

    // Results left by an earlier visit are not this visit's.
    app.setup = SetupProgress {
        generation: 0,
        checks: checks(),
        ..SetupProgress::default()
    };
    assert_eq!(app.checks(), None);
    app.handle(press(KeyCode::Char(' ')));
    app.handle(press(KeyCode::Enter));
    assert_eq!(step(&app), Some(SetupStep::Select));
}

#[test]
fn leaving_setup_while_checking_cancels_the_check() {
    let mut app = app();
    open_setup(&mut app);
    assert_eq!(
        app.handle(press(KeyCode::Char('b'))),
        Some(Effect::Setup(SetupCommand::Cancel))
    );
    assert_eq!(app.screen, Screen::Main);
    assert_eq!(app.flow, None);
}

#[test]
fn only_found_apps_can_be_picked_and_one_is_needed() {
    let mut app = app();
    open_setup(&mut app);
    checks_arrive(&mut app);
    assert_eq!(app.picked(), Vec::new());
    app.handle(press(KeyCode::Enter));
    assert_eq!(step(&app), Some(SetupStep::Select));
    assert_eq!(
        app.notice,
        Some(Notice::Info("Pick at least one app with Space.".into()))
    );

    for _ in 0..5 {
        app.handle(key(KeyCode::Down, KeyEventKind::Repeat));
    }
    app.handle(press(KeyCode::Char(' ')));
    assert_eq!(app.picked(), Vec::new(), "OpenCode was not found");
    app.handle(press(KeyCode::Up));
    app.handle(press(KeyCode::Char(' ')));
    app.handle(key(KeyCode::Char(' '), KeyEventKind::Repeat));
    assert_eq!(app.picked(), vec![LocalSessionRuntime::Pi]);
}

#[test]
fn confirming_installs_the_picked_apps_in_order() {
    let mut app = app();
    open_setup(&mut app);
    checks_arrive(&mut app);
    app.handle(press(KeyCode::Down));
    app.handle(press(KeyCode::Char(' ')));
    app.handle(press(KeyCode::Up));
    app.handle(press(KeyCode::Char(' ')));
    app.handle(press(KeyCode::Enter));
    assert_eq!(step(&app), Some(SetupStep::Confirm));
    app.handle(press(KeyCode::Char('b')));
    assert_eq!(step(&app), Some(SetupStep::Select));
    app.handle(press(KeyCode::Enter));
    assert_eq!(app.handle(key(KeyCode::Enter, KeyEventKind::Repeat)), None);
    assert_eq!(
        app.handle(press(KeyCode::Enter)),
        Some(Effect::Setup(SetupCommand::Install {
            generation: 1,
            runtimes: vec![LocalSessionRuntime::Codex, LocalSessionRuntime::Claude],
        }))
    );
    assert_eq!(step(&app), Some(SetupStep::Finish));
    assert!(app.install_running());
}

#[test]
fn a_running_install_can_only_be_cancelled_then_left_once_done() {
    let mut app = app();
    open_setup(&mut app);
    checks_arrive(&mut app);
    app.handle(press(KeyCode::Char(' ')));
    app.handle(press(KeyCode::Enter));
    app.handle(press(KeyCode::Enter));

    assert_eq!(app.handle(press(KeyCode::Enter)), None);
    assert_eq!(
        app.handle(press(KeyCode::Char('b'))),
        Some(Effect::Setup(SetupCommand::Cancel))
    );
    assert_eq!(app.screen, Screen::Setup);

    app.setup.installs = vec![(LocalSessionRuntime::Codex, InstallState::Interrupted)];
    assert!(!app.install_running());
    assert_eq!(app.handle(press(KeyCode::Enter)), None);
    assert_eq!(app.screen, Screen::Main);
    assert_eq!(app.flow, None);
}

#[test]
fn new_sessions_are_the_ones_missing_when_setup_opened() {
    let mut app = app();
    let now = std::time::Instant::now();
    let old = source(LocalSessionRuntime::Codex, "/srv/old", "", true);
    let new = source(LocalSessionRuntime::Claude, "/srv/new", "", false);
    let mut list = SessionList::default();
    list.record(Ok(vec![old.clone()]), now);
    app.set_sessions(list.clone());
    open_setup(&mut app);
    list.record(Ok(vec![old, new.clone()]), now);
    app.set_sessions(list);
    assert_eq!(app.new_sessions(), vec![&new]);
}

#[test]
fn without_a_list_yet_the_first_one_read_is_the_baseline() {
    let mut app = app();
    open_setup(&mut app);
    assert!(app.new_sessions().is_empty());
    let now = std::time::Instant::now();
    let mut list = SessionList::default();
    list.record(
        Ok(vec![source(LocalSessionRuntime::Pi, "/srv/a", "", true)]),
        now,
    );
    app.set_sessions(list);
    assert!(app.new_sessions().is_empty());
}

mod groups {
    use super::{app, control, press, submit};
    use crate::tui::app::{App, Effect, Notice, Screen};
    use crate::tui::groups::{
        ActionProgress, GroupAction, GroupCommand, GroupList, GroupStatus, GroupSummary,
        MemberList, Outcome,
    };
    use crossterm::event::{KeyCode, KeyEventKind};
    use std::time::Instant;

    fn group(group_id: &str, title: &str, status: GroupStatus, state: &str) -> GroupSummary {
        GroupSummary {
            group_id: group_id.into(),
            title: title.into(),
            status,
            state: state.into(),
            running_actors: 0,
        }
    }

    fn with_groups(app: &mut App, groups: Vec<GroupSummary>) {
        let mut list = GroupList::default();
        list.record(Ok(groups), Instant::now());
        app.set_groups(list);
    }

    fn three() -> Vec<GroupSummary> {
        vec![
            group("g_a", "alpha", GroupStatus::Running, "active"),
            group("g_b", "beta", GroupStatus::Paused, "paused"),
            group("g_c", "gamma", GroupStatus::Offline, "active"),
        ]
    }

    fn open(app: &mut App) {
        assert_eq!(submit(app, "/groups"), None);
        assert_eq!(app.screen, Screen::Groups);
    }

    fn selected(app: &App) -> Option<&str> {
        app.groups_view.selected.as_deref()
    }

    #[test]
    fn main_selects_by_id_and_enter_does_not_fall_to_a_neighbour() {
        let mut app = app();
        with_groups(&mut app, three());
        app.handle(press(KeyCode::Down));
        assert_eq!(selected(&app), Some("g_b"));
        assert!(app.want().members.is_none());
        app.handle(press(KeyCode::Enter));
        assert_eq!(app.screen, Screen::Groups);
        assert_eq!(app.want().members.as_deref(), Some("g_b"));
        app.handle(press(KeyCode::Char('b')));
        with_groups(&mut app, vec![three()[0].clone()]);
        app.handle(press(KeyCode::Enter));
        assert_eq!(app.screen, Screen::Main);
        assert_eq!(selected(&app), Some("g_b"));
        submit(&mut app, "/g");
        assert_eq!(app.screen, Screen::Groups);
        assert_eq!(selected(&app), Some("g_a"));
    }

    #[test]
    fn groups_opens_on_the_first_group_and_reads_only_its_members() {
        let mut app = app();
        with_groups(&mut app, three());
        open(&mut app);
        assert_eq!(selected(&app), Some("g_a"));
        let want = app.want();
        assert!(want.groups && !want.sessions);
        assert_eq!(want.members.as_deref(), Some("g_a"));
        app.handle(press(KeyCode::Down));
        assert_eq!(app.want().members.as_deref(), Some("g_b"));
        app.handle(press(KeyCode::End));
        assert_eq!(selected(&app), Some("g_c"));
        app.handle(press(KeyCode::Down));
        assert_eq!(selected(&app), Some("g_c"), "the last stays selected");
        app.handle(press(KeyCode::Home));
        assert_eq!(selected(&app), Some("g_a"));
        app.handle(press(KeyCode::Char('b')));
        assert_eq!(app.screen, Screen::Main);
        assert_eq!(app.want().members, None);
    }

    #[test]
    fn the_selection_follows_its_group_when_the_list_re_sorts() {
        let mut app = app();
        with_groups(&mut app, three());
        open(&mut app);
        app.handle(press(KeyCode::Down));
        assert_eq!(selected(&app), Some("g_b"));
        // beta starts running and moves to the top.
        with_groups(
            &mut app,
            vec![
                group("g_b", "beta", GroupStatus::Running, "active"),
                group("g_a", "alpha", GroupStatus::Running, "active"),
                group("g_c", "gamma", GroupStatus::Offline, "active"),
            ],
        );
        assert_eq!(selected(&app), Some("g_b"));
    }

    #[test]
    fn s_and_x_ask_first_and_enter_sends_the_pinned_action() {
        let mut app = app();
        with_groups(&mut app, three());
        open(&mut app);
        // A paused group is worded as resuming, and can be stopped directly.
        app.handle(press(KeyCode::Down));
        assert_eq!(app.handle(press(KeyCode::Char('s'))), None);
        let confirm = app.groups_view.confirm.clone().expect("confirmation");
        assert_eq!(confirm.group_id, "g_b");
        assert_eq!(confirm.action, GroupAction::Start { resume: true });
        assert_eq!(app.handle(press(KeyCode::Char('b'))), None);
        assert!(app.groups_view.confirm.is_none());
        assert_eq!(app.screen, Screen::Groups, "b cancels only the question");

        assert_eq!(app.handle(press(KeyCode::Char('x'))), None);
        // A refresh that re-sorts the list changes neither group nor action.
        with_groups(
            &mut app,
            vec![
                group("g_c", "gamma", GroupStatus::Running, "active"),
                group("g_b", "beta", GroupStatus::Running, "active"),
            ],
        );
        let Some(Effect::Group(sent)) = app.handle(press(KeyCode::Enter)) else {
            panic!("Enter sends the confirmed action");
        };
        assert_eq!(
            (sent.group_id.as_str(), sent.action),
            ("g_b", GroupAction::Stop)
        );
        assert!(app.groups_view.confirm.is_none());
    }

    #[test]
    fn a_shown_stopped_group_can_still_be_stopped_and_a_running_one_started() {
        let mut app = app();
        with_groups(&mut app, three());
        open(&mut app);
        app.handle(press(KeyCode::End));
        app.handle(press(KeyCode::Char('x')));
        assert_eq!(
            app.groups_view
                .confirm
                .as_ref()
                .map(|confirm| confirm.action),
            Some(GroupAction::Stop)
        );
        app.handle(press(KeyCode::Esc));
        app.handle(press(KeyCode::Home));
        app.handle(press(KeyCode::Char('s')));
        assert_eq!(
            app.groups_view
                .confirm
                .as_ref()
                .map(|confirm| confirm.action),
            Some(GroupAction::Start { resume: false })
        );
    }

    #[test]
    fn actions_happen_only_on_a_key_press() {
        let mut app = app();
        with_groups(&mut app, three());
        open(&mut app);
        let repeat = |code| super::key(code, KeyEventKind::Repeat);
        assert_eq!(app.handle(repeat(KeyCode::Char('x'))), None);
        assert!(app.groups_view.confirm.is_none());
        app.handle(press(KeyCode::Char('x')));
        assert_eq!(app.handle(repeat(KeyCode::Enter)), None);
        assert!(app.groups_view.confirm.is_some());
    }

    #[test]
    fn a_group_that_leaves_the_list_cannot_be_acted_on() {
        let mut app = app();
        with_groups(&mut app, three());
        open(&mut app);
        app.handle(press(KeyCode::Char('x')));
        with_groups(&mut app, three()[1..].to_vec());
        assert_eq!(app.handle(press(KeyCode::Enter)), None);
        assert!(
            matches!(&app.notice, Some(Notice::Error(message)) if message.contains("no longer listed"))
        );
        // The gone group stays selected rather than handing the keys to another.
        assert_eq!(selected(&app), Some("g_a"));
        assert_eq!(app.handle(press(KeyCode::Char('x'))), None);
        assert!(app.groups_view.confirm.is_none());
        // Moving picks a listed group again.
        app.handle(press(KeyCode::Down));
        assert_eq!(selected(&app), Some("g_b"));
    }

    fn sent(app: &mut App) -> GroupCommand {
        app.handle(press(KeyCode::Char('x')));
        match app.handle(press(KeyCode::Enter)) {
            Some(Effect::Group(command)) => command,
            other => panic!("expected an action, got {other:?}"),
        }
    }

    #[test]
    fn one_action_at_a_time_and_its_outcome_is_reported_then_read_again() {
        let mut app = app();
        with_groups(&mut app, three());
        open(&mut app);
        let command = sent(&mut app);
        // Before the task even reports it, a second action cannot be asked.
        app.handle(press(KeyCode::Char('s')));
        assert!(app.groups_view.confirm.is_none());
        assert!(
            matches!(&app.notice, Some(Notice::Info(message)) if message.starts_with("Wait for stopping alpha"))
        );
        app.set_action(ActionProgress {
            running: Some(command.clone()),
            last: None,
        });
        assert_eq!(app.busy_action(), Some(&command));
        // Going back and quitting stay possible meanwhile.
        app.handle(press(KeyCode::Char('b')));
        assert_eq!(app.screen, Screen::Main);
        let nonce = app.want().nonce;
        app.set_action(ActionProgress {
            running: None,
            last: Some((command.clone(), Outcome::Done)),
        });
        assert_eq!(app.notice, Some(Notice::Info("alpha: stop done.".into())));
        assert_eq!(app.busy_action(), None);
        assert!(app.want().nonce > nonce, "the lists are read again");
        assert_eq!(
            app.handle(control('c', KeyEventKind::Press)),
            Some(Effect::Host(crate::host_state::HostIntent::Interrupt))
        );
    }

    #[test]
    fn failed_and_unconfirmed_actions_say_so() {
        let mut app = app();
        with_groups(&mut app, three());
        open(&mut app);
        let command = sent(&mut app);
        app.set_action(ActionProgress {
            running: None,
            last: Some((command, Outcome::Failed("runtime_error: no runtime".into()))),
        });
        assert_eq!(
            app.notice,
            Some(Notice::Error(
                "Could not stop alpha: runtime_error: no runtime".into()
            ))
        );
        let command = sent(&mut app);
        let nonce = app.want().nonce;
        app.set_action(ActionProgress {
            running: None,
            last: Some((command, Outcome::Unknown("request timed out".into()))),
        });
        assert_eq!(
            app.notice,
            Some(Notice::Error(
                "alpha: stop not confirmed (request timed out); check its state.".into()
            ))
        );
        assert!(app.want().nonce > nonce);
    }

    #[test]
    fn members_show_only_for_the_group_they_were_read_for() {
        let mut app = app();
        with_groups(&mut app, three());
        open(&mut app);
        let mut members = MemberList::default();
        members.record("g_a", Ok(Vec::new()), Instant::now());
        app.members = members;
        assert!(app.selected_members().is_some());
        app.handle(press(KeyCode::Down));
        assert!(
            app.selected_members().is_none(),
            "g_a's members are not g_b's"
        );
    }

    #[test]
    fn without_a_selected_group_the_keys_say_why_nothing_happens() {
        let mut app = app();
        open(&mut app);
        app.handle(press(KeyCode::Char('x')));
        assert!(app.groups_view.confirm.is_none());
        assert_eq!(
            app.notice,
            Some(Notice::Error("Groups are still being read.".into()))
        );
        with_groups(&mut app, Vec::new());
        app.handle(press(KeyCode::Char('s')));
        assert!(app.groups_view.confirm.is_none());
        assert_eq!(
            app.notice,
            Some(Notice::Error("There is no group to start or stop.".into()))
        );
    }
}

#[test]
fn command_candidates_are_visible_filtered_and_explicitly_chosen() {
    let mut app = app();
    type_text(&mut app, "/s");
    assert_eq!(
        app.candidates()
            .iter()
            .map(|index| super::COMMANDS[*index].0)
            .collect::<Vec<_>>(),
        ["/sessions", "/setup"]
    );
    app.handle(press(KeyCode::Down));
    assert_eq!(app.completion_index(), 1);
    app.handle(press(KeyCode::Esc));
    assert!(app.candidates().is_empty());
    assert_eq!(app.input, "/s");
    app.handle(press(KeyCode::Tab));
    assert_eq!(app.input, "/setup");
    assert_eq!(app.screen, Screen::Main);
    assert!(matches!(
        app.handle(press(KeyCode::Enter)),
        Some(Effect::Setup(SetupCommand::Check { .. }))
    ));
}

#[test]
fn editing_moves_by_utf8_boundaries_and_never_changes_screen() {
    let mut app = app();
    type_text(&mut app, "/he日lp");
    app.handle(press(KeyCode::Left));
    app.handle(press(KeyCode::Left));
    app.handle(press(KeyCode::Backspace));
    assert_eq!(app.input, "/help");
    app.handle(press(KeyCode::Home));
    app.handle(press(KeyCode::Delete));
    assert_eq!(app.input, "help");
    app.handle(press(KeyCode::Char('/')));
    app.handle(control('e', KeyEventKind::Press));
    assert_eq!(app.input_cursor, app.input.len());
    app.handle(control('a', KeyEventKind::Press));
    assert_eq!(app.input_cursor, 0);
    assert_eq!(app.screen, Screen::Main);
    app.handle(press(KeyCode::Enter));
    assert_eq!(app.screen, Screen::Help);
}

#[test]
fn pasted_multiple_commands_do_not_run_a_prefix_even_on_enter() {
    let mut app = app();
    app.handle(Event::Paste("/quit\n/setup".into()));
    assert!(app.candidates().is_empty());
    assert_eq!(app.handle(press(KeyCode::Enter)), None);
    assert_eq!(app.screen, Screen::Main);
    assert!(!app.stopping());
    assert!(matches!(app.notice, Some(Notice::Error(_))));
}

#[test]
fn source_browser_preserves_identity_after_resort_and_removal() {
    let mut app = app();
    let a = source(LocalSessionRuntime::Codex, "/a", "alpha", true);
    let b = source(LocalSessionRuntime::Claude, "/b", "beta", true);
    let at = std::time::Instant::now();
    let mut list = SessionList::default();
    list.record(Ok(vec![a.clone(), b.clone()]), at);
    app.set_sessions(list);
    submit(&mut app, "/sessions");
    assert!(app.want().sessions && app.want().members.is_none());
    app.handle(press(KeyCode::Down));
    assert_eq!(app.selected_source(), Some(&b));
    app.handle(press(KeyCode::Enter));
    assert!(app.sessions_view.details);
    let mut list = SessionList::default();
    list.record(Ok(vec![a]), at);
    app.set_sessions(list);
    assert!(app.selected_source().is_none());
    assert_eq!(
        app.sessions_view.selected.as_deref(),
        Some(b.source_id.as_str())
    );
    app.handle(press(KeyCode::Char('b')));
    assert!(!app.sessions_view.details);
    app.handle(press(KeyCode::Char('b')));
    assert_eq!(app.screen, Screen::Main);
}
