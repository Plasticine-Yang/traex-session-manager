//! App — the in-memory state machine behind the UI (spec §5.5).
//!
//! Ticket 01 only needs the `all_rows` / `view` seam plus cursor navigation;
//! later tickets grow scope, lifecycle, search, selection, and modes here.

use ratatui::widgets::TableState;

use crate::store::Session;

/// Application state (spec §5.5, ticket-01 subset).
pub struct App {
    /// Last query result — the authoritative snapshot (spec §4.2).
    pub all_rows: Vec<Session>,
    /// Filtered+sorted indices into `all_rows`, in render order (spec §4.2).
    pub view: Vec<usize>,
    /// Cursor / scroll position over `view`.
    pub table: TableState,
    /// tsm's launch directory; the Scope=Project match key (spec §8). Consumed
    /// by the scope-toggle refresh in a later ticket.
    #[allow(dead_code)]
    pub cwd: String,
    /// Set once `q` / `Ctrl-c` is pressed.
    pub should_quit: bool,
}

impl App {
    /// Build an app around the initial snapshot for the current project.
    pub fn new(cwd: String, all_rows: Vec<Session>) -> Self {
        let mut app = App {
            all_rows,
            view: Vec::new(),
            table: TableState::default(),
            cwd,
            should_quit: false,
        };
        app.rebuild_view();
        app
    }

    /// Recompute `view` from `all_rows`. Ticket 01 is the default landing:
    /// rows are already the current-project active set sorted by
    /// `updated_at_ms DESC` from the query, so `view` is the identity mapping.
    pub fn rebuild_view(&mut self) {
        self.view = (0..self.all_rows.len()).collect();
        self.clamp_cursor();
    }

    /// Rows currently on screen, in render order.
    pub fn visible_sessions(&self) -> impl Iterator<Item = &Session> {
        self.view.iter().map(move |&i| &self.all_rows[i])
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

    fn session(id: &str) -> Session {
        Session {
            id: id.to_string(),
            title: id.to_string(),
            first_user_message: String::new(),
            cwd: "/proj".to_string(),
            updated_at: 0,
            updated_at_ms: 0,
            archived: false,
            archived_at: None,
            git_branch: None,
            model: None,
            tokens_used: 0,
        }
    }

    fn app_with(n: usize) -> App {
        let rows = (0..n).map(|i| session(&i.to_string())).collect();
        App::new("/proj".to_string(), rows)
    }

    #[test]
    fn new_app_selects_first_row() {
        let app = app_with(3);
        assert_eq!(app.view, vec![0, 1, 2]);
        assert_eq!(app.table.selected(), Some(0));
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
}
