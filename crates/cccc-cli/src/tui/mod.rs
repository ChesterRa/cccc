//! Terminal interface for a bare `cccc` launch in an interactive terminal.
//!
//! The interface is the foreground host's face, not a separate client: it
//! shows what this process runs, and quitting it stops what this process
//! started. One plain OS thread owns the terminal -- modes, input and drawing
//! -- so a console write that blocks (Windows console hosts pause output while
//! text is selected) cannot stall the Tokio workers serving Web and the
//! embedded daemon. The daemon work the screen needs runs as Tokio tasks.

mod app;
mod daemon;
mod groups;
mod groups_view;
mod refresh;
mod sessions;
mod sessions_view;
mod setup;
mod setup_view;
mod tasks;
mod tasks_view;
mod terminal;
#[cfg(test)]
mod test_support;
mod text;
mod view;

use crate::host_state::{HostIntent, HostPhase, HostState};
use anyhow::{Result, anyhow};
use app::{App, Context, Effect};
use cccc_core::HomeLayout;
use crossterm::event;
use groups::{ActionProgress, GroupCommand, GroupList, MemberList};
use refresh::{Lists, Want};
use sessions::SessionList;
use setup::{SetupCommand, SetupProgress};
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;

pub(crate) use terminal::restore as restore_terminal;

const THREAD_NAME: &str = "cccc-tui";
// Input wait per loop; also the longest a host change waits to be drawn.
const TICK: Duration = Duration::from_millis(100);
// Entering the interface is a few mode changes and one write; only a console
// that has stopped accepting output takes longer.
const START_TIMEOUT: Duration = Duration::from_secs(5);
// Interrupting an install drops its provider command, whose process tree the
// runtime capture ends; the bound only covers a task that fails to notice.
const SETUP_STOP_TIMEOUT: Duration = Duration::from_secs(5);
// The thread sees the request within a tick; the rest is restoring the
// terminal and printing the receipt, which only a stalled console delays.
const FINISH_TIMEOUT: Duration = Duration::from_secs(3);
// Restoring from a host path that cannot wait for the interface thread.
const RESTORE_TIMEOUT: Duration = Duration::from_secs(1);
const AGE_REFRESH: Duration = Duration::from_secs(1);

/// The open interface. Stop its daemon work with [`Session::stop_work`] when
/// the host starts stopping, and close it with [`Session::finish`].
pub(crate) struct Session {
    finish: Arc<AtomicBool>,
    done: oneshot::Receiver<()>,
    stop: watch::Sender<bool>,
    refresh: Option<JoinHandle<()>>,
    setup: Option<JoinHandle<()>>,
    act: Option<JoinHandle<()>>,
}

/// Take over the terminal and show the interface. `host` carries this
/// process's lifecycle; `intents` carries the user's stop requests back. An
/// error means the terminal is back in its own modes, or will be as soon as
/// a stalled console lets the interface thread continue.
pub(crate) async fn start(
    home: HomeLayout,
    host: watch::Receiver<HostState>,
    intents: mpsc::UnboundedSender<HostIntent>,
) -> Result<Session> {
    let (sessions_tx, sessions) = watch::channel(SessionList::default());
    let (groups_tx, groups) = watch::channel(GroupList::default());
    let (members_tx, members) = watch::channel(MemberList::default());
    let (tasks_tx, tasks) = watch::channel(tasks::Snapshot::default());
    let (want, want_rx) = watch::channel(Want::default());
    let (setup_commands, setup_commands_rx) = mpsc::unbounded_channel();
    let (progress_tx, progress) = watch::channel(SetupProgress::default());
    let (group_commands, group_commands_rx) = mpsc::unbounded_channel();
    let (action_tx, action) = watch::channel(ActionProgress::default());
    let (stop, stop_rx) = watch::channel(false);
    let finish = Arc::new(AtomicBool::new(false));
    let (entered_tx, entered) = oneshot::channel();
    let (done_tx, done) = oneshot::channel();
    let initial = host.borrow().clone();
    let ui = Ui {
        app: App::new(Context::new(&home), initial),
        host,
        intents,
        sessions,
        groups,
        members,
        tasks,
        want,
        setup_commands,
        progress,
        group_commands,
        action,
        finish: Arc::clone(&finish),
    };
    std::thread::Builder::new()
        .name(THREAD_NAME.into())
        .spawn(move || {
            match terminal::enter() {
                Ok(screen) => {
                    let _ = entered_tx.send(Ok(()));
                    ui.run(screen);
                }
                Err(error) => {
                    let _ = entered_tx.send(Err(error));
                }
            }
            let _ = done_tx.send(());
        })
        .map_err(|error| anyhow!("could not start the terminal interface: {error}"))?;
    let failure = match tokio::time::timeout(START_TIMEOUT, entered).await {
        Ok(Ok(Ok(()))) => None,
        Ok(Ok(Err(error))) => Some(anyhow!("could not open the terminal interface: {error}")),
        Ok(Err(_)) => Some(anyhow!("the terminal interface stopped while opening")),
        Err(_) => Some(anyhow!(
            "the terminal interface did not open within {} seconds",
            START_TIMEOUT.as_secs()
        )),
    };
    if let Some(failure) = failure {
        // A thread that opens late sees this and closes straight away; the
        // restore covers a host that exits before that thread gets a turn.
        finish.store(true, Ordering::SeqCst);
        let _ = terminal::restore_within(RESTORE_TIMEOUT);
        return Err(failure);
    }
    let refresh = tokio::spawn(refresh::run(
        home.clone(),
        want_rx,
        Lists {
            sessions: sessions_tx,
            groups: groups_tx,
            members: members_tx,
            tasks: tasks_tx,
        },
    ));
    let act = tokio::spawn(groups::act(
        home.clone(),
        group_commands_rx,
        action_tx,
        stop_rx.clone(),
    ));
    let setup = tokio::spawn(setup::run(home, setup_commands_rx, progress_tx, stop_rx));
    Ok(Session {
        finish,
        done,
        stop,
        refresh: Some(refresh),
        setup: Some(setup),
        act: Some(act),
    })
}

impl Session {
    /// Stop the daemon work the interface started and wait for it to end.
    /// Call this when stopping begins, before the daemon goes away, so that
    /// no install outlives it; an interrupted install is recorded as such.
    /// Later calls do nothing.
    pub(crate) async fn stop_work(&mut self) {
        let _ = self.stop.send(true);
        // Reads have nothing to finish, and a group action only stops being
        // waited for: a request already sent is the daemon's to complete.
        for task in [self.refresh.take(), self.act.take()].into_iter().flatten() {
            task.abort();
            let _ = task.await;
        }
        if let Some(mut setup) = self.setup.take()
            && tokio::time::timeout(SETUP_STOP_TIMEOUT, &mut setup)
                .await
                .is_err()
        {
            setup.abort();
            let _ = setup.await;
        }
    }

    /// Close the interface after the host has published its final state:
    /// the interface thread restores the terminal and prints the receipt,
    /// and this waits for that within a bound.
    pub(crate) async fn finish(mut self) -> Result<()> {
        self.stop_work().await;
        self.finish.store(true, Ordering::SeqCst);
        let result = match tokio::time::timeout(FINISH_TIMEOUT, &mut self.done).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(_)) => Err(anyhow!("the terminal interface stopped unexpectedly")),
            Err(_) => Err(anyhow!(
                "the terminal interface did not close within {} seconds; the terminal may need a reset",
                FINISH_TIMEOUT.as_secs()
            )),
        };
        if result.is_err() {
            // The flag requests closure; it does not prove that the drawing
            // thread reached restoration. A blocked console must not leave
            // input in raw mode just because Drop sees that request flag.
            let _ = terminal::restore_within(RESTORE_TIMEOUT);
        }
        result
    }
}

// A session dropped without `finish` is an error path that may be about to
// end the process before the interface thread gets another turn, so the
// terminal is put back here, within a bound.
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
        for task in [self.refresh.take(), self.setup.take(), self.act.take()]
            .into_iter()
            .flatten()
        {
            task.abort();
        }
        if !self.finish.swap(true, Ordering::SeqCst) {
            let _ = terminal::restore_within(RESTORE_TIMEOUT);
        }
    }
}

struct Ui {
    app: App,
    host: watch::Receiver<HostState>,
    intents: mpsc::UnboundedSender<HostIntent>,
    sessions: watch::Receiver<SessionList>,
    groups: watch::Receiver<GroupList>,
    members: watch::Receiver<MemberList>,
    tasks: watch::Receiver<tasks::Snapshot>,
    want: watch::Sender<Want>,
    setup_commands: mpsc::UnboundedSender<SetupCommand>,
    progress: watch::Receiver<SetupProgress>,
    group_commands: mpsc::UnboundedSender<GroupCommand>,
    action: watch::Receiver<ActionProgress>,
    finish: Arc<AtomicBool>,
}

impl Ui {
    fn run(mut self, mut screen: terminal::Screen) {
        let _stop_host = StopHostOnPanic(self.intents.clone());
        let mut usable = true;
        let mut dirty = true;
        let mut last_draw = Instant::now();
        loop {
            match self.host.has_changed() {
                Ok(true) => {
                    self.app.host = self.host.borrow_and_update().clone();
                    dirty = true;
                }
                Ok(false) => {}
                Err(_) => break,
            }
            dirty |= self.take_work_updates();
            if self.finish.load(Ordering::SeqCst) {
                // The host publishes its final state before asking to finish.
                self.app.host = self.host.borrow().clone();
                break;
            }
            if matches!(self.app.host.phase, HostPhase::Stopped { .. }) {
                break;
            }
            let wanted = self.app.want();
            self.want.send_if_modified(|want| {
                let changed = *want != wanted;
                *want = wanted;
                changed
            });
            if view::shows_age(&self.app) && last_draw.elapsed() >= AGE_REFRESH {
                dirty = true;
            }
            if usable && dirty {
                dirty = false;
                last_draw = Instant::now();
                if !self.draw(&mut screen) {
                    usable = false;
                    self.lose_terminal();
                }
            }
            if !usable {
                std::thread::sleep(TICK);
                continue;
            }
            match self.read_input(&mut screen) {
                Ok(changed) => dirty |= changed,
                Err(_) => {
                    usable = false;
                    self.lose_terminal();
                }
            }
        }
        // The receipt reports how an interrupted install or group action ended.
        self.take_work_updates();
        let restored = terminal::restore();
        self.print_receipt(restored);
    }

    fn take_work_updates(&mut self) -> bool {
        let mut changed = false;
        if self.sessions.has_changed().unwrap_or(false) {
            let list = self.sessions.borrow_and_update().clone();
            self.app.set_sessions(list);
            changed = true;
        }
        if self.groups.has_changed().unwrap_or(false) {
            let list = self.groups.borrow_and_update().clone();
            self.app.set_groups(list);
            changed = true;
        }
        if self.members.has_changed().unwrap_or(false) {
            self.app.members = self.members.borrow_and_update().clone();
            changed = true;
        }
        if self.tasks.has_changed().unwrap_or(false) {
            let tasks = self.tasks.borrow_and_update().clone();
            self.app.set_tasks(tasks);
            changed = true;
        }
        if self.progress.has_changed().unwrap_or(false) {
            self.app.setup = self.progress.borrow_and_update().clone();
            changed = true;
        }
        if self.action.has_changed().unwrap_or(false) {
            let progress = self.action.borrow_and_update().clone();
            self.app.set_action(progress);
            changed = true;
        }
        changed
    }

    fn draw(&self, screen: &mut terminal::Screen) -> bool {
        // Another path may have restored the terminal already; drawing now
        // would write the interface into the user's normal screen.
        if !terminal::is_entered() {
            return true;
        }
        screen
            .draw(|frame| view::render(frame, &self.app, Instant::now()))
            .is_ok()
    }

    // Wait up to a tick for input, then take whatever else is already queued
    // so one burst costs one draw.
    fn read_input(&mut self, screen: &mut terminal::Screen) -> std::io::Result<bool> {
        if !event::poll(TICK)? {
            return Ok(false);
        }
        loop {
            let input = event::read()?;
            // Below the minimum size no confirmation is visible. Keep the
            // global exit key, but never accept an unseen setup/group action.
            // size() uses OS APIs, not a terminal protocol query.
            let size = screen.size()?;
            let painted = screen.get_frame().area();
            let visible = size.width >= view::MIN_WIDTH
                && size.height >= view::MIN_HEIGHT
                && painted.width == size.width
                && painted.height == size.height;
            let allowed = accepts_input(&input, visible);
            match allowed.then(|| self.app.handle(input)).flatten() {
                Some(Effect::Host(intent)) => {
                    let _ = self.intents.send(intent);
                }
                Some(Effect::Setup(command)) => {
                    let _ = self.setup_commands.send(command);
                }
                Some(Effect::Group(command)) => {
                    let _ = self.group_commands.send(command);
                }
                Some(Effect::Repaint) => repaint(screen)?,
                None => {}
            }
            if !event::poll(Duration::ZERO)? {
                return Ok(true);
            }
        }
    }

    // Without a terminal there is no way left to quit, so losing it is an
    // ordinary stop request; the loop keeps waiting for the host's report.
    fn lose_terminal(&self) {
        let _ = self.intents.send(HostIntent::Quit);
    }

    fn print_receipt(&self, restored: std::io::Result<()>) {
        let mut out = std::io::stdout().lock();
        if let HostPhase::Stopped { report } = &self.app.host.phase {
            let _ = writeln!(out, "{}", view::receipt(report));
        }
        if let Some(interrupted) = setup_view::interrupted_receipt(&self.app) {
            let _ = writeln!(out, "{interrupted}");
        }
        if let Some(unanswered) = groups_view::unanswered_receipt(&self.app) {
            let _ = writeln!(out, "{unanswered}");
        }
        for panic in terminal::take_deferred_panics() {
            let _ = writeln!(
                out,
                "\nA background thread panicked while the interface was open:\n{panic}"
            );
        }
        if let Err(error) = restored {
            let _ = writeln!(
                out,
                "The terminal may not be fully restored ({error}); reset it if it misbehaves."
            );
        }
        let _ = out.flush();
    }
}

fn accepts_input(input: &event::Event, visible: bool) -> bool {
    if visible {
        return true;
    }
    matches!(input, event::Event::Key(key)
        if matches!(key.code, event::KeyCode::Char('c' | 'C'))
        && key.modifiers.contains(event::KeyModifiers::CONTROL)
        && !key.modifiers.contains(event::KeyModifiers::ALT))
}

/// Redraw every cell without asking the terminal anything. `Terminal::clear`
/// first reads the cursor position, a query some terminals never answer;
/// resizing a full-screen viewport to its own size clears the screen and both
/// buffers instead.
fn repaint(screen: &mut terminal::Screen) -> std::io::Result<()> {
    let size = screen.size()?;
    screen.resize(ratatui::layout::Rect::new(0, 0, size.width, size.height))
}

/// The interface thread is the only way to quit from the keyboard, so its
/// panic asks the host to stop rather than leaving CCCC running unseen.
struct StopHostOnPanic(mpsc::UnboundedSender<HostIntent>);

impl Drop for StopHostOnPanic {
    fn drop(&mut self) {
        if std::thread::panicking() {
            let _ = self.0.send(HostIntent::Quit);
        }
    }
}
