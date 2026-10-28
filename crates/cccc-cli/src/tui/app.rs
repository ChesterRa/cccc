//! What the interface shows and how input changes it. This is plain state with
//! no terminal or channels, so screens and key rules are testable directly.

use super::groups::{
    ActionProgress, GroupAction, GroupCommand, GroupList, GroupSummary, MemberList, Outcome,
};
use super::refresh::Want;
use super::sessions::SessionList;
use super::setup::{InstallState, RUNTIMES, SetupCommand, SetupProgress, pickable};
use super::tasks::{Request as TaskRequest, Selection as TaskSelection, Snapshot as TaskSnapshot};
use super::text;
use crate::commands::sessions::setup::SetupInspection;
use crate::host_state::{HostIntent, HostPhase, HostState};
use cccc_contracts::local_session::{LocalSessionRuntime, LocalSessionSource};
use cccc_core::HomeLayout;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::cell::Cell;
use std::collections::HashSet;
use std::path::PathBuf;

/// Completion candidates, in the order the command line lists them.
pub(super) const COMMANDS: [(&str, &str); 5] = [
    ("/groups", "Browse groups and start or stop one"),
    ("/sessions", "Browse native sessions and their details"),
    ("/setup", "Connect Codex, Claude Code, Pi or OpenCode"),
    ("/help", "Commands and keyboard shortcuts"),
    ("/quit", "Stop services started here and exit"),
];
const INPUT_LIMIT: usize = 200;

/// Facts fixed for the life of the interface.
#[derive(Clone, Debug)]
pub(super) struct Context {
    pub(super) version: &'static str,
    /// CCCC_HOME as shown on screen.
    pub(super) home: String,
    pub(super) user_home: Option<PathBuf>,
    /// False when NO_COLOR asks for no color.
    pub(super) color: bool,
}

impl Context {
    pub(super) fn new(home: &HomeLayout) -> Self {
        let user_home = std::env::home_dir();
        Self {
            version: crate::PRODUCT_VERSION,
            home: text::home_relative(&home.root().to_string_lossy(), user_home.as_deref()),
            user_home,
            color: std::env::var_os("NO_COLOR").is_none_or(|value| value.is_empty()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Screen {
    Main,
    Help,
    Setup,
    Groups,
    Sessions,
    Tasks,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SetupStep {
    Select,
    Confirm,
    Finish,
}

/// One `/setup` visit, as far as the screen is concerned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SetupFlow {
    pub(super) generation: u64,
    pub(super) step: SetupStep,
    pub(super) cursor: usize,
    /// Picked apps, in [`RUNTIMES`] order. Nothing starts picked.
    pub(super) picked: [bool; RUNTIMES.len()],
    /// Sessions already known when the flow started, so the last step can
    /// show only the ones that appeared since. `None` until a list was read.
    pub(super) known: Option<HashSet<String>>,
    pub(super) install_sent: bool,
    /// The confirm and finish steps can be taller than the terminal.
    pub(super) scroll: Scroll,
}

/// A body that can be taller than the screen, moved with the arrow, paging,
/// Home and End keys.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Scroll {
    /// First body line shown.
    pub(super) top: u16,
    /// How far the body can move, as last drawn; only drawing knows the width
    /// that wrapping, and so the body's height, depend on.
    room: Cell<ScrollRoom>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ScrollRoom {
    pub(super) limit: u16,
    pub(super) page: u16,
}

impl SetupFlow {
    fn go(&mut self, step: SetupStep) {
        self.step = step;
        self.scroll = Scroll::default();
    }
}

impl Scroll {
    /// Fit a body of `lines` rows into `height` rows: the first row to show,
    /// and whether there is anything to scroll to.
    pub(super) fn fit(&self, lines: usize, height: u16) -> (u16, bool) {
        let limit = lines.saturating_sub(usize::from(height));
        let room = ScrollRoom {
            limit: u16::try_from(limit).unwrap_or(u16::MAX),
            page: height.saturating_sub(1),
        };
        self.room.set(room);
        (self.top.min(room.limit), limit > 0)
    }

    // `true` when the key was a scroll key.
    pub(super) fn by(&mut self, code: KeyCode) -> bool {
        let room = self.room.get();
        let page = room.page.max(1);
        self.top = match code {
            KeyCode::Up => self.top.saturating_sub(1),
            KeyCode::Down => self.top.saturating_add(1),
            KeyCode::PageUp => self.top.saturating_sub(page),
            KeyCode::PageDown => self.top.saturating_add(page),
            KeyCode::Home => 0,
            KeyCode::End => room.limit,
            _ => return false,
        }
        .min(room.limit);
        true
    }
}

/// `/groups` state kept between visits. The selection is a group ID, so a
/// list that re-sorts or loses a group never moves it onto another group.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct GroupsView {
    pub(super) selected: Option<String>,
    /// The action waiting for Enter, fixed to the group and action asked for.
    pub(super) confirm: Option<GroupCommand>,
    /// Rows the list had when last drawn, for paging.
    pub(super) page: Cell<u16>,
}

/// Read-only source browser. Identity survives list sorting and refreshes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct SessionsView {
    pub(super) selected: Option<String>,
    pub(super) page: Cell<u16>,
    pub(super) details: bool,
    pub(super) scroll: Scroll,
}

/// A task browser belongs to the Group explicitly opened, even if Groups
/// re-sort while the reader is on a task's notes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct TasksView {
    pub(super) group_id: String,
    pub(super) group_name: String,
    pub(super) done: bool,
    pub(super) page: usize,
    pub(super) selected: Option<String>,
    pub(super) details: bool,
    pub(super) scroll: Scroll,
}

impl TasksView {
    pub(super) fn request(&self) -> TaskRequest {
        TaskRequest {
            group_id: self.group_id.clone(),
            selection: if self.details {
                TaskSelection::Detail(self.selected.clone().unwrap_or_default())
            } else {
                TaskSelection::Page {
                    done: self.done,
                    page: self.page,
                }
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Notice {
    Info(String),
    Error(String),
}

/// What handling an input asks the interface thread to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Effect {
    Host(HostIntent),
    Setup(SetupCommand),
    Group(GroupCommand),
    /// Repaint every cell, for when something else wrote over the screen.
    Repaint,
}

pub(super) struct App {
    pub(super) context: Context,
    pub(super) host: HostState,
    pub(super) sessions: SessionList,
    pub(super) groups: GroupList,
    /// Members of the group last asked for; shown only while it is selected.
    pub(super) members: MemberList,
    pub(super) action: ActionProgress,
    pub(super) groups_view: GroupsView,
    pub(super) sessions_view: SessionsView,
    pub(super) tasks: TaskSnapshot,
    pub(super) tasks_view: TasksView,
    /// Help is taller than the smallest screen.
    pub(super) help: Scroll,
    pub(super) setup: SetupProgress,
    pub(super) flow: Option<SetupFlow>,
    pub(super) screen: Screen,
    pub(super) input: String,
    /// UTF-8 byte boundary in input; display uses terminal columns.
    pub(super) input_cursor: usize,
    completion: usize,
    completion_hidden: bool,
    pub(super) notice: Option<Notice>,
    /// Ctrl+C presses passed to the host, which alone decides whether one is
    /// the second press that forces an exit.
    pub(super) interrupts: u32,
    quit_requested: bool,
    generation: u64,
    /// The action sent until its outcome arrives, so a second one cannot
    /// be confirmed in between.
    sent: Option<GroupCommand>,
    serial: u64,
    /// Raised to have the lists read again now.
    nonce: u64,
}

impl App {
    pub(super) fn new(context: Context, host: HostState) -> Self {
        Self {
            context,
            host,
            sessions: SessionList::default(),
            groups: GroupList::default(),
            members: MemberList::default(),
            action: ActionProgress::default(),
            groups_view: GroupsView::default(),
            sessions_view: SessionsView::default(),
            tasks: TaskSnapshot::default(),
            tasks_view: TasksView::default(),
            help: Scroll::default(),
            setup: SetupProgress::default(),
            flow: None,
            screen: Screen::Main,
            input: String::new(),
            input_cursor: 0,
            completion: 0,
            completion_hidden: false,
            notice: None,
            interrupts: 0,
            quit_requested: false,
            generation: 0,
            sent: None,
            serial: 0,
            nonce: 0,
        }
    }

    /// Whether this process is on its way out, whoever asked first.
    pub(super) fn stopping(&self) -> bool {
        self.quit_requested || self.interrupts > 0 || self.host.phase != HostPhase::Running
    }

    /// What to read: only what a screen shows. Setup counts for sessions,
    /// since its last step lists the ones that appear.
    pub(super) fn want(&self) -> Want {
        let shown = |screens: &[Screen]| !self.stopping() && screens.contains(&self.screen);
        Want {
            sessions: shown(&[Screen::Main, Screen::Setup, Screen::Sessions]),
            groups: shown(&[Screen::Main, Screen::Groups]),
            // Not for a selected group that has left the list.
            members: shown(&[Screen::Groups])
                .then(|| self.selected_group().map(|group| group.group_id.clone()))
                .flatten(),
            tasks: self.task_request(),
            nonce: self.nonce,
        }
    }

    fn task_request(&self) -> Option<TaskRequest> {
        if self.stopping() {
            return None;
        }
        match self.screen {
            Screen::Groups if self.groups_view.confirm.is_none() => self
                .selected_group()
                .map(|group| TaskRequest::open(group.group_id.clone())),
            Screen::Tasks => Some(self.tasks_view.request()),
            _ => None,
        }
    }

    pub(super) fn selected_tasks(&self) -> Option<&TaskSnapshot> {
        (self.tasks.request == self.task_request())
            .then_some(&self.tasks)
            .filter(|snapshot| snapshot.request.is_some())
    }

    pub(super) fn set_tasks(&mut self, tasks: TaskSnapshot) {
        self.tasks = tasks;
        if self.screen == Screen::Tasks && !self.tasks_view.details {
            let selection = self
                .selected_tasks()
                .and_then(TaskSnapshot::page)
                .and_then(|page| {
                    let selected = self.tasks_view.selected.as_deref();
                    page.tasks
                        .iter()
                        .find(|task| Some(task.id.as_str()) == selected)
                        .or_else(|| page.tasks.first())
                        .map(|task| task.id.clone())
                });
            if self.selected_tasks().and_then(TaskSnapshot::page).is_some() {
                self.tasks_view.selected = selection;
            }
        }
    }

    pub(super) fn set_groups(&mut self, groups: GroupList) {
        self.groups = groups;
        if self.groups_view.selected.is_none() {
            self.groups_view.selected = self
                .groups
                .groups
                .first()
                .map(|group| group.group_id.clone());
        }
    }

    /// The selected group, unless it has left the list.
    pub(super) fn selected_group(&self) -> Option<&GroupSummary> {
        self.groups.find(self.groups_view.selected.as_deref()?)
    }

    /// The selected group's members, once read for that group.
    pub(super) fn selected_members(&self) -> Option<&MemberList> {
        (self.groups_view.selected.as_deref() == Some(self.members.group_id.as_str()))
            .then_some(&self.members)
    }

    /// Take the action task's progress, and report an action that ended.
    pub(super) fn set_action(&mut self, progress: ActionProgress) {
        let serial = |last: &Option<(GroupCommand, Outcome)>| {
            last.as_ref().map(|(command, _)| command.serial)
        };
        if serial(&progress.last) != serial(&self.action.last)
            && let Some((command, outcome)) = &progress.last
        {
            let name = text::fit(&text::one_line(&command.name), 32);
            let verb = command.action.verb();
            self.notice = Some(match outcome {
                Outcome::Done => Notice::Info(format!("{name}: {verb} done.")),
                Outcome::Failed(error) => Notice::Error(format!(
                    "Could not {verb} {name}: {}",
                    text::one_line(error)
                )),
                Outcome::Unknown(error) => Notice::Error(format!(
                    "{name}: {verb} not confirmed ({}); check its state.",
                    text::one_line(error)
                )),
            });
            if self
                .sent
                .as_ref()
                .is_some_and(|sent| sent.serial == command.serial)
            {
                self.sent = None;
            }
            // Whatever happened, show what the daemon has now.
            self.nonce += 1;
        }
        self.action = progress;
    }

    /// The action on its way to the daemon or waiting for its answer.
    pub(super) fn busy_action(&self) -> Option<&GroupCommand> {
        self.action.running.as_ref().or(self.sent.as_ref())
    }

    pub(super) fn set_sessions(&mut self, sessions: SessionList) {
        if let Some(flow) = &mut self.flow
            && flow.known.is_none()
            && sessions.last_ok.is_some()
        {
            flow.known = Some(source_ids(&sessions));
        }
        self.sessions = sessions;
        if self.sessions_view.selected.is_none() {
            self.sessions_view.selected = self
                .sessions
                .sources
                .first()
                .map(|source| source.source_id.clone());
        }
    }

    /// The current visit's checks, once they are all in.
    pub(super) fn checks(&self) -> Option<&[SetupInspection]> {
        let flow = self.flow.as_ref()?;
        (self.setup.generation == flow.generation
            && !self.setup.checking
            && self.setup.checks.len() == RUNTIMES.len())
        .then_some(self.setup.checks.as_slice())
    }

    /// The current visit's install rows, once the install has started.
    pub(super) fn installs(&self) -> Option<&[(LocalSessionRuntime, InstallState)]> {
        let flow = self.flow.as_ref()?;
        (flow.install_sent
            && self.setup.generation == flow.generation
            && !self.setup.installs.is_empty())
        .then_some(self.setup.installs.as_slice())
    }

    /// From the moment install is asked for until every app has an outcome.
    pub(super) fn install_running(&self) -> bool {
        self.flow.as_ref().is_some_and(|flow| flow.install_sent)
            && self.installs().is_none_or(|_| self.setup.installing())
    }

    /// Sessions that appeared after the flow started.
    pub(super) fn new_sessions(&self) -> Vec<&LocalSessionSource> {
        let Some(known) = self.flow.as_ref().and_then(|flow| flow.known.as_ref()) else {
            return Vec::new();
        };
        self.sessions
            .sources
            .iter()
            .filter(|source| !known.contains(&source.source_id))
            .collect()
    }

    pub(super) fn picked(&self) -> Vec<LocalSessionRuntime> {
        let Some(flow) = &self.flow else {
            return Vec::new();
        };
        RUNTIMES
            .iter()
            .zip(flow.picked)
            .filter_map(|(runtime, picked)| picked.then_some(*runtime))
            .collect()
    }

    pub(super) fn handle(&mut self, event: Event) -> Option<Effect> {
        match event {
            Event::Key(key) => self.key(key),
            Event::Paste(pasted) => {
                if !self.stopping() && self.screen == Screen::Main {
                    self.insert(&text::one_line(&pasted));
                }
                None
            }
            _ => None,
        }
    }

    fn key(&mut self, key: KeyEvent) -> Option<Effect> {
        // Windows reports releases too. Holding a key repeats it, which is
        // fine for typing and moving but must not repeat an action.
        let press = match key.kind {
            KeyEventKind::Press => true,
            KeyEventKind::Repeat => false,
            KeyEventKind::Release => return None,
        };
        if let Some(effect) = self.control_key(key, press) {
            return effect;
        }
        if self.stopping() {
            return None;
        }
        match self.screen {
            Screen::Help => {
                if is_back(key.code) || key.code == KeyCode::Char('q') {
                    self.screen = Screen::Main;
                } else {
                    self.help.by(key.code);
                }
                None
            }
            Screen::Setup => self.setup_key(key.code, press),
            Screen::Groups => self.groups_key(key.code, press),
            Screen::Tasks => self.tasks_key(key.code, press),
            Screen::Sessions => {
                self.sessions_key(key.code, press);
                None
            }
            Screen::Main => self.command_line_key(key, press),
        }
    }

    // `Some` when the key was a control chord, handled or deliberately not.
    fn control_key(&mut self, key: KeyEvent, press: bool) -> Option<Option<Effect>> {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        // AltGr arrives as Control+Alt on Windows and types characters that
        // some layouts need; it is text, not a chord.
        if !control || key.modifiers.contains(KeyModifiers::ALT) {
            return None;
        }
        let KeyCode::Char(letter) = key.code else {
            return Some(None);
        };
        Some(match letter.to_ascii_lowercase() {
            'c' if press => {
                self.interrupts = self.interrupts.saturating_add(1);
                Some(Effect::Host(HostIntent::Interrupt))
            }
            'z' if press => {
                self.notice = Some(Notice::Info(
                    "Suspend is not supported here; use /quit.".into(),
                ));
                None
            }
            'l' if press => Some(Effect::Repaint),
            'u' if !self.stopping() && self.screen == Screen::Main => {
                self.clear_input();
                None
            }
            'a' if self.screen == Screen::Main => {
                self.input_cursor = 0;
                None
            }
            'e' if self.screen == Screen::Main => {
                self.input_cursor = self.input.len();
                None
            }
            _ => None,
        })
    }

    fn command_line_key(&mut self, key: KeyEvent, press: bool) -> Option<Effect> {
        match key.code {
            KeyCode::Enter if press => return self.submit(),
            KeyCode::Char(ch) => self.insert(&ch.to_string()),
            KeyCode::Backspace if self.input_cursor > 0 => {
                let previous = self.input[..self.input_cursor]
                    .char_indices()
                    .next_back()
                    .map_or(0, |(i, _)| i);
                self.input.drain(previous..self.input_cursor);
                self.input_cursor = previous;
                self.edited();
            }
            KeyCode::Delete if self.input_cursor < self.input.len() => {
                let next = self.input_cursor
                    + self.input[self.input_cursor..]
                        .chars()
                        .next()
                        .expect("cursor precedes a character")
                        .len_utf8();
                self.input.drain(self.input_cursor..next);
                self.edited();
            }
            KeyCode::Left => {
                self.input_cursor = self.input[..self.input_cursor]
                    .char_indices()
                    .next_back()
                    .map_or(0, |(i, _)| i)
            }
            KeyCode::Right => {
                if let Some(ch) = self.input[self.input_cursor..].chars().next() {
                    self.input_cursor += ch.len_utf8();
                }
            }
            KeyCode::Home => self.input_cursor = 0,
            KeyCode::End => self.input_cursor = self.input.len(),
            KeyCode::Up | KeyCode::Down if self.input.is_empty() => self.move_selection(key.code),
            KeyCode::Up | KeyCode::Down => {
                let count = self.candidates().len();
                if count > 0 {
                    self.completion = if key.code == KeyCode::Up {
                        (self.completion + count - 1) % count
                    } else {
                        (self.completion + 1) % count
                    };
                }
            }
            KeyCode::Esc => {
                if !self.candidates().is_empty() {
                    self.completion_hidden = true;
                } else {
                    self.clear_input();
                }
                self.notice = None;
            }
            KeyCode::Tab => self.complete(),
            _ => {}
        }
        None
    }

    fn edited(&mut self) {
        self.completion = 0;
        self.completion_hidden = false;
    }

    fn clear_input(&mut self) {
        self.input.clear();
        self.input_cursor = 0;
        self.edited();
    }

    fn insert(&mut self, typed: &str) {
        let room = INPUT_LIMIT.saturating_sub(self.input.chars().count());
        let typed: String = text::one_line(typed).chars().take(room).collect();
        self.input.insert_str(self.input_cursor, &typed);
        self.input_cursor += typed.len();
        self.edited();
    }

    /// Only a single slash token can be completed. In particular a pasted
    /// multi-command string must never execute its first command.
    pub(super) fn candidates(&self) -> Vec<usize> {
        if self.completion_hidden
            || !self.input.starts_with('/')
            || self.input.chars().any(char::is_whitespace)
        {
            return Vec::new();
        }
        let prefix = self.input.to_ascii_lowercase();
        COMMANDS
            .iter()
            .enumerate()
            .filter_map(|(index, (name, _))| name.starts_with(&prefix).then_some(index))
            .collect()
    }

    pub(super) fn completion_index(&self) -> usize {
        self.completion
            .min(self.candidates().len().saturating_sub(1))
    }

    fn complete(&mut self) {
        // Tab may reopen a dismissed menu, but never starts work.
        self.completion_hidden = false;
        if let Some(index) = self.candidates().get(self.completion_index()) {
            self.input = COMMANDS[*index].0.to_owned();
            self.input_cursor = self.input.len();
            self.completion = 0;
        }
    }

    fn submit(&mut self) -> Option<Effect> {
        if self.input.is_empty() {
            self.notice = None;
            if self.selected_group().is_some() {
                self.open_groups();
            } else if self.groups_view.selected.is_some() {
                self.notice = Some(Notice::Info(
                    "The selected group is no longer listed. Choose another with Up/Down.".into(),
                ));
            }
            return None;
        }
        let command = self
            .candidates()
            .get(self.completion_index())
            .map(|index| COMMANDS[*index].0.to_owned())
            .unwrap_or_else(|| self.input.trim().to_ascii_lowercase());
        self.clear_input();
        self.notice = None;
        match command.as_str() {
            "/quit" | "/exit" => {
                self.quit_requested = true;
                Some(Effect::Host(HostIntent::Quit))
            }
            "/help" => {
                self.help = Scroll::default();
                self.screen = Screen::Help;
                None
            }
            "/setup" => Some(self.open_setup()),
            "/groups" => {
                self.open_groups();
                None
            }
            "/sessions" => {
                if self.selected_source().is_none() {
                    self.sessions_view.selected = self
                        .sessions
                        .sources
                        .first()
                        .map(|source| source.source_id.clone());
                }
                self.sessions_view.details = false;
                self.screen = Screen::Sessions;
                None
            }
            other if other.starts_with('/') => {
                self.notice = Some(Notice::Error(format!(
                    "Unknown command {}. Try /help.",
                    text::fit(other, 24)
                )));
                None
            }
            _ => {
                self.notice = Some(Notice::Error("Commands start with /. Try /help.".into()));
                None
            }
        }
    }

    pub(super) fn selected_source(&self) -> Option<&LocalSessionSource> {
        let selected = self.sessions_view.selected.as_deref()?;
        self.sessions
            .sources
            .iter()
            .find(|source| source.source_id == selected)
    }

    fn sessions_key(&mut self, code: KeyCode, press: bool) {
        if is_back(code) {
            if self.sessions_view.details {
                self.sessions_view.details = false;
            } else {
                self.screen = Screen::Main;
            }
            return;
        }
        let code = match code {
            KeyCode::Char('j') => KeyCode::Down,
            KeyCode::Char('k') => KeyCode::Up,
            other => other,
        };
        if self.sessions_view.details {
            self.sessions_view.scroll.by(code);
            return;
        }
        if code == KeyCode::Enter && press && self.selected_source().is_some() {
            self.sessions_view.details = true;
            self.sessions_view.scroll = Scroll::default();
            return;
        }
        let count = self.sessions.sources.len();
        if count == 0 {
            return;
        }
        let current = self
            .sessions
            .sources
            .iter()
            .position(|source| Some(&source.source_id) == self.sessions_view.selected.as_ref());
        let page = usize::from(self.sessions_view.page.get().max(1));
        let Some(next) = selection_index(code, current, count, page) else {
            return;
        };
        self.sessions_view.selected = Some(self.sessions.sources[next].source_id.clone());
    }

    fn open_setup(&mut self) -> Effect {
        self.generation += 1;
        self.flow = Some(SetupFlow {
            generation: self.generation,
            step: SetupStep::Select,
            cursor: 0,
            picked: [false; RUNTIMES.len()],
            known: self
                .sessions
                .last_ok
                .is_some()
                .then(|| source_ids(&self.sessions)),
            install_sent: false,
            scroll: Scroll::default(),
        });
        self.screen = Screen::Setup;
        Effect::Setup(SetupCommand::Check {
            generation: self.generation,
        })
    }

    fn setup_key(&mut self, code: KeyCode, press: bool) -> Option<Effect> {
        let step = self.flow.as_ref()?.step;
        if step != SetupStep::Select
            && let Some(flow) = &mut self.flow
            && flow.scroll.by(code)
        {
            return None;
        }
        match step {
            SetupStep::Select => self.select_key(code, press),
            SetupStep::Confirm => {
                if code == KeyCode::Enter && press {
                    let runtimes = self.picked();
                    let flow = self.flow.as_mut()?;
                    flow.go(SetupStep::Finish);
                    flow.install_sent = true;
                    return Some(Effect::Setup(SetupCommand::Install {
                        generation: flow.generation,
                        runtimes,
                    }));
                }
                if is_back(code)
                    && let Some(flow) = &mut self.flow
                {
                    flow.go(SetupStep::Select);
                }
                None
            }
            SetupStep::Finish => {
                if self.install_running() {
                    // The only way out of a running install is to cancel it.
                    return (is_back(code) && press).then_some(Effect::Setup(SetupCommand::Cancel));
                }
                if (is_back(code) || code == KeyCode::Enter) && press {
                    self.close_setup();
                }
                None
            }
        }
    }

    fn select_key(&mut self, code: KeyCode, press: bool) -> Option<Effect> {
        if is_back(code) {
            self.close_setup();
            // Ends a check still running; nothing else can be running here.
            return Some(Effect::Setup(SetupCommand::Cancel));
        }
        let selectable: Vec<bool> = match self.checks() {
            Some(checks) => checks.iter().map(pickable).collect(),
            None => return None,
        };
        let flow = self.flow.as_mut()?;
        match code {
            KeyCode::Up => flow.cursor = flow.cursor.saturating_sub(1),
            KeyCode::Down => flow.cursor = (flow.cursor + 1).min(RUNTIMES.len() - 1),
            KeyCode::Char(' ') if press && selectable[flow.cursor] => {
                flow.picked[flow.cursor] = !flow.picked[flow.cursor];
                self.notice = None;
            }
            KeyCode::Enter if press => {
                if flow.picked.contains(&true) {
                    flow.go(SetupStep::Confirm);
                    self.notice = None;
                } else {
                    self.notice = Some(Notice::Info("Pick at least one app with Space.".into()));
                }
            }
            _ => {}
        }
        None
    }

    fn open_groups(&mut self) {
        // A group that left the list while this screen was closed is not
        // worth landing on; one that leaves while it is open stays selected,
        // shown as gone, so no action can fall to its neighbour.
        if self.selected_group().is_none() {
            self.groups_view.selected = self
                .groups
                .groups
                .first()
                .map(|group| group.group_id.clone());
        }
        self.groups_view.confirm = None;
        self.screen = Screen::Groups;
    }

    fn groups_key(&mut self, code: KeyCode, press: bool) -> Option<Effect> {
        if let Some(confirm) = self.groups_view.confirm.clone() {
            if code == KeyCode::Enter && press {
                self.groups_view.confirm = None;
                return self.send(confirm);
            }
            if is_back(code) {
                self.groups_view.confirm = None;
            }
            return None;
        }
        match code {
            code if is_back(code) => self.screen = Screen::Main,
            KeyCode::Char('s') if press => self.ask(false),
            KeyCode::Char('x') if press => self.ask(true),
            KeyCode::Char('t') if press => self.open_tasks(),
            KeyCode::Char('j') => self.move_selection(KeyCode::Down),
            KeyCode::Char('k') => self.move_selection(KeyCode::Up),
            code => self.move_selection(code),
        }
        None
    }

    fn open_tasks(&mut self) {
        let Some(group) = self.selected_group() else {
            self.notice = Some(Notice::Info(
                "Choose a listed Group before reading its tasks.".into(),
            ));
            return;
        };
        self.tasks_view = TasksView {
            group_id: group.group_id.clone(),
            group_name: group.name().to_owned(),
            ..TasksView::default()
        };
        self.screen = Screen::Tasks;
        self.notice = None;
        // The summary may already have this exact page.
        self.set_tasks(self.tasks.clone());
    }

    fn tasks_key(&mut self, code: KeyCode, press: bool) -> Option<Effect> {
        if self.tasks_view.details {
            if is_back(code) && press {
                self.tasks_view.details = false;
                self.tasks_view.scroll = Scroll::default();
            } else {
                self.tasks_view.scroll.by(code);
            }
            return None;
        }
        if is_back(code) && press {
            self.screen = Screen::Groups;
            self.notice = None;
            return None;
        }
        if code == KeyCode::Char('d') && press {
            self.tasks_view.done = !self.tasks_view.done;
            self.tasks_view.page = 0;
            self.tasks_view.selected = None;
            return None;
        }
        if code == KeyCode::PageUp && press && self.tasks_view.page > 0 {
            self.tasks_view.page -= 1;
            self.tasks_view.selected = None;
            return None;
        }
        let page = self.selected_tasks().and_then(TaskSnapshot::page)?;
        if code == KeyCode::PageDown && press && page.has_more {
            self.tasks_view.page = self.tasks_view.page.saturating_add(1);
            self.tasks_view.selected = None;
            return None;
        }
        let selected = page
            .tasks
            .iter()
            .position(|task| Some(task.id.as_str()) == self.tasks_view.selected.as_deref());
        if code == KeyCode::Enter && press && selected.is_some() {
            self.tasks_view.details = true;
            self.tasks_view.scroll = Scroll::default();
            return None;
        }
        let len = page.tasks.len();
        if len == 0 {
            return None;
        }
        let index = selected.unwrap_or(0);
        let index = match code {
            KeyCode::Up | KeyCode::Char('k') => index.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => index.saturating_add(1).min(len - 1),
            KeyCode::Home => 0,
            KeyCode::End => len - 1,
            _ => return None,
        };
        self.tasks_view.selected = Some(page.tasks[index].id.clone());
        None
    }

    // Opens the confirmation for the selected group; nothing is sent yet.
    fn ask(&mut self, stop: bool) {
        if let Some(busy) = self.busy_action() {
            self.notice = Some(Notice::Info(format!(
                "Wait for {} {} to finish.",
                busy.action.doing().to_lowercase(),
                text::fit(&text::one_line(&busy.name), 32)
            )));
            return;
        }
        let Some(group) = self.selected_group() else {
            let reason = if self.groups_view.selected.is_some() {
                "The selected group is no longer listed."
            } else if self.groups.last_ok.is_none() {
                "Groups are still being read."
            } else {
                "There is no group to start or stop."
            };
            self.notice = Some(Notice::Error(reason.into()));
            return;
        };
        let action = if stop {
            GroupAction::Stop
        } else {
            GroupAction::Start {
                resume: group.state == "paused",
            }
        };
        let command = GroupCommand {
            serial: self.serial + 1,
            group_id: group.group_id.clone(),
            name: group.name().to_owned(),
            action,
        };
        self.serial = command.serial;
        self.groups_view.confirm = Some(command);
        self.notice = None;
    }

    fn send(&mut self, command: GroupCommand) -> Option<Effect> {
        if self.busy_action().is_some() {
            return None;
        }
        if self.groups.find(&command.group_id).is_none() {
            self.notice = Some(Notice::Error(format!(
                "{} is no longer listed; nothing was sent.",
                text::fit(&text::one_line(&command.name), 32)
            )));
            return None;
        }
        self.notice = None;
        self.sent = Some(command.clone());
        Some(Effect::Group(command))
    }

    fn move_selection(&mut self, code: KeyCode) {
        let count = self.groups.groups.len();
        if count == 0 {
            return;
        }
        let page = usize::from(self.groups_view.page.get().max(1));
        let current = self
            .groups_view
            .selected
            .as_deref()
            .and_then(|group_id| self.groups.position(group_id));
        let Some(next) = selection_index(code, current, count, page) else {
            return;
        };
        self.groups_view.selected = Some(self.groups.groups[next].group_id.clone());
    }

    fn close_setup(&mut self) {
        self.flow = None;
        self.screen = Screen::Main;
        self.notice = None;
    }
}

// Both directories navigate by stable identity, with the same paging rules.
fn selection_index(
    code: KeyCode,
    current: Option<usize>,
    count: usize,
    page: usize,
) -> Option<usize> {
    if count == 0 {
        return None;
    }
    Some(match (code, current) {
        (KeyCode::Home, _) => 0,
        (KeyCode::End, _) => count - 1,
        (KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown, None) => 0,
        (KeyCode::Up, Some(index)) => index.saturating_sub(1),
        (KeyCode::Down, Some(index)) => (index + 1).min(count - 1),
        (KeyCode::PageUp, Some(index)) => index.saturating_sub(page),
        (KeyCode::PageDown, Some(index)) => (index + page).min(count - 1),
        _ => return None,
    })
}

// `b` is the way back everywhere; Esc also works where the terminal delivers
// it promptly.
fn is_back(code: KeyCode) -> bool {
    matches!(code, KeyCode::Char('b') | KeyCode::Esc)
}

fn source_ids(sessions: &SessionList) -> HashSet<String> {
    sessions
        .sources
        .iter()
        .map(|source| source.source_id.clone())
        .collect()
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
