//! tsm — traex session manager (spec §1). Ticket 01 walking skeleton:
//! launch → current-project active list → quit.

mod app;
mod format;
mod mutate;
mod rename;
mod store;
mod ui;
mod update;

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
    match parse_command(std::env::args().skip(1))? {
        Command::Version => {
            println!("tsm {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Command::SelfUpdate { check_only } => {
            update::self_update(check_only)?;
            Ok(())
        }
        Command::Tui { store_path } => run_app(store_path),
    }
}

fn run_app(store_flag: Option<PathBuf>) -> Result<()> {
    let store_path = store::locate_db(store_flag.as_deref())?;
    let store = Store::open(store_path)?;

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

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Tui { store_path: Option<PathBuf> },
    Version,
    SelfUpdate { check_only: bool },
}

fn parse_command(mut args: impl Iterator<Item = String>) -> Result<Command> {
    let Some(first) = args.next() else {
        return Ok(Command::Tui { store_path: None });
    };

    match first.as_str() {
        "--version" | "-V" => {
            ensure_no_more_args(args, "--version")?;
            Ok(Command::Version)
        }
        "self-update" => {
            let check_only = match args.next().as_deref() {
                None => false,
                Some("--check") => true,
                Some(other) => anyhow::bail!("unknown self-update argument: {other}"),
            };
            ensure_no_more_args(args, "self-update")?;
            Ok(Command::SelfUpdate { check_only })
        }
        _ => parse_tui_args(std::iter::once(first).chain(args)),
    }
}

fn ensure_no_more_args(mut args: impl Iterator<Item = String>, command: &str) -> Result<()> {
    if let Some(argument) = args.next() {
        anyhow::bail!("unexpected argument for {command}: {argument}");
    }
    Ok(())
}

/// Parse the TUI's single supported flag, `--db <path>` (spec §9.3).
fn parse_tui_args(mut args: impl Iterator<Item = String>) -> Result<Command> {
    let mut store_path = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => {
                let path = args.next().context("--db requires a path argument")?;
                store_path = Some(PathBuf::from(path));
            }
            other if other.starts_with("--db=") => {
                store_path = Some(PathBuf::from(&other["--db=".len()..]));
            }
            other => anyhow::bail!("unknown argument: {other}"),
        }
    }
    Ok(Command::Tui { store_path })
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
        Mode::Rename { .. } => handle_key_rename(app, key),
        Mode::ConfirmDelete { .. } => handle_key_confirm(app, key),
        Mode::Running { .. } => handle_key_running(app, key),
        Mode::Result { .. } => handle_key_result(app, key),
        Mode::Help => app.dismiss_help(),
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
        KeyCode::Char('r') => app.start_rename(),
        KeyCode::Char('/') => app.enter_search(),
        KeyCode::Char('R') => app.refresh(),
        KeyCode::Char('?') => app.show_help(),
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

/// Rename-mode keys: single-line character editing and save/cancel (spec §7.1).
fn handle_key_rename(app: &mut App, key: KeyEvent) {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Char('c') if ctrl => app.should_quit = true,
        KeyCode::Enter => app.submit_rename(),
        KeyCode::Esc => app.cancel_rename(),
        KeyCode::Left => app.rename_left(),
        KeyCode::Right => app.rename_right(),
        KeyCode::Home => app.rename_home(),
        KeyCode::End => app.rename_end(),
        KeyCode::Backspace => app.rename_backspace(),
        KeyCode::Delete => app.rename_delete(),
        KeyCode::Char(character) => app.rename_insert(character),
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

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn app() -> App {
        static N: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "tsm-main-{}-{}.sqlite",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_file(&path);
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE threads (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL DEFAULT '',
                first_user_message TEXT NOT NULL DEFAULT '',
                cwd TEXT NOT NULL,
                source TEXT NOT NULL DEFAULT 'cli',
                updated_at INTEGER NOT NULL,
                updated_at_ms INTEGER,
                archived INTEGER NOT NULL DEFAULT 0,
                archived_at INTEGER,
                git_branch TEXT,
                model TEXT,
                tokens_used INTEGER NOT NULL DEFAULT 0
            );
            INSERT INTO threads
                (id,title,cwd,updated_at,updated_at_ms,archived,tokens_used)
            VALUES ('one','one','/proj',1,1000,0,0);",
        )
        .unwrap();
        drop(conn);
        let store = Store::open(path).unwrap();
        let rows = store.query_project_active("/proj").unwrap();
        App::with_runner_and_availability(
            store,
            "/proj".to_string(),
            rows,
            std::sync::Arc::new(|_, _| None),
            true,
        )
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn parses_version_without_opening_the_store() {
        assert_eq!(
            parse_command(["--version".to_string()].into_iter()).unwrap(),
            Command::Version
        );
    }

    #[test]
    fn parses_self_update_check_without_opening_the_store() {
        assert_eq!(
            parse_command(["self-update".to_string(), "--check".to_string()].into_iter()).unwrap(),
            Command::SelfUpdate { check_only: true }
        );
    }

    #[test]
    fn rejects_unknown_self_update_arguments() {
        let error = parse_command(["self-update".to_string(), "--force".to_string()].into_iter())
            .unwrap_err();
        assert_eq!(error.to_string(), "unknown self-update argument: --force");
    }

    #[test]
    fn normal_mode_wires_refresh_and_help() {
        let mut app = app();
        app.message = Some("stale".to_string());
        handle_key(&mut app, key(KeyCode::Char('R')));
        assert_eq!(app.message, None);

        handle_key(&mut app, key(KeyCode::Char('?')));
        assert_eq!(app.mode, Mode::Help);
        handle_key(&mut app, key(KeyCode::Char('x')));
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn help_esc_also_dismisses() {
        let mut app = app();
        app.show_help();
        handle_key(&mut app, key(KeyCode::Esc));
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn rename_keymap_enters_edits_and_cancels() {
        let mut app = app();
        handle_key(&mut app, key(KeyCode::Char('r')));
        assert!(matches!(app.mode, Mode::Rename { .. }));

        handle_key(&mut app, key(KeyCode::Home));
        handle_key(&mut app, key(KeyCode::Char('新')));
        handle_key(&mut app, key(KeyCode::Right));
        handle_key(&mut app, key(KeyCode::End));
        handle_key(&mut app, key(KeyCode::Backspace));
        handle_key(&mut app, key(KeyCode::Delete));
        assert!(matches!(app.mode, Mode::Rename { .. }));

        handle_key(&mut app, key(KeyCode::Esc));
        assert_eq!(app.mode, Mode::Normal);
    }
}
