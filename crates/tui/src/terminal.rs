use std::{
    io::{self, Stdout, Write, stdout},
    panic,
    sync::{
        Once,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
};

use crossterm::{
    cursor::Show,
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{
        BeginSynchronizedUpdate, EndSynchronizedUpdate, EnterAlternateScreen, LeaveAlternateScreen,
        disable_raw_mode, enable_raw_mode,
    },
};
use ratatui::{Frame, Terminal, backend::CrosstermBackend};

static ACTIVE: AtomicBool = AtomicBool::new(false);
static MODES: AtomicU8 = AtomicU8::new(0);
static PANIC_HOOK: Once = Once::new();

const MOUSE: u8 = 1;
const KEYBOARD: u8 = 2;

#[derive(Clone, Copy, Debug)]
pub struct TerminalOptions {
    pub mouse_capture: bool,
    pub keyboard_enhancement: bool,
}

impl Default for TerminalOptions {
    fn default() -> Self {
        Self {
            mouse_capture: true,
            keyboard_enhancement: true,
        }
    }
}

pub struct TerminalSession {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    options: TerminalOptions,
    active: bool,
}

impl TerminalSession {
    pub fn enter(options: TerminalOptions) -> io::Result<Self> {
        install_panic_hook();
        enable_raw_mode()?;
        let mut output = stdout();
        if let Err(error) = activate(&mut output, options) {
            restore(&mut output);
            return Err(error);
        }
        let terminal = match Terminal::new(CrosstermBackend::new(output)) {
            Ok(terminal) => terminal,
            Err(error) => {
                restore(&mut stdout());
                return Err(error);
            }
        };
        Ok(Self {
            terminal,
            options,
            active: true,
        })
    }

    pub fn draw(&mut self, render: impl FnOnce(&mut Frame<'_>)) -> io::Result<()> {
        execute!(self.terminal.backend_mut(), BeginSynchronizedUpdate)?;
        let drawn = self.terminal.draw(render).map(drop);
        execute!(self.terminal.backend_mut(), EndSynchronizedUpdate)?;
        drawn
    }

    pub fn clear(&mut self) -> io::Result<()> {
        self.terminal.clear()
    }

    pub fn suspend(&mut self) -> io::Result<()> {
        if self.active {
            restore(self.terminal.backend_mut());
            self.active = false;
        }
        Ok(())
    }

    pub fn resume(&mut self) -> io::Result<()> {
        if !self.active {
            enable_raw_mode()?;
            activate(self.terminal.backend_mut(), self.options)?;
            self.terminal.clear()?;
            self.active = true;
        }
        Ok(())
    }

    pub fn set_mouse_capture(&mut self, enabled: bool) -> io::Result<()> {
        if enabled == self.options.mouse_capture {
            return Ok(());
        }
        self.options.mouse_capture = enabled;
        if enabled {
            execute!(self.terminal.backend_mut(), EnableMouseCapture)?;
            MODES.fetch_or(MOUSE, Ordering::AcqRel);
        } else {
            execute!(self.terminal.backend_mut(), DisableMouseCapture)?;
            MODES.fetch_and(!MOUSE, Ordering::AcqRel);
        }
        Ok(())
    }

    #[must_use]
    pub const fn mouse_capture(&self) -> bool {
        self.options.mouse_capture
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        if self.active {
            drop(self.terminal.show_cursor());
            restore(self.terminal.backend_mut());
        }
    }
}

fn activate(output: &mut impl Write, options: TerminalOptions) -> io::Result<()> {
    ACTIVE.store(true, Ordering::Release);
    execute!(output, EnterAlternateScreen, EnableBracketedPaste)?;
    let mut modes = 0;
    if options.mouse_capture {
        execute!(output, EnableMouseCapture)?;
        modes |= MOUSE;
    }
    if options.keyboard_enhancement
        && execute!(
            output,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )
        .is_ok()
    {
        modes |= KEYBOARD;
    }
    MODES.store(modes, Ordering::Release);
    Ok(())
}

fn restore(output: &mut impl Write) {
    ACTIVE.store(false, Ordering::Release);
    let modes = MODES.swap(0, Ordering::AcqRel);
    drop(disable_raw_mode());
    if modes & KEYBOARD != 0 {
        drop(execute!(output, PopKeyboardEnhancementFlags));
    }
    if modes & MOUSE != 0 {
        drop(execute!(output, DisableMouseCapture));
    }
    drop(execute!(
        output,
        EndSynchronizedUpdate,
        DisableBracketedPaste,
        LeaveAlternateScreen,
        Show
    ));
}

fn install_panic_hook() {
    PANIC_HOOK.call_once(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            if ACTIVE.load(Ordering::Acquire) {
                restore(&mut stdout());
            }
            previous(info);
        }));
    });
}
