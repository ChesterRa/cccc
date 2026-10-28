use super::*;
use crate::tui::app::{App, Screen};
use crate::tui::groups::{GroupList, GroupStatus, GroupSummary};
use crate::tui::test_support::{context, running};
use crossterm::event::{Event, KeyCode, KeyEvent};
use ratatui::{Terminal, backend::TestBackend};

fn task(id: &str, status: &str) -> Value {
    json!({"id":id,"title":format!("Task {id} 日本語"),"status":status,"assignee":"reviewer",
        "waiting_on":"user","blocked_by":["T002"],"handoff_to":"peer",
        "checklist":[{"text":"first","status":"done"},{"text":"second","status":"in_progress"}],
        "notes":format!("{}\nLAST_NOTES_LINE", "長い説明 emoji 🧪\n".repeat(90)),
        "outcome":"FINAL_OUTCOME"})
}

fn page() -> Page {
    parse_page(json!({"pages":{
        "planned":{"tasks":[task("T002","planned")],"has_more":false},
        "active":{"tasks":[task("T001","active")],"has_more":true}},
        "facets":{"status_counts":{"active":21,"planned":1,"done":4},"waiting_user":1,"blocked":2,"pending_handoffs":1}
    }).as_object().expect("response"), "active,planned").expect("page")
}

fn press(app: &mut App, code: KeyCode) {
    assert!(
        app.handle(Event::Key(KeyEvent::from(code))).is_none(),
        "task browsing must not emit a write or host intent"
    );
}

fn open() -> App {
    let mut app = App::new(context(), running());
    let mut groups = GroupList::default();
    groups.record(
        Ok(vec![GroupSummary {
            group_id: "g_tasks".into(),
            title: "Work".into(),
            status: GroupStatus::Offline,
            state: "active".into(),
            running_actors: 0,
        }]),
        Instant::now(),
    );
    app.set_groups(groups);
    assert!(
        app.want().tasks.is_none(),
        "main screen must not poll tasks"
    );
    app.input = "/groups".into();
    press(&mut app, KeyCode::Enter);
    let request = app.want().tasks.expect("selected Group tasks");
    let mut snapshot = Snapshot::default();
    snapshot.record(&request, Ok(Data::Page(page())), Instant::now());
    app.set_tasks(snapshot);
    press(&mut app, KeyCode::Char('t'));
    assert_eq!(app.screen, Screen::Tasks);
    app
}

fn rows(app: &App, width: u16, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    terminal
        .draw(|frame| crate::tui::view::render(frame, app, Instant::now()))
        .expect("render");
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect()
}

#[test]
fn atomic_pages_keep_active_before_planned_and_expose_more_pages() {
    let page = page();
    assert_eq!(
        page.tasks
            .iter()
            .map(|task| task.id.as_str())
            .collect::<Vec<_>>(),
        ["T001", "T002"]
    );
    assert_eq!(page.counts.done, 4);
    assert!(page.has_more);
    assert!(
        parse_page(
            json!({"pages":{},"facets":{}})
                .as_object()
                .expect("response"),
            "active,planned"
        )
        .is_err()
    );
}

#[test]
fn failed_reads_keep_only_the_same_request_and_do_not_cross_group_or_task() {
    let at = Instant::now();
    let mut snapshot = Snapshot::default();
    let first = Request::open("g_tasks".into());
    snapshot.record(&first, Ok(Data::Page(page())), at);
    snapshot.record(&first, Err("offline".into()), at);
    assert!(snapshot.page().is_some());
    assert_eq!(snapshot.last_ok, Some(at));
    for request in [
        Request::open("g_other".into()),
        Request {
            group_id: "g_tasks".into(),
            selection: Selection::Detail("T001".into()),
        },
    ] {
        snapshot.record(&request, Err("offline".into()), at);
        assert!(snapshot.data.is_none());
        assert!(snapshot.last_ok.is_none());
    }
}

#[test]
fn browsing_pages_done_and_details_is_read_only_and_fences_late_answers() {
    let mut app = open();
    let original = app.want().tasks.expect("open request");
    press(&mut app, KeyCode::PageDown);
    assert_eq!(app.tasks_view.page, 1);
    assert!(
        app.selected_tasks().is_none(),
        "old page must not fill the new page"
    );
    press(&mut app, KeyCode::PageDown);
    assert_eq!(app.tasks_view.page, 1, "cannot skip unread pages");
    press(&mut app, KeyCode::Char('d'));
    assert!(app.tasks_view.done);
    assert_eq!(app.tasks_view.page, 0);
    assert!(app.selected_tasks().is_none());
    let mut late = Snapshot::default();
    late.record(&original, Ok(Data::Page(page())), Instant::now());
    app.set_tasks(late);
    assert!(
        app.selected_tasks().is_none(),
        "late Open results cannot appear as Done"
    );
    let request = app.want().tasks.expect("Done request");
    let mut ready = Snapshot::default();
    ready.record(
        &request,
        Ok(Data::Page(Page {
            tasks: vec![Task::parse(&task("T009", "done")).expect("task")],
            counts: Counts::default(),
            has_more: false,
        })),
        Instant::now(),
    );
    app.set_tasks(ready);
    press(&mut app, KeyCode::Enter);
    assert_eq!(
        app.want().tasks.expect("detail request").selection,
        Selection::Detail("T009".into())
    );
    assert!(app.selected_tasks().is_none());
    press(&mut app, KeyCode::Char('b'));
    assert!(!app.tasks_view.details);
    press(&mut app, KeyCode::Char('b'));
    assert_eq!(app.screen, Screen::Groups);
    press(&mut app, KeyCode::Char('b'));
    assert_eq!(app.screen, Screen::Main);
    assert!(app.want().tasks.is_none());
}

#[test]
fn minimum_screen_has_one_focus_and_complete_scrolling_details() {
    let mut app = open();
    let shown = rows(&app, 60, 16).join("\n");
    assert!(shown.contains("Active 21  Planned 1  Done 4"));
    assert!(shown.contains("Waiting user 1"));
    assert!(shown.contains("Enter details"));
    assert!(shown.contains("T001"));
    press(&mut app, KeyCode::Enter);
    let request = app.want().tasks.expect("exact read");
    let mut snapshot = Snapshot::default();
    snapshot.record(
        &request,
        Ok(Data::Detail(
            Task::parse(&task("T001", "active")).expect("task"),
        )),
        Instant::now(),
    );
    app.set_tasks(snapshot);
    let top = rows(&app, 60, 16).join("\n");
    assert!(top.contains("Task T001"));
    assert!(top.contains("Group g_tasks"));
    assert!(
        top.lines()
            .any(|line| line.starts_with(" Status") && line.contains("Active"))
    );
    press(&mut app, KeyCode::End);
    let last = rows(&app, 60, 16).join("\n");
    assert!(last.contains("LAST_NOTES_LINE"));
    assert!(last.contains("FINAL_OUTCOME"));
    assert!(last.contains("b tasks"));
    for forbidden in [KeyCode::Char('s'), KeyCode::Char('x'), KeyCode::Char('d')] {
        press(&mut app, forbidden);
        assert!(app.tasks_view.details);
    }
    assert_eq!(app.tasks_view.selected.as_deref(), Some("T001"));
}
