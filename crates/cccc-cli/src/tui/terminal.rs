//! Terminal mode changes for the interface, and their single, idempotent undo.

use crossterm::event::{DisableBracketedPaste, EnableBracketedPaste};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{cursor, execute, terminal};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io::{self, Stdout};
use std::sync::{Mutex, Once, PoisonError, TryLockError};
use std::time::Duration;

pub(super) type Screen = Terminal<CrosstermBackend<Stdout>>;

/// What has to be undone; `None` while the terminal is in its own modes.
static ENTERED: Mutex<Option<Entered>> = Mutex::new(None);
/// Background panics while the interface owns the screen, kept for the
/// receipt instead of being written over the interface.
static DEFERRED_PANICS: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct Entered {
    #[cfg(windows)]
    console: console::Modes,
}

/// Switch to raw input on the alternate screen. On failure the steps already
/// taken are undone before the error returns.
pub(super) fn enter() -> io::Result<Screen> {
    install_panic_hook();
    {
        let mut entered = lock(&ENTERED);
        if entered.is_some() {
            return Err(io::Error::other("the terminal interface is already open"));
        }
        #[cfg(windows)]
        let console = console::Modes::save()?;
        terminal::enable_raw_mode()?;
        *entered = Some(Entered {
            #[cfg(windows)]
            console,
        });
    }
    // Writing happens without the lock, so a console that stops accepting
    // output cannot keep a restore from putting the input modes back.
    let prepared = prepare().and_then(|()| Terminal::new(CrosstermBackend::new(io::stdout())));
    if prepared.is_err() {
        let _ = restore();
        return prepared;
    }
    // A restore that ran while this was writing could not undo what was
    // written after it, so close the screen again here.
    if !is_entered() {
        let _ = execute!(
            io::stdout(),
            LeaveAlternateScreen,
            DisableBracketedPaste,
            cursor::Show
        );
        return Err(io::Error::other(
            "the terminal interface was closed while opening",
        ));
    }
    prepared
}

fn prepare() -> io::Result<()> {
    #[cfg(windows)]
    console::prepare()?;
    // Paste is accepted as data so that a pasted line break cannot submit
    // a command; terminals without bracketed paste deliver plain keys.
    execute!(
        io::stdout(),
        EnterAlternateScreen,
        EnableBracketedPaste,
        cursor::Hide
    )
}

/// Undo every mode the interface set, once. Later calls do nothing. Output
/// can block when the terminal stops accepting it, so the input modes, which
/// need none, are restored first, and callers that must not wait run this
/// where they can bound the wait.
pub(crate) fn restore() -> io::Result<()> {
    let mut entered = lock(&ENTERED);
    let Some(_modes) = entered.take() else {
        return Ok(());
    };
    let mut out = io::stdout();
    let results = [
        terminal::disable_raw_mode(),
        #[cfg(windows)]
        _modes.console.restore_input(),
        execute!(out, LeaveAlternateScreen),
        execute!(out, DisableBracketedPaste),
        execute!(out, cursor::Show),
        // The sequences above rely on VT output processing.
        #[cfg(windows)]
        _modes.console.restore_output(),
    ];
    results.into_iter().collect()
}

/// [`restore`] on a helper thread, waiting at most `timeout`. `None` means it
/// did not finish in time; the helper then finishes on its own.
pub(super) fn restore_within(timeout: Duration) -> Option<io::Result<()>> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("cccc-tui-restore".into())
        .spawn(move || {
            let _ = sender.send(restore());
        })
        .ok()?;
    receiver.recv_timeout(timeout).ok()
}

pub(super) fn is_entered() -> bool {
    lock(&ENTERED).is_some()
}

/// Panics a background thread had while the interface was open.
pub(super) fn take_deferred_panics() -> Vec<String> {
    std::mem::take(&mut *lock(&DEFERRED_PANICS))
}

// The interface thread and the main thread end the interface when they panic,
// so the terminal is restored before the panic is reported. Other threads are
// Tokio workers and helpers whose panics the host may survive; writing those
// reports onto the alternate screen would garble it, so they go to the
// diagnostics log, which the host points away from the terminal while the
// interface is open, and wait for the receipt. A panic hook cannot tell
// whether the process survives, so it never claims that services stopped.
fn install_panic_hook() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let thread = std::thread::current();
            let name = thread.name().unwrap_or("<unnamed>");
            if holds_terminal() && name != "main" && name != super::THREAD_NAME {
                let report = format!("thread '{name}' {info}");
                cccc_runtime::diagnostic(format_args!("{report}"));
                lock(&DEFERRED_PANICS).push(report);
                return;
            }
            let _ = restore();
            previous(info);
        }));
    });
}

// The panic hook must not wait on the lock: the panicking thread may be the
// one holding it. A thread busy entering or restoring counts as holding the
// terminal.
fn holds_terminal() -> bool {
    match ENTERED.try_lock() {
        Ok(entered) => entered.is_some(),
        Err(TryLockError::Poisoned(entered)) => entered.into_inner().is_some(),
        Err(TryLockError::WouldBlock) => true,
    }
}

// A poisoned lock only means a panic happened while it was held; the guarded
// state is still the best account of what to undo.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Console modes that crossterm changes without restoring: its raw-mode exit
/// only sets the line, echo and Ctrl+C bits back, and it turns on VT output
/// processing for good.
#[cfg(windows)]
mod console {
    use crossterm_winapi::{ConsoleMode, Handle};
    use std::io;

    const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;
    const ENABLE_VIRTUAL_TERMINAL_INPUT: u32 = 0x0200;

    pub(super) struct Modes {
        input: u32,
        output: u32,
    }

    impl Modes {
        pub(super) fn save() -> io::Result<Self> {
            Ok(Self {
                input: input()?.mode()?,
                output: output()?.mode()?,
            })
        }

        pub(super) fn restore_input(&self) -> io::Result<()> {
            input()?.set_mode(self.input)
        }

        pub(super) fn restore_output(&self) -> io::Result<()> {
            output()?.set_mode(self.output)
        }
    }

    /// Input arrives as key records, which crossterm decodes; with VT input
    /// left on by an earlier program, keys would arrive as escape sequences
    /// it reads as separate characters. Output needs VT processing for the
    /// alternate screen and colors.
    pub(super) fn prepare() -> io::Result<()> {
        let input = input()?;
        input.set_mode(input.mode()? & !ENABLE_VIRTUAL_TERMINAL_INPUT)?;
        let output = output()?;
        output.set_mode(output.mode()? | ENABLE_VIRTUAL_TERMINAL_PROCESSING)
    }

    fn input() -> io::Result<ConsoleMode> {
        Ok(ConsoleMode::from(Handle::current_in_handle()?))
    }

    fn output() -> io::Result<ConsoleMode> {
        Ok(ConsoleMode::from(Handle::current_out_handle()?))
    }
}
