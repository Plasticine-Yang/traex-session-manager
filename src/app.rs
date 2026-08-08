//! App — the in-memory state machine behind the UI (spec §5.5).
//!
//! Ticket 02 adds the two orthogonal filter dimensions — Scope (Project ↔ All)
//! and Lifecycle (Active ↔ Archived) — plus the preview toggle. `scope +
//! lifecycle` decide the DB query (spec §4.2 seam); `search`/sort still land in
//! later tickets on the in-memory snapshot.

use ratatui::widgets::TableState;

use crate::store::{Session, Store};

/// Project range of the list (spec §4.3, CONTEXT "Scope"). Two-state toggle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// `threads.cwd` byte-exactly equals the launch directory (spec §8).
    Project,
    /// All projects; the list gains a `cwd` column (spec §5.2).
    All,
}

/// Active/archived band of the list (spec §4.3, CONTEXT "Lifecycle"). Strictly
/// two-state — no combined/all view (spec §4.3, non-goal).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    /// `archived = 0`.
    Active,
    /// `archived = 1`.
    Archived,
}

/// Why the current view has no rows, so the UI can pick the right guidance
/// (spec §4.6 / §8). Search-driven emptiness arrives in a later ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmptyReason {
    /// Scope=Project, Active, nothing here — offer switching to all projects.
    ProjectEmpty,
    /// Lifecycle=Archived, nothing archived in this scope.
    ArchivedEmpty,
    /// Scope=All, Active, and genuinely no sessions at all.
    NoSessions,
}

/// Application state (spec §5.5, ticket-02 subset).
pub struct App {
    /// Read-only handle used to re-query on scope/lifecycle changes (spec §4.2).
    store: Store,
    /// Last query result — the authoritative snapshot (spec §4.2).
    pub all_rows: Vec<Session>,
    /// Filtered+sorted indices into `all_rows`, in render order (spec §4.2).
    pub view: Vec<usize>,
    /// Cursor / scroll position over `view`.
    pub table: TableState,
    /// tsm's launch directory; the Scope=Project match key (spec §8).
    pub cwd: String,
    /// `$HOME`, used only to render `cwd` relative to `~` in the All view (spec §5.2).
    pub home: String,
    /// Project range (spec §4.3).
    pub scope: Scope,
    /// Lifecycle band (spec §4.3).
    pub lifecycle: Lifecycle,
    /// User's preview-panel intent (spec §5.1); the UI additionally auto-hides
    /// it when the terminal is narrower than 100 columns.
    pub show_preview: bool,
    /// Transient footer message (e.g. a busy re-query that kept stale rows).
    pub message: Option<String>,
    /// Set once `q` / `Ctrl-c` is pressed.
    pub should_quit: bool,
}

impl App {
    /// Build an app around the initial snapshot for the current project.
    ///
    /// `initial_rows` is the startup query (Project · Active) that main already
    /// ran, so a startup lock stays a clean fatal exit (spec §11) rather than a
    /// half-drawn TUI.
    pub fn new(store: Store, cwd: String, initial_rows: Vec<Session>) -> Self {
        let home = std::env::var("HOME").unwrap_or_default();
        let mut app = App {
            store,
            all_rows: initial_rows,
            view: Vec::new(),
            table: TableState::default(),
            cwd,
            home,
            scope: Scope::Project,
            lifecycle: Lifecycle::Active,
            show_preview: true,
            message: None,
            should_quit: false,
        };
        app.rebuild_view();
        app
    }

    /// Recompute `view` from `all_rows`. Ticket 02 has no in-memory search yet,
    /// so `view` is the identity mapping over the (already sorted) query result.
    pub fn rebuild_view(&mut self) {
        self.view = (0..self.all_rows.len()).collect();
        self.clamp_cursor();
    }

    /// Rows currently on screen, in render order.
    pub fn visible_sessions(&self) -> impl Iterator<Item = &Session> {
        self.view.iter().map(move |&i| &self.all_rows[i])
    }

    /// The row under the cursor, if any.
    pub fn selected_session(&self) -> Option<&Session> {
        let i = self.table.selected()?;
        self.view.get(i).map(|&idx| &self.all_rows[idx])
    }

    /// Toggle Scope (Project ↔ All) and re-query (spec §4.3 / §2.5).
    pub fn toggle_scope(&mut self) {
        self.scope = match self.scope {
            Scope::Project => Scope::All,
            Scope::All => Scope::Project,
        };
        self.requery();
    }

    /// Toggle Lifecycle (Active ↔ Archived) and re-query (spec §4.3 / §2.5).
    pub fn toggle_lifecycle(&mut self) {
        self.lifecycle = match self.lifecycle {
            Lifecycle::Active => Lifecycle::Archived,
            Lifecycle::Archived => Lifecycle::Active,
        };
        self.requery();
    }

    /// Toggle the preview panel's visibility intent (`Enter` in Normal, spec §5.7).
    pub fn toggle_preview(&mut self) {
        self.show_preview = !self.show_preview;
    }

    /// Re-run the query for the current scope×lifecycle and reset the cursor to
    /// the top (spec §2.5 / §4.2). A busy timeout is non-fatal: keep the stale
    /// rows and surface a footer message (spec §11 runtime rule).
    fn requery(&mut self) {
        let scope_cwd = match self.scope {
            Scope::Project => Some(self.cwd.as_str()),
            Scope::All => None,
        };
        let archived = matches!(self.lifecycle, Lifecycle::Archived);
        match self.store.query(scope_cwd, archived) {
            Ok(rows) => {
                self.all_rows = rows;
                self.message = None;
                self.rebuild_view();
                self.reset_cursor();
            }
            Err(err) => {
                // Runtime transient error (spec §11): non-fatal, keep the stale
                // rows and surface the message. Manual `R` retry lands in ticket
                // 07, so don't promise a key that isn't wired yet.
                self.message = Some(err.to_string());
            }
        }
    }

    /// Point the cursor at the first row (or none when empty) after the result
    /// set changes wholesale, so an old index never lands on an unrelated row.
    fn reset_cursor(&mut self) {
        *self.table.offset_mut() = 0;
        if self.view.is_empty() {
            self.table.select(None);
        } else {
            self.table.select(Some(0));
        }
    }

    /// Classify why the current view is empty, for the guidance text (spec §4.6 / §8).
    pub fn empty_reason(&self) -> EmptyReason {
        match (self.scope, self.lifecycle) {
            (_, Lifecycle::Archived) => EmptyReason::ArchivedEmpty,
            (Scope::Project, Lifecycle::Active) => EmptyReason::ProjectEmpty,
            (Scope::All, Lifecycle::Active) => EmptyReason::NoSessions,
        }
    }

    /// Ensure the cursor points at a valid row (or none when empty).
    fn clamp_cursor(&mut self) {
        if self.view.is_empty() {
            self.table.select(None);
        } else {
            let idx = self.table.selected().unwrap_or(0).min(self.view.len() - 1);
            self.table.select(Some(idx));
        }
    }

    /// Move the cursor down one row (`j` / `↓`).
    pub fn cursor_down(&mut self) {
        if self.view.is_empty() {
            return;
        }
        let next = match self.table.selected() {
            Some(i) if i + 1 < self.view.len() => i + 1,
            Some(i) => i,
            None => 0,
        };
        self.table.select(Some(next));
    }

    /// Move the cursor up one row (`k` / `↑`).
    pub fn cursor_up(&mut self) {
        if self.view.is_empty() {
            return;
        }
        let next = match self.table.selected() {
            Some(i) => i.saturating_sub(1),
            None => 0,
        };
        self.table.select(Some(next));
    }

    /// Jump to the first row (`g`).
    pub fn cursor_first(&mut self) {
        if !self.view.is_empty() {
            self.table.select(Some(0));
        }
    }

    /// Jump to the last row (`G`).
    pub fn cursor_last(&mut self) {
        if !self.view.is_empty() {
            self.table.select(Some(self.view.len() - 1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::path::Path;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Seed a `threads` table under `/proj` (2 active, 1 archived) and
    /// `/elsewhere` (1 active, 1 archived) so scope×lifecycle toggles are
    /// observable.
    fn seed(path: &Path, active_in_proj: usize) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE threads (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL DEFAULT '',
                first_user_message TEXT NOT NULL DEFAULT '',
                cwd TEXT NOT NULL,
                updated_at INTEGER NOT NULL,
                updated_at_ms INTEGER,
                archived INTEGER NOT NULL DEFAULT 0,
                archived_at INTEGER,
                git_branch TEXT,
                model TEXT,
                tokens_used INTEGER NOT NULL DEFAULT 0
            );",
        )
        .unwrap();
        for i in 0..active_in_proj {
            conn.execute(
                "INSERT INTO threads (id,title,cwd,updated_at,updated_at_ms,archived,tokens_used)
                 VALUES (?1,?1,'/proj',?2,?3,0,0)",
                rusqlite::params![format!("p{i}"), i as i64, i as i64 * 1000],
            )
            .unwrap();
        }
        conn.execute_batch(
            "INSERT INTO threads (id,title,cwd,updated_at,updated_at_ms,archived,archived_at,tokens_used)
             VALUES ('parch','ParchTitle','/proj',900,900000,1,9050,0),
                    ('oact','OactTitle','/elsewhere',800,800000,0,NULL,0),
                    ('oarch','OarchTitle','/elsewhere',700,700000,1,7050,0)",
        )
        .unwrap();
    }

    fn unique_db() -> std::path::PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("tsm-app-{}-{}.sqlite", std::process::id(), n))
    }

    /// Build an app with `n` active current-project rows, landing on the default
    /// Project · Active view.
    fn app_with(n: usize) -> App {
        let path = unique_db();
        let _ = std::fs::remove_file(&path);
        seed(&path, n);
        let store = Store::open(path.clone()).unwrap();
        let rows = store.query_project_active("/proj").unwrap();
        App::new(store, "/proj".to_string(), rows)
    }

    #[test]
    fn new_app_selects_first_row() {
        let app = app_with(3);
        assert_eq!(app.view, vec![0, 1, 2]);
        assert_eq!(app.table.selected(), Some(0));
        assert_eq!(app.scope, Scope::Project);
        assert_eq!(app.lifecycle, Lifecycle::Active);
    }

    #[test]
    fn empty_app_selects_nothing() {
        let app = app_with(0);
        assert_eq!(app.table.selected(), None);
    }

    #[test]
    fn cursor_moves_within_bounds() {
        let mut app = app_with(3);
        app.cursor_down();
        assert_eq!(app.table.selected(), Some(1));
        app.cursor_down();
        app.cursor_down(); // clamp at last
        assert_eq!(app.table.selected(), Some(2));
        app.cursor_up();
        assert_eq!(app.table.selected(), Some(1));
        app.cursor_up();
        app.cursor_up(); // clamp at first
        assert_eq!(app.table.selected(), Some(0));
    }

    #[test]
    fn jump_to_first_and_last() {
        let mut app = app_with(5);
        app.cursor_last();
        assert_eq!(app.table.selected(), Some(4));
        app.cursor_first();
        assert_eq!(app.table.selected(), Some(0));
    }

    #[test]
    fn navigation_on_empty_is_noop() {
        let mut app = app_with(0);
        app.cursor_down();
        app.cursor_up();
        app.cursor_first();
        app.cursor_last();
        assert_eq!(app.table.selected(), None);
    }

    #[test]
    fn toggle_scope_requeries_all_projects() {
        let mut app = app_with(2); // p0,p1 active in /proj
        assert_eq!(app.all_rows.len(), 2);
        app.toggle_scope();
        assert_eq!(app.scope, Scope::All);
        // All · Active = p0,p1 (/proj) + oact (/elsewhere) = 3 rows.
        assert_eq!(app.all_rows.len(), 3);
        assert!(app.all_rows.iter().any(|s| s.id == "oact"));
        // Cursor resets to the top on a wholesale result change.
        assert_eq!(app.table.selected(), Some(0));
        app.toggle_scope();
        assert_eq!(app.scope, Scope::Project);
        assert_eq!(app.all_rows.len(), 2);
    }

    #[test]
    fn toggle_lifecycle_requeries_archived() {
        let mut app = app_with(2);
        app.toggle_lifecycle();
        assert_eq!(app.lifecycle, Lifecycle::Archived);
        // Project · Archived = parch only.
        assert_eq!(app.all_rows.len(), 1);
        assert_eq!(app.all_rows[0].id, "parch");
        app.toggle_lifecycle();
        assert_eq!(app.lifecycle, Lifecycle::Active);
        assert_eq!(app.all_rows.len(), 2);
    }

    #[test]
    fn scope_and_lifecycle_compose() {
        let mut app = app_with(2);
        app.toggle_scope(); // All · Active
        app.toggle_lifecycle(); // All · Archived
        // All · Archived = parch + oarch.
        assert_eq!(app.all_rows.len(), 2);
        assert!(app.all_rows.iter().any(|s| s.id == "parch"));
        assert!(app.all_rows.iter().any(|s| s.id == "oarch"));
    }

    #[test]
    fn cursor_stays_in_bounds_after_shrinking_requery() {
        let mut app = app_with(3);
        app.cursor_last(); // selects index 2
        app.toggle_lifecycle(); // Project · Archived has 1 row
        assert_eq!(app.table.selected(), Some(0));
        assert_eq!(app.table.offset(), 0);
    }

    #[test]
    fn empty_reason_by_cause() {
        let mut app = app_with(0);
        // Project · Active, empty.
        assert_eq!(app.empty_reason(), EmptyReason::ProjectEmpty);
        app.toggle_lifecycle(); // Project · Archived (has parch, but reason is about band)
        assert_eq!(app.empty_reason(), EmptyReason::ArchivedEmpty);
        app.toggle_lifecycle();
        app.toggle_scope(); // All · Active
        assert_eq!(app.empty_reason(), EmptyReason::NoSessions);
    }

    #[test]
    fn toggle_preview_flips_intent() {
        let mut app = app_with(1);
        assert!(app.show_preview);
        app.toggle_preview();
        assert!(!app.show_preview);
        app.toggle_preview();
        assert!(app.show_preview);
    }

    #[test]
    fn selected_session_tracks_cursor() {
        let mut app = app_with(3);
        // p0 (updated_at_ms 0) .. p2 (2000), newest first => p2,p1,p0.
        assert_eq!(app.selected_session().unwrap().id, "p2");
        app.cursor_last();
        assert_eq!(app.selected_session().unwrap().id, "p0");
    }
}
