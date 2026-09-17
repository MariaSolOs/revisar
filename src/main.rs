use anyhow::{Context, Result, bail};
use crossterm::{
    cursor::Show,
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use revisar::{
    app::{Action, App, Confirmation, Mode},
    diff::Snapshot,
    review::markdown,
    ui::Ui,
};
use std::{
    fs::{File, OpenOptions},
    io::{self, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

struct Screen(Terminal<CrosstermBackend<File>>);

fn restore() {
    let _ = disable_raw_mode();
    if let Ok(mut tty) = OpenOptions::new().write(true).open("/dev/tty") {
        let _ = execute!(
            tty,
            PopKeyboardEnhancementFlags,
            DisableBracketedPaste,
            LeaveAlternateScreen,
            Show
        );
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        restore();
    }
}

impl Screen {
    fn open() -> Result<Self> {
        // UI always goes to the controlling terminal. stdout is feedback only,
        // so the Pi extension can redirect it without escape-code pollution.
        let tty = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .context("revisar needs an interactive terminal")?;
        let mut screen = Self(Terminal::new(CrosstermBackend::new(tty))?);
        enable_raw_mode()?;
        execute!(
            screen.0.backend_mut(),
            EnterAlternateScreen,
            EnableBracketedPaste,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
        // Terminal::clear queries the cursor through crossterm's global
        // stdout, which would pollute redirected feedback and then time out.
        execute!(
            screen.0.backend_mut(),
            crossterm::terminal::Clear(crossterm::terminal::ClearType::All)
        )?;
        Ok(screen)
    }
}

fn run() -> Result<u8> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [] => (),
        [arg] if arg == "--help" || arg == "-h" => {
            println!(
                "revisar - review agentic working-tree changes\n\nUsage: revisar [--help | --version]\n\nRun inside a Git working tree. Reviews net staged + unstaged + untracked\nchanges against HEAD. UI uses /dev/tty; explicit Send writes Markdown to\nstdout. Nothing is saved. q cancels without output.\n\nKeys: c comment, v range, C file, a general, s summary, S Send, ? help.\n\nExit codes: 0 sent, 1 error, 2 cancelled (no feedback).\nBuild: cargo build --release --locked"
            );
            return Ok(0);
        }
        [arg] if arg == "--version" || arg == "-V" => {
            println!("revisar {}", env!("CARGO_PKG_VERSION"));
            return Ok(0);
        }
        _ => bail!("Unsupported arguments. Run revisar --help"),
    }
    let snapshot = Snapshot::load(&std::env::current_dir()?)?;
    let mut app = App::new(snapshot);
    let mut ui = Ui::default();
    let terminated = Arc::new(AtomicBool::new(false));
    for signal in [
        signal_hook::consts::SIGTERM,
        signal_hook::consts::SIGHUP,
        signal_hook::consts::SIGINT,
    ] {
        signal_hook::flag::register(signal, Arc::clone(&terminated))?;
    }
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        hook(info);
    }));
    let mut screen = Screen::open()?;
    let stale = loop {
        if terminated.load(Ordering::Relaxed) {
            return Ok(2);
        }
        screen.0.draw(|frame| ui.draw(frame, &mut app))?;
        if !event::poll(Duration::from_millis(100))? {
            continue;
        }
        match app.event(event::read()?) {
            Action::Continue => (),
            Action::Cancel => return Ok(2),
            Action::Send => {
                app.message = "Checking that the working tree has not changed...".into();
                screen.0.draw(|frame| ui.draw(frame, &mut app))?;
                match app.snapshot.unchanged() {
                    Ok(true) => break false,
                    Ok(false) => app.mode = Mode::Confirm(Confirmation::Stale),
                    Err(e) => {
                        app.message =
                            format!("Cannot verify the snapshot; feedback was not sent: {e:#}")
                    }
                }
            }
            Action::SendStale => break true,
        }
    };
    drop(screen);
    let feedback = markdown(&app.snapshot, &app.comments, stale);
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(feedback.as_bytes())
        .context("Could not deliver review to stdout")?;
    stdout.flush()?;
    Ok(0)
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(code) => std::process::ExitCode::from(code),
        Err(error) => {
            eprintln!("revisar: {error:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
