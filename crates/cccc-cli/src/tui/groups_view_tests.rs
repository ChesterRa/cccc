use crate::tui::app::App;
use crate::tui::groups::{
    ActionProgress, GroupList, GroupStatus, GroupSummary, Member, MemberList, MemberState,
};
use crate::tui::test_support::{context, running};
use crate::tui::view::render;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::time::Instant;

fn press(app: &mut App, code: KeyCode) {
    app.handle(Event::Key(KeyEvent::new_with_kind(
        code,
        KeyModifiers::NONE,
        KeyEventKind::Press,
    )));
}

fn type_command(app: &mut App, command: &str) {
    for ch in command.chars() {
        press(app, KeyCode::Char(ch));
    }
    press(app, KeyCode::Enter);
}

fn group(
    group_id: &str,
    title: &str,
    status: GroupStatus,
    state: &str,
    running: u64,
) -> GroupSummary {
    GroupSummary {
        group_id: group_id.into(),
        title: title.into(),
        status,
        state: state.into(),
        running_actors: running,
    }
}

fn app_with(groups: Vec<GroupSummary>) -> App {
    let mut app = App::new(context(), running());
    let mut list = GroupList::default();
    list.record(Ok(groups), Instant::now());
    app.set_groups(list);
    type_command(&mut app, "/groups");
    app
}

fn member(id: &str, runtime: &str, role: &str, native: bool, state: MemberState) -> Member {
    Member {
        id: id.into(),
        runtime: runtime.into(),
        role: role.into(),
        native,
        state,
    }
}

fn with_members(app: &mut App, group_id: &str, members: Vec<Member>) {
    let mut list = MemberList::default();
    list.record(group_id, Ok(members), Instant::now());
    app.members = list;
}

fn draw_rows(app: &App, width: u16, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
    terminal
        .draw(|frame| render(frame, app, Instant::now()))
        .expect("draw");
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect()
}

fn demo() -> Vec<GroupSummary> {
    vec![
        group("g_dev", "cccc-dev", GroupStatus::Running, "active", 2),
        group("g_docs", "docs", GroupStatus::Paused, "paused", 1),
        group("g_new", "empty", GroupStatus::Offline, "active", 0),
    ]
}

#[test]
fn groups_lists_states_and_the_selected_groups_members() {
    let mut app = app_with(demo());
    with_members(
        &mut app,
        "g_dev",
        vec![
            member("codex-1", "Codex", "foreman", false, MemberState::Running),
            member("claude-1", "Claude Code", "peer", true, MemberState::Ready),
            member("reviewer", "Codex", "peer", false, MemberState::Off),
        ],
    );
    let rows = draw_rows(&app, 80, 16);
    assert!(rows[0].starts_with(" Groups") && rows[0].ends_with("3 groups, 1 running"));
    assert!(rows[2].starts_with(" > cccc-dev"));
    assert!(rows[2].contains("Running") && rows[2].ends_with('2'));
    assert!(rows[3].starts_with("   docs") && rows[3].contains("Paused"));
    assert!(
        rows[4].starts_with("   empty") && rows[4].contains("Offline") && rows[4].ends_with('0')
    );
    assert_eq!(rows[5], "");
    assert_eq!(rows[6], " cccc-dev");
    assert_eq!(rows[7], " g_dev  State active | 2 actors running");
    assert_eq!(rows[8], " Web  http://127.0.0.1:8848/?group=g_dev");
    assert!(
        rows[10].starts_with("   codex-1")
            && rows[10].contains("foreman")
            && rows[10].contains("running")
    );
    assert!(
        rows[11].contains("claude-1")
            && rows[11].contains("Claude Code")
            && rows[11].ends_with("ready         native")
    );
    assert!(rows[9].contains("Tasks: reading"));
    assert!(rows[12].contains("reviewer") && rows[12].contains("off"));
    assert_eq!(
        rows[15],
        " Up/Down group   t tasks   s start   x stop   b back"
    );
}

#[test]
fn an_empty_started_group_shows_its_state_beside_the_running_count() {
    let mut app = app_with(demo());
    press(&mut app, KeyCode::End);
    with_members(&mut app, "g_new", Vec::new());
    let rows = draw_rows(&app, 80, 16);
    assert!(rows[4].starts_with(" > empty"));
    assert_eq!(rows[6], " empty");
    assert_eq!(rows[7], " g_new  State active | 0 actors running");
    assert_eq!(rows[10], "   No actors in this group.");
}

#[test]
fn confirmations_name_the_group_and_the_effect() {
    let mut app = app_with(demo());
    with_members(
        &mut app,
        "g_dev",
        vec![member(
            "codex-1",
            "Codex",
            "foreman",
            false,
            MemberState::Running,
        )],
    );
    press(&mut app, KeyCode::Char('x'));
    let rows = draw_rows(&app, 80, 16);
    assert_eq!(rows[2], " Stop cccc-dev?");
    assert_eq!(rows[3], " g_dev  State active | 2 actors running");
    assert_eq!(rows[4], "");
    // No native members, so nothing is said about them.
    assert_eq!(rows[5], " Its managed actors stop.");
    assert_eq!(rows[6], "");
    assert_eq!(rows[15], " Enter stop   b cancel");

    press(&mut app, KeyCode::Char('b'));
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Char('s'));
    let rows = draw_rows(&app, 60, 16);
    assert_eq!(rows[2], " Resume docs?");
    // Members not read yet: they may be native, so they are covered.
    let body = words(&rows[5..9]);
    assert!(
        body.contains("Delivery resumes and its enabled actors run"),
        "{body}"
    );
    assert!(
        body.contains("native members can receive from this group again"),
        "{body}"
    );
    assert_eq!(rows[15], " Enter resume   b cancel");
}

fn words(rows: &[String]) -> String {
    rows.iter()
        .flat_map(|row| row.split_whitespace())
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn the_whole_question_fits_the_smallest_screen_with_many_groups_and_a_long_name() {
    let long = "a group with a very long title that would never fit on one line of sixty";
    let mut groups = vec![group(
        "g_long_id_0001",
        long,
        GroupStatus::Running,
        "active",
        1,
    )];
    groups.extend((0..6).map(|index| {
        group(
            &format!("g_{index}"),
            &format!("other-{index}"),
            GroupStatus::Stopped,
            "stopped",
            0,
        )
    }));
    let mut app = app_with(groups);
    with_members(
        &mut app,
        "g_long_id_0001",
        vec![member(
            "claude-1",
            "Claude Code",
            "peer",
            true,
            MemberState::Ready,
        )],
    );
    press(&mut app, KeyCode::Char('x'));
    let rows = draw_rows(&app, 60, 16);
    assert!(
        rows[2].starts_with(" Stop a group with a very long"),
        "{rows:#?}"
    );
    assert!(rows[2].ends_with("...?"), "{rows:#?}");
    assert!(rows[2].chars().count() <= 60);
    assert!(rows[3].starts_with(" g_long_id_0001"), "{rows:#?}");
    let body = words(&rows[5..13]);
    assert!(
        body.ends_with(
            "Its managed actors stop. Native members stop receiving from this group. \
             Their apps stay open, and work they already received is not interrupted."
        ),
        "{body}"
    );
    assert_eq!(rows[15], " Enter stop   b cancel");
}

#[test]
fn a_running_action_is_shown_and_reported_if_cccc_stops_first() {
    let mut app = app_with(demo());
    press(&mut app, KeyCode::Char('x'));
    let Some(crate::tui::app::Effect::Group(command)) = app.handle(Event::Key(
        KeyEvent::new_with_kind(KeyCode::Enter, KeyModifiers::NONE, KeyEventKind::Press),
    )) else {
        panic!("Enter sends");
    };
    app.set_action(ActionProgress {
        running: Some(command),
        last: None,
    });
    let rows = draw_rows(&app, 80, 16);
    assert_eq!(rows[15], " Stopping cccc-dev...   b back");
    assert_eq!(
        super::unanswered_receipt(&app).as_deref(),
        Some("Stopping cccc-dev had not finished when CCCC stopped; check the group's state.")
    );
}

#[test]
fn a_long_list_keeps_the_selection_in_view_on_the_smallest_screen() {
    let groups = (0..12)
        .map(|index| {
            group(
                &format!("g_{index:02}"),
                &format!("group-{index:02}"),
                GroupStatus::Stopped,
                "stopped",
                0,
            )
        })
        .collect();
    let mut app = app_with(groups);
    for _ in 0..9 {
        press(&mut app, KeyCode::Down);
    }
    let rows = draw_rows(&app, 60, 16);
    let list = &rows[2..7];
    assert!(
        list.iter().any(|row| row.starts_with(" > group-09")),
        "{list:#?}"
    );
    assert_eq!(
        rows[15],
        " Up/Down group   t tasks   s start   x stop   b back"
    );
}

#[test]
fn a_gone_group_is_said_to_be_gone() {
    let mut app = app_with(demo());
    let mut list = GroupList::default();
    list.record(Ok(demo()[1..].to_vec()), Instant::now());
    app.set_groups(list);
    let rows = draw_rows(&app, 80, 16);
    assert!(
        rows[5].contains("The selected group is no longer listed"),
        "{rows:#?}"
    );
}

#[test]
fn the_main_screen_shows_an_action_still_running_and_members_are_not_read_for_a_gone_group() {
    let mut app = app_with(demo());
    press(&mut app, KeyCode::Char('x'));
    let Some(crate::tui::app::Effect::Group(command)) = app.handle(Event::Key(
        KeyEvent::new_with_kind(KeyCode::Enter, KeyModifiers::NONE, KeyEventKind::Press),
    )) else {
        panic!("Enter sends");
    };
    app.set_action(ActionProgress {
        running: Some(command),
        last: None,
    });
    press(&mut app, KeyCode::Char('b'));
    let rows = draw_rows(&app, 80, 16);
    assert_eq!(rows[13], " Stopping cccc-dev...");

    let mut app = app_with(demo());
    assert_eq!(app.want().members.as_deref(), Some("g_dev"));
    let mut list = GroupList::default();
    list.record(Ok(demo()[1..].to_vec()), Instant::now());
    app.set_groups(list);
    assert_eq!(app.want().members, None);
}
