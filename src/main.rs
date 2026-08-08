//! tsm — traex session manager (spec §1). Ticket 01 walking skeleton:
//! launch → current-project active list → quit.

mod app;
mod format;
mod mutate;
mod rename;
mod store;
mod ui;

use std::io::{self, Stdout};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use app::App;
use app::Mode;
use store::Store;

fn main() {
    if let Err(err) = run() {
        // Startup-phase fatal errors: clear stderr message, non-zero exit, no TUI
        // (spec §11).
        eprintln!("tsm: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let db_flag = parse_db_flag(std::env::args().skip(1))?;
    let db_path = store::locate_db(db_flag.as_deref())?;
    let store = Store::open(db_path)?;

    // Launch anchor = process CWD, byte-exact match key (spec §8).
    let cwd = std::env::current_dir()
        .context("failed to read current directory")?
        .to_string_lossy()
        .into_owned();

    // First query runs before the TUI so a startup lock is a clean fatal exit
    // (spec §11); the store maps SQLITE_BUSY to the busy message.
    let rows = store.query_project_active(&cwd)?;

    let mut app = App::new(store, cwd, rows);
    run_tui(&mut app)?;
    Ok(())
}

/// Parse the single supported flag, `--db <path>` (spec §9.3). Unknown flags
/// are surfaced rather than silently ignored.
fn parse_db_flag(mut args: impl Iterator<Item = String>) -> Result<Option<PathBuf>> {
    let mut db = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => {
                let path = args
                    .next()
                    .context("--db requires a path argument")?;
                db = Some(PathBuf::from(path));
            }
            other if other.starts_with("--db=") => {
                db = Some(PathBuf::from(&other["--db=".len()..]));
            }
            other => anyhow::bail!("unknown argument: {other}"),
        }
    }
    Ok(db)
}

/// Set up the terminal, run the event loop, and always restore on exit.
fn run_tui(app: &mut App) -> Result<()> {
    let mut terminal = setup_terminal()?;
    let result = event_loop(&mut terminal, app);
    restore_terminal(&mut terminal)?;
    result
}

type Tui = Terminal<CrosstermBackend<Stdout>>;

fn setup_terminal() -> Result<Tui> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    Ok(Terminal::new(backend)?)
}

fn restore_terminal(terminal: &mut Tui) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn event_loop(terminal: &mut Tui, app: &mut App) -> Result<()> {
    while !app.should_quit {
        terminal.draw(|f| ui::draw(f, app))?;

        // While a batch runs, fold in any completed outcomes each frame so the
        // progress modal advances and the run finishes (spec §6.1/§6.5). A short
        // poll keeps the progress bar responsive without a busy loop.
        let timeout = if matches!(app.mode, Mode::Running { .. }) {
            app.poll_batch();
            Duration::from_millis(50)
        } else {
            Duration::from_millis(250)
        };

        // Poll so the loop can stay responsive; only key presses drive state.
        if event::poll(timeout)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            handle_key(app, key);
        }
    }
    Ok(())
}

/// Map a key press to a state transition (spec §5.7). Dispatch is
/// mode-dependent: in Search mode printable keys edit the term (spec §4.4), and
/// the batch modes (confirm/running/result) have their own tiny key tables
/// (spec §6.3/§6.6/§6.8).
fn handle_key(app: &mut App, key: KeyEvent) {
    match app.mode {
        Mode::Normal => handle_key_normal(app, key),
        Mode::Search => handle_key_search(app, key),
        Mode::ConfirmDelete { .. } => handle_key_confirm(app, key),
        Mode::Running { .. } => handle_key_running(app, key),
        Mode::Result { .. } => handle_key_result(app, key),
    }
}

/// Normal-mode keys: navigation, filter toggles, selection, search entry (spec §5.7).
fn handle_key_normal(app: &mut App, key: KeyEvent) {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Char('c') if ctrl => app.should_quit = true,
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Char('j') | KeyCode::Down => app.cursor_down(),
        KeyCode::Char('k') | KeyCode::Up => app.cursor_up(),
        KeyCode::Char('g') => app.cursor_first(),
        KeyCode::Char('G') => app.cursor_last(),
        KeyCode::Char('p') => app.toggle_scope(),
        KeyCode::Tab => app.toggle_lifecycle(),
        KeyCode::Char(' ') => app.toggle_selected(),
        KeyCode::Char('*') => app.invert_visible_selection(),
        KeyCode::Char('d') => app.request_delete(),
        KeyCode::Char('a') => app.request_archive(),
        KeyCode::Char('/') => app.enter_search(),
        // `Esc` clears a committed filter (spec §4.4); harmless when none is set.
        KeyCode::Esc => app.search_clear(),
        KeyCode::Enter => app.toggle_preview(),
        _ => {}
    }
}

/// Search-mode keys: printable input edits the term with live filtering; only
/// `↑`/`↓` move the cursor, `Enter` commits, `Esc` clears (spec §4.4).
fn handle_key_search(app: &mut App, key: KeyEvent) {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Char('c') if ctrl => app.should_quit = true,
        KeyCode::Down => app.cursor_down(),
        KeyCode::Up => app.cursor_up(),
        KeyCode::Enter => app.search_commit(),
        KeyCode::Esc => app.search_clear(),
        KeyCode::Backspace => app.search_backspace(),
        KeyCode::Char(c) => app.search_push(c),
        _ => {}
    }
}

/// Delete-confirmation modal keys (spec §6.3): uppercase `D` confirms (guards
/// against a fat-fingered lowercase), `Esc`/`n` cancel.
fn handle_key_confirm(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('D') => app.confirm_delete(),
        KeyCode::Esc | KeyCode::Char('n') => app.cancel_confirm(),
        _ => {}
    }
}

/// Progress-modal keys (spec §6.8): `Esc` stops dispatch but lets in-flight
/// workers finish; everything else is swallowed (the batch is blocking).
fn handle_key_running(app: &mut App, key: KeyEvent) {
    if key.code == KeyCode::Esc {
        app.cancel_batch();
    }
}

/// Result-face keys (spec §6.6): `d` retries the still-selected failures, `Esc`
/// closes back to the list.
fn handle_key_result(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('d') => app.retry_failed(),
        KeyCode::Esc => app.dismiss_result(),
        _ => {}
    }
}
