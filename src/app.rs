//! App — the in-memory state machine behind the UI (spec §5.5).
//!
//! Ticket 02 added the two orthogonal filter dimensions — Scope (Project ↔ All)
//! and Lifecycle (Active ↔ Archived) — plus the preview toggle. Ticket 03 adds
//! the third dimension, Search, plus multi-selection. `scope + lifecycle` decide
//! the DB query (spec §4.2 seam); `search` + sort work on the in-memory
//! `all_rows` snapshot instead of re-querying per keystroke (spec §4.2 / R1).
//! Ticket 07 adds honest manual refresh and the help overlay.

use std::collections::HashSet;

use ratatui::widgets::TableState;

use crate::mutate::{self, BatchHandle, BatchJob, Op, Runner};
use crate::rename::{self, RenameError, RenameOutcome, RenameRunner};
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

/// Input mode (spec §5.5). Because `p`/`Tab`/`Space`/`*` are all printable, live
/// search has to be a distinct mode or typing would leak into filter keys (spec
/// §4.4). Ticket 05 adds the batch-delete trio (confirm → running → result);
/// ticket 07 adds help. The `Running`/`Result` modes carry only the
/// op verb; the live counters and failure list live on [`App`] (they mutate
/// every frame and are neither cloneable nor cheap to compare).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// Keys act on the filtered set (navigation, toggles, selection).
    Normal,
    /// Live incremental filtering; printable keys edit the search term.
    Search,
    /// Inline single-row title editor. `cursor` is a character index, never a
    /// byte offset, so left/right/delete remain safe for CJK input (spec §7.1).
    Rename {
        id: String,
        buf: String,
        cursor: usize,
    },
    /// Delete-confirmation modal (spec §6.3): lists the titles about to be
    /// deleted; uppercase `D` confirms, `Esc`/`n` cancels. Single-delete (no
    /// selection, just the cursor row) rides the same modal with one item.
    ConfirmDelete { ids: Vec<String> },
    /// Blocking progress modal while a batch runs (spec §6.1/§6.5).
    Running { op: Op },
    /// Partial-failure result face (spec §6.6): lists failed ids + stderr; `d`
    /// retries the still-selected failures, `Esc` closes.
    Result { op: Op },
    /// Complete v1 key table. Any key closes it (spec §5.7).
    Help,
}

/// Why the current view has no rows, so the UI can pick the right guidance
/// (spec §4.6 / §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmptyReason {
    /// A committed/live search matched nothing in this scope (spec §4.6).
    SearchNoMatch,
    /// Scope=Project, Active, nothing here — offer switching to all projects.
    ProjectEmpty,
    /// Lifecycle=Archived, nothing archived in this scope.
    ArchivedEmpty,
    /// Scope=All, Active, and genuinely no sessions at all.
    NoSessions,
}

/// Live progress of the in-flight batch, rendered by the progress modal (spec
/// §6.5) and consumed to build the result face (spec §6.6). `total` is fixed at
/// launch; the rest accumulate as outcomes stream in.
#[derive(Debug, Default, Clone)]
pub struct BatchProgress {
    /// Every id in this batch, in dispatch order — the source of truth for the
    /// cancelled/unfired set (spec §6.8), which is `ids − succeeded − failed`.
    pub ids: Vec<String>,
    pub succeeded: Vec<String>,
    pub failed: Vec<(String, String)>,
    /// `Esc` pressed: dispatch stopped, in-flight workers left to finish (spec §6.8).
    pub cancelled: bool,
}

impl BatchProgress {
    /// Total ids in the batch.
    pub fn total(&self) -> usize {
        self.ids.len()
    }

    /// Ids resolved so far (success + failure); `total - done` are still in
    /// flight or, once cancelled, will never start.
    pub fn done(&self) -> usize {
        self.succeeded.len() + self.failed.len()
    }

    /// In-flight count for the `⟳ z` aggregate (spec §6.5). Zero once cancelled
    /// (no more dispatch) or complete.
    pub fn in_flight(&self) -> usize {
        self.total().saturating_sub(self.done())
    }

    /// Ids that never ran because dispatch was cancelled (spec §6.8): the batch
    /// set minus everything that resolved. Meaningful only after a cancelled
    /// batch finishes; empty on a batch that ran to completion.
    pub fn cancelled_ids(&self) -> Vec<String> {
        let resolved: HashSet<&str> = self
            .succeeded
            .iter()
            .map(String::as_str)
            .chain(self.failed.iter().map(|(id, _)| id.as_str()))
            .collect();
        self.ids
            .iter()
            .filter(|id| !resolved.contains(id.as_str()))
            .cloned()
            .collect()
    }
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
    /// Case-insensitive substring filter over `title` + `first_user_message`
    /// (spec §4.3). Empty string means search is off. Applied in memory over
    /// `all_rows` (spec §4.2 seam).
    pub search: String,
    /// Input mode (spec §5.5); gates whether printable keys edit `search` or act
    /// as filter/selection keys.
    pub mode: Mode,
    /// Multi-selection, keyed by session id so it survives filter re-orders and
    /// refreshes (spec §4.5 / §5.3). Hidden-but-selected rows stay in the set.
    pub selected: HashSet<String>,
    /// User's preview-panel intent (spec §5.1); the UI additionally auto-hides
    /// it when the terminal is narrower than 100 columns.
    pub show_preview: bool,
    /// Transient footer message (e.g. a busy re-query that kept stale rows).
    pub message: Option<String>,
    /// Startup PATH probe. A missing `traex` does not block read-only use, but
    /// the footer keeps mutation unavailability visible (spec §11).
    pub traex_available: bool,
    /// The in-flight batch's outcome stream + cancel switch, live only during
    /// `Mode::Running` (spec §6.1). Kept off `Mode` because it is neither
    /// comparable nor cloneable; the visible progress counters live in
    /// [`BatchProgress`] instead.
    batch: Option<BatchHandle>,
    /// Live progress of the running (or just-finished) batch (spec §6.5/§6.6).
    pub progress: BatchProgress,
    /// How each op actually runs (spec §6.4). Production spawns `traex`; tests
    /// inject a deterministic runner.
    runner: Runner,
    /// Independent SQLite write path for the one direct mutation tsm owns
    /// (`threads.title`, spec §2.6/§7).
    rename_runner: RenameRunner,
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
        Self::with_runner_and_availability(
            store,
            cwd,
            initial_rows,
            mutate::traex_runner(),
            mutate::traex_available(),
        )
    }

    /// Build an app with an explicit batch runner (production passes
    /// [`mutate::traex_runner`]; tests inject a deterministic one).
    #[cfg(test)]
    pub fn with_runner(
        store: Store,
        cwd: String,
        initial_rows: Vec<Session>,
        runner: Runner,
    ) -> Self {
        let rename_runner = rename::runner(store.db_path().to_path_buf());
        Self::with_runners_and_availability(store, cwd, initial_rows, runner, rename_runner, true)
    }

    #[cfg(test)]
    fn with_rename_runner(
        store: Store,
        cwd: String,
        initial_rows: Vec<Session>,
        rename_runner: RenameRunner,
    ) -> Self {
        Self::with_runners_and_availability(
            store,
            cwd,
            initial_rows,
            std::sync::Arc::new(|_, _| None),
            rename_runner,
            true,
        )
    }

    /// Build an app with explicit mutation execution and PATH-probe state.
    pub fn with_runner_and_availability(
        store: Store,
        cwd: String,
        initial_rows: Vec<Session>,
        runner: Runner,
        traex_available: bool,
    ) -> Self {
        let rename_runner = rename::runner(store.db_path().to_path_buf());
        Self::with_runners_and_availability(
            store,
            cwd,
            initial_rows,
            runner,
            rename_runner,
            traex_available,
        )
    }

    fn with_runners_and_availability(
        store: Store,
        cwd: String,
        initial_rows: Vec<Session>,
        runner: Runner,
        rename_runner: RenameRunner,
        traex_available: bool,
    ) -> Self {
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
            search: String::new(),
            mode: Mode::Normal,
            selected: HashSet::new(),
            show_preview: true,
            message: None,
            traex_available,
            batch: None,
            progress: BatchProgress::default(),
            runner,
            rename_runner,
            should_quit: false,
        };
        app.rebuild_view();
        app
    }

    /// Recompute `view` from `all_rows`, applying the in-memory search filter
    /// (spec §4.2 seam). `all_rows` is already scope×lifecycle-filtered and
    /// sorted by the query; search is a case-insensitive substring test over
    /// `title` + `first_user_message` (spec §4.3, non-fuzzy).
    pub fn rebuild_view(&mut self) {
        if self.search.is_empty() {
            self.view = (0..self.all_rows.len()).collect();
        } else {
            let needle = self.search.to_lowercase();
            self.view = self
                .all_rows
                .iter()
                .enumerate()
                .filter(|(_, s)| session_matches(s, &needle))
                .map(|(i, _)| i)
                .collect();
        }
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

    pub fn rename_display(&self, session_id: &str) -> Option<String> {
        let Mode::Rename { id, buf, cursor } = &self.mode else {
            return None;
        };
        if id != session_id {
            return None;
        }
        let byte = char_to_byte(buf, *cursor);
        let mut display = String::with_capacity(buf.len() + "▏".len());
        display.push_str(&buf[..byte]);
        display.push('▏');
        display.push_str(&buf[byte..]);
        Some(display)
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

    /// Open the complete key-reference overlay (`?`, spec §5.7).
    pub fn show_help(&mut self) {
        self.mode = Mode::Help;
    }

    /// Close the help overlay on any key.
    pub fn dismiss_help(&mut self) {
        self.mode = Mode::Normal;
    }

    // --- Search (spec §4.4) ------------------------------------------------

    /// Enter Search mode (`/`), keeping any current term so `/` re-opens editing
    /// with the committed word (spec §4.4).
    pub fn enter_search(&mut self) {
        self.mode = Mode::Search;
    }

    /// Append a typed character to the search term and re-filter live (spec §4.4).
    pub fn search_push(&mut self, c: char) {
        self.search.push(c);
        self.rebuild_view();
    }

    /// Delete the last character of the search term and re-filter (spec §4.4).
    pub fn search_backspace(&mut self) {
        self.search.pop();
        self.rebuild_view();
    }

    /// Commit the search: keep the filter, return to Normal (`Enter` in Search).
    /// The filtered set stays live so `*`/`Space`/`d` act on it (spec §4.4).
    pub fn search_commit(&mut self) {
        self.mode = Mode::Normal;
    }

    /// Clear the search term and return to Normal. Used by `Esc` in Search mode
    /// and by `Esc` in Normal when a committed filter is present (spec §4.4).
    pub fn search_clear(&mut self) {
        self.search.clear();
        self.mode = Mode::Normal;
        self.rebuild_view();
    }

    // --- Rename (spec §7) ----------------------------------------------------

    pub fn start_rename(&mut self) {
        let Some(session) = self.selected_session() else {
            return;
        };
        let id = session.id.clone();
        let buf = session.title.clone();
        let cursor = buf.chars().count();
        self.message = None;
        self.mode = Mode::Rename { id, buf, cursor };
    }

    pub fn cancel_rename(&mut self) {
        self.mode = Mode::Normal;
        self.message = None;
    }

    pub fn rename_insert(&mut self, character: char) {
        let Mode::Rename { buf, cursor, .. } = &mut self.mode else {
            return;
        };
        let byte = char_to_byte(buf, *cursor);
        buf.insert(byte, character);
        *cursor += 1;
    }

    pub fn rename_left(&mut self) {
        if let Mode::Rename { cursor, .. } = &mut self.mode {
            *cursor = cursor.saturating_sub(1);
        }
    }

    pub fn rename_right(&mut self) {
        if let Mode::Rename { buf, cursor, .. } = &mut self.mode {
            *cursor = (*cursor + 1).min(buf.chars().count());
        }
    }

    pub fn rename_home(&mut self) {
        if let Mode::Rename { cursor, .. } = &mut self.mode {
            *cursor = 0;
        }
    }

    pub fn rename_end(&mut self) {
        if let Mode::Rename { buf, cursor, .. } = &mut self.mode {
            *cursor = buf.chars().count();
        }
    }

    pub fn rename_backspace(&mut self) {
        let Mode::Rename { buf, cursor, .. } = &mut self.mode else {
            return;
        };
        if *cursor == 0 {
            return;
        }
        let start = char_to_byte(buf, *cursor - 1);
        let end = char_to_byte(buf, *cursor);
        buf.replace_range(start..end, "");
        *cursor -= 1;
    }

    pub fn rename_delete(&mut self) {
        let Mode::Rename { buf, cursor, .. } = &mut self.mode else {
            return;
        };
        if *cursor >= buf.chars().count() {
            return;
        }
        let start = char_to_byte(buf, *cursor);
        let end = char_to_byte(buf, *cursor + 1);
        buf.replace_range(start..end, "");
    }

    pub fn submit_rename(&mut self) {
        let (id, buf) = match &self.mode {
            Mode::Rename { id, buf, .. } => (id.clone(), buf.clone()),
            _ => return,
        };
        let Some(title) = rename::normalize_title(&buf) else {
            self.message = Some("标题不能为空".to_string());
            return;
        };

        match (self.rename_runner)(&id, &title) {
            Ok(RenameOutcome::Renamed) => {
                self.mode = Mode::Normal;
                if self.restore_after_rename(Some(&id)) {
                    self.message = Some("已重命名".to_string());
                }
            }
            Ok(RenameOutcome::Missing) => {
                self.mode = Mode::Normal;
                if self.restore_after_rename(None) {
                    self.message = Some("会话已不存在,可能已在别处删除".to_string());
                }
            }
            Err(RenameError::Busy) => {
                self.message = Some("库忙,请重试".to_string());
            }
            Err(RenameError::Other(error)) => {
                self.message = Some(format!("重命名失败: {error}"));
            }
        }
    }

    fn restore_after_rename(&mut self, anchor: Option<&str>) -> bool {
        let scope_cwd = match self.scope {
            Scope::Project => Some(self.cwd.as_str()),
            Scope::All => None,
        };
        let archived = matches!(self.lifecycle, Lifecycle::Archived);
        match self.store.query(scope_cwd, archived) {
            Ok(rows) => {
                self.all_rows = rows;
                self.rebuild_view();
                self.prune_selection();
                self.restore_cursor(anchor);
                true
            }
            Err(error) => {
                self.message = Some(runtime_query_message(&error));
                false
            }
        }
    }

    // --- Multi-selection (spec §4.5 / §5.3) --------------------------------

    /// Toggle selection of the cursor row (`Space`), keyed by session id so the
    /// choice survives filter re-orders (spec §5.3).
    pub fn toggle_selected(&mut self) {
        if let Some(id) = self.selected_session().map(|s| s.id.clone()) {
            self.toggle_id(id);
        }
    }

    /// Invert selection over the currently visible (filtered) set (`*`, spec
    /// §5.3). Hidden-but-selected rows are untouched (spec §4.5).
    pub fn invert_visible_selection(&mut self) {
        let visible_ids: Vec<String> = self.visible_sessions().map(|s| s.id.clone()).collect();
        for id in visible_ids {
            self.toggle_id(id);
        }
    }

    /// Flip one id's membership in the selection set.
    fn toggle_id(&mut self, id: String) {
        if !self.selected.remove(&id) {
            self.selected.insert(id);
        }
    }

    /// Whether the given session id is selected (for row rendering).
    pub fn is_selected(&self, id: &str) -> bool {
        self.selected.contains(id)
    }

    /// Count of selected sessions (footer `N selected`, spec §5.4).
    pub fn selected_count(&self) -> usize {
        self.selected.len()
    }

    // --- Delete / batch engine (spec §6) -----------------------------------

    /// The ids a batch op (`d`/`a`) will act on (spec §6.3): every selected
    /// session if any are selected, otherwise just the cursor row. Selection
    /// order is not meaningful, but the cursor-row fallback and the "list titles"
    /// modal need a stable order, so selected ids follow current view order
    /// (visible first), with hidden-but-selected rows appended.
    pub fn batch_targets(&self) -> Vec<String> {
        if self.selected.is_empty() {
            return self
                .selected_session()
                .map(|s| vec![s.id.clone()])
                .unwrap_or_default();
        }
        // Visible selected rows in view order, then any hidden selected rows
        // (spec §4.5: hidden-but-selected still count).
        let mut ids: Vec<String> = self
            .visible_sessions()
            .filter(|s| self.selected.contains(&s.id))
            .map(|s| s.id.clone())
            .collect();
        for id in &self.selected {
            if !ids.contains(id) {
                ids.push(id.clone());
            }
        }
        ids
    }

    /// `d`: open the delete-confirmation modal for the current targets (spec
    /// §6.3). A no-op when there is nothing to delete (empty view, no selection).
    pub fn request_delete(&mut self) {
        let ids = self.batch_targets();
        if ids.is_empty() {
            return;
        }
        self.mode = Mode::ConfirmDelete { ids };
    }

    /// The archive op for the current lifecycle view (spec §3.2 / §5.7): `a`
    /// archives in the Active view and unarchives in the Archived view. The view
    /// itself gates against traex's non-idempotent hard error — Active rows only
    /// ever archive, Archived rows only ever unarchive.
    pub fn archive_op(&self) -> Op {
        match self.lifecycle {
            Lifecycle::Active => Op::Archive,
            Lifecycle::Archived => Op::Unarchive,
        }
    }

    /// `a`: archive (Active view) or unarchive (Archived view) the current
    /// targets. Reversible, so it is **confirmation-free and fires immediately**
    /// (spec §6.3) — straight into the batch pipeline, no `ConfirmDelete` gate.
    /// A no-op when there is nothing to act on (empty view, no selection).
    pub fn request_archive(&mut self) {
        let ids = self.batch_targets();
        if ids.is_empty() {
            return;
        }
        self.start_batch(self.archive_op(), ids);
    }

    /// The title to show for an id in the confirm modal, falling back through
    /// `first_user_message` to `(untitled)` — mirrors the list's display rule.
    pub fn title_for(&self, id: &str) -> String {
        match self.all_rows.iter().find(|s| s.id == id) {
            Some(s) => crate::format::session_display(&s.title, &s.first_user_message),
            None => id.to_string(),
        }
    }

    /// `Esc`/`n` in the confirm modal: back to Normal, selection untouched.
    pub fn cancel_confirm(&mut self) {
        self.mode = Mode::Normal;
    }

    /// `D` in the confirm modal: launch the delete batch (spec §6.1/§6.4). The
    /// confirmed ids drive a `BatchJob`; the fan-out pool starts immediately and
    /// the modal switches to the blocking progress view.
    pub fn confirm_delete(&mut self) {
        let ids = match &self.mode {
            Mode::ConfirmDelete { ids } => ids.clone(),
            _ => return,
        };
        self.start_batch(Op::Delete, ids);
    }

    /// Kick off a batch for `op` over `ids`, moving into `Mode::Running` (spec
    /// §6.2). Shared by delete-confirm and (ticket 06) archive/unarchive, and by
    /// retry (spec §6.6).
    pub fn start_batch(&mut self, op: Op, ids: Vec<String>) {
        if ids.is_empty() {
            return;
        }
        self.progress = BatchProgress {
            ids: ids.clone(),
            ..Default::default()
        };
        let job = BatchJob { op, ids };
        self.batch = Some(mutate::spawn(job, self.runner.clone()));
        self.mode = Mode::Running { op };
    }

    /// Drain any outcomes the workers have produced and fold them into
    /// `progress` (spec §6.5). Called each frame while `Mode::Running`; once the
    /// batch is finished, transitions to the result face or auto-closes (spec
    /// §6.6). Returns nothing; the caller re-renders from the new state.
    pub fn poll_batch(&mut self) {
        let Some(handle) = &self.batch else { return };
        let finished = handle.is_finished();
        for out in handle.drain_ready() {
            match out.error {
                None => self.progress.succeeded.push(out.id),
                Some(e) => self.progress.failed.push((out.id, e)),
            }
        }
        // Check `is_finished` *before* draining: a worker can send its last
        // outcome and then exit between the two calls, so ordering it this way
        // guarantees the final drain above already saw everything a "finished"
        // pool produced.
        if finished {
            self.finish_batch();
        }
    }

    /// `Esc` in the progress modal: stop dispatching, let in-flight finish (spec
    /// §6.8). The workers still stream their outcomes; `poll_batch` keeps folding
    /// them until the pool drains.
    pub fn cancel_batch(&mut self) {
        if let Some(handle) = &self.batch {
            handle.cancel();
            self.progress.cancelled = true;
        }
    }

    /// Batch is done (all workers exited): re-query the store once (spec §6.7),
    /// reconcile the selection (successes drop, failures stay, spec §6.6), and
    /// either auto-close on full success or show the result face.
    fn finish_batch(&mut self) {
        let op = match &self.mode {
            Mode::Running { op } => *op,
            _ => return,
        };
        self.batch = None;

        // Successes leave the selection; failures stay so `d` can retry them, and
        // cancelled/unfired ids also stay selected+retryable (spec §6.6/§6.8).
        for id in &self.progress.succeeded {
            self.selected.remove(id);
        }

        // Full re-query so archive's moved files / vanished rows are reflected
        // authoritatively (spec §6.7); also prunes `selected` of gone rows.
        let refreshed = self.refresh_after_mutation();

        // Full success on a batch that ran to completion → auto-close with a
        // toast (spec §6.6). A cancelled batch always shows the result face so
        // the user sees the unfired set and the `d`-retry hint (spec §6.8), even
        // if every dispatched worker happened to succeed.
        if self.progress.failed.is_empty() && !self.progress.cancelled {
            let n = self.progress.succeeded.len();
            if refreshed {
                self.message = Some(format!("{} {}.", op.past_verb(), n));
            }
            self.mode = Mode::Normal;
        } else {
            self.mode = Mode::Result { op };
        }
    }

    /// `d` on the result face: retry the still-selected failures **and** any
    /// cancelled/unfired ids that are still selected, through the same pipeline
    /// (spec §6.6/§6.8). If nothing is selected anymore, closes.
    pub fn retry_failed(&mut self) {
        let op = match &self.mode {
            Mode::Result { op } => *op,
            _ => return,
        };
        // Both failed and cancelled/unfired ids are retryable (spec §6.8). Dedup
        // by preserving first-seen order: failures first, then unfired.
        let mut ids: Vec<String> = Vec::new();
        for id in self
            .progress
            .failed
            .iter()
            .map(|(id, _)| id.clone())
            .chain(self.progress.cancelled_ids())
        {
            if self.selected.contains(&id) && !ids.contains(&id) {
                ids.push(id);
            }
        }
        if ids.is_empty() {
            self.mode = Mode::Normal;
            return;
        }
        self.start_batch(op, ids);
    }

    /// `Esc` on the result face: close back to Normal, leaving failures selected.
    pub fn dismiss_result(&mut self) {
        self.mode = Mode::Normal;
    }

    /// Re-query the current scope×lifecycle after a mutation and prune the
    /// selection of ids that no longer exist (spec §6.7). Unlike [`requery`],
    /// this keeps the cursor near where it was (by id) rather than resetting to
    /// the top, and preserves the transient message. A busy timeout is
    /// non-fatal.
    fn refresh_after_mutation(&mut self) -> bool {
        self.refresh_snapshot(true)
    }

    /// `R`: re-run the current scope×lifecycle query, preserving every UI
    /// dimension and restoring the cursor by session id (spec §5.7/§12).
    pub fn refresh(&mut self) {
        self.refresh_snapshot(true);
    }

    /// Re-query the current snapshot. Successful refreshes preserve search and
    /// selection by id; `keep_cursor` additionally anchors the cursor by id.
    /// Runtime `SQLITE_BUSY` is non-fatal and leaves `all_rows` untouched.
    fn refresh_snapshot(&mut self, keep_cursor: bool) -> bool {
        let anchor = keep_cursor
            .then(|| self.selected_session().map(|s| s.id.clone()))
            .flatten();
        let scope_cwd = match self.scope {
            Scope::Project => Some(self.cwd.as_str()),
            Scope::All => None,
        };
        let archived = matches!(self.lifecycle, Lifecycle::Archived);
        match self.store.query(scope_cwd, archived) {
            Ok(rows) => {
                self.all_rows = rows;
                self.rebuild_view();
                self.prune_selection();
                if keep_cursor {
                    self.restore_cursor(anchor.as_deref());
                } else {
                    self.reset_cursor();
                }
                self.message = None;
                true
            }
            Err(err) => {
                self.message = Some(runtime_query_message(&err));
                false
            }
        }
    }

    /// Drop selected ids that are no longer present in `all_rows` (spec §6.7:
    /// `selected` filters out vanished/changed rows).
    fn prune_selection(&mut self) {
        let present: HashSet<&str> = self.all_rows.iter().map(|s| s.id.as_str()).collect();
        self.selected.retain(|id| present.contains(id.as_str()));
    }

    /// Put the cursor back on `anchor` if it survives; otherwise clamp to a valid
    /// row so it never lands on an unrelated session.
    fn restore_cursor(&mut self, anchor: Option<&str>) {
        if self.view.is_empty() {
            self.table.select(None);
            return;
        }
        let idx = anchor
            .and_then(|id| self.view.iter().position(|&i| self.all_rows[i].id == id))
            .unwrap_or_else(|| self.table.selected().unwrap_or(0).min(self.view.len() - 1));
        self.table.select(Some(idx));
    }

    /// Re-run the query for the current scope×lifecycle and reset the cursor to
    /// the top (spec §2.5 / §4.2). A busy timeout is non-fatal: keep the stale
    /// rows and surface a footer message (spec §11 runtime rule).
    fn requery(&mut self) {
        self.refresh_snapshot(false);
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
    ///
    /// Search-driven emptiness wins: if a term is active but nothing matched,
    /// the guidance is about the search, not the scope/lifecycle band.
    pub fn empty_reason(&self) -> EmptyReason {
        if !self.search.is_empty() {
            return EmptyReason::SearchNoMatch;
        }
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

/// Runtime query failures use the actionable busy toast from spec §11 while
/// retaining useful detail for non-busy errors.
fn runtime_query_message(err: &anyhow::Error) -> String {
    if crate::store::is_busy_error(err) {
        "库忙,按 R 重试".to_string()
    } else {
        err.to_string()
    }
}

fn char_to_byte(text: &str, character_index: usize) -> usize {
    text.char_indices()
        .nth(character_index)
        .map(|(byte, _)| byte)
        .unwrap_or(text.len())
}

/// Case-insensitive substring match of `needle` against a session's `title` +
/// `first_user_message` (spec §4.3). `needle` must already be lowercased.
fn session_matches(s: &Session, needle: &str) -> bool {
    s.title.to_lowercase().contains(needle) || s.first_user_message.to_lowercase().contains(needle)
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
                source TEXT NOT NULL DEFAULT 'cli',
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

    fn app_with_rename_runner(n: usize, rename_runner: RenameRunner) -> (App, std::path::PathBuf) {
        let path = unique_db();
        let _ = std::fs::remove_file(&path);
        seed(&path, n);
        let store = Store::open(path.clone()).unwrap();
        let rows = store.query_project_active("/proj").unwrap();
        (
            App::with_rename_runner(store, "/proj".to_string(), rows, rename_runner),
            path,
        )
    }

    /// Like [`app_with`], but injects a batch runner that actually mutates the
    /// backing DB so the post-batch re-query (spec §6.7) reflects real changes.
    /// The runner deletes the row from `path` on "success"; ids listed in
    /// `fail_ids` return a canned error and leave the row in place.
    fn app_with_batch(n: usize, fail_ids: &[&str]) -> App {
        let path = unique_db();
        let _ = std::fs::remove_file(&path);
        seed(&path, n);
        let store = Store::open(path.clone()).unwrap();
        let rows = store.query_project_active("/proj").unwrap();
        let fails: HashSet<String> = fail_ids.iter().map(|s| s.to_string()).collect();
        let db = path.clone();
        let runner: Runner = std::sync::Arc::new(move |_op, id| {
            if fails.contains(id) {
                return Some(format!("Error: boom {id}"));
            }
            let conn = Connection::open(&db).unwrap();
            conn.execute("DELETE FROM threads WHERE id = ?1", rusqlite::params![id])
                .unwrap();
            None
        });
        App::with_runner(store, "/proj".to_string(), rows, runner)
    }

    /// Poll the batch to completion (workers are real threads).
    fn drive(app: &mut App) {
        for _ in 0..500 {
            if !matches!(app.mode, Mode::Running { .. }) {
                return;
            }
            app.poll_batch();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        panic!("batch did not finish");
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

    // --- Search (ticket 03) ------------------------------------------------

    #[test]
    fn search_filters_title_case_insensitively() {
        // /proj active rows are titled "0","1"; add a searchable one via /elsewhere? No —
        // use the All view where titles like "OactTitle" exist.
        let mut app = app_with(2);
        app.toggle_scope(); // All · Active: p0,p1,oact
        app.enter_search();
        for c in "oact".chars() {
            app.search_push(c);
        }
        let ids: Vec<_> = app.visible_sessions().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["oact"]);
        // Case-insensitive: uppercasing the needle still matches.
        app.search_clear();
        app.enter_search();
        for c in "OACT".chars() {
            app.search_push(c);
        }
        let ids: Vec<_> = app.visible_sessions().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["oact"]);
    }

    #[test]
    fn search_matches_first_user_message() {
        let mut app = app_with(2);
        // Seed has empty first_user_message for p*, so inject via all_rows directly.
        app.all_rows[0].first_user_message = "hello WORLD dump".to_string();
        app.enter_search();
        for c in "world".chars() {
            app.search_push(c);
        }
        assert_eq!(app.view.len(), 1);
        assert_eq!(
            app.visible_sessions().next().unwrap().id,
            app.all_rows[0].id
        );
    }

    #[test]
    fn search_commit_keeps_filter_esc_clears() {
        let mut app = app_with(2);
        app.toggle_scope();
        app.enter_search();
        for c in "oact".chars() {
            app.search_push(c);
        }
        app.search_commit();
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.search, "oact");
        assert_eq!(app.view.len(), 1); // filter still live in Normal
        app.search_clear();
        assert_eq!(app.search, "");
        assert_eq!(app.view.len(), 3); // full All·Active set restored
    }

    #[test]
    fn search_backspace_re_widens() {
        let mut app = app_with(2);
        app.toggle_scope();
        app.enter_search();
        for c in "oactx".chars() {
            app.search_push(c);
        }
        assert!(app.view.is_empty());
        app.search_backspace(); // "oact"
        assert_eq!(app.view.len(), 1);
    }

    #[test]
    fn search_no_match_empty_reason() {
        let mut app = app_with(2);
        app.enter_search();
        for c in "zzz".chars() {
            app.search_push(c);
        }
        assert!(app.view.is_empty());
        assert_eq!(app.empty_reason(), EmptyReason::SearchNoMatch);
    }

    // --- Multi-selection (ticket 03) ---------------------------------------

    #[test]
    fn space_toggles_cursor_row_by_id() {
        let mut app = app_with(3); // cursor on p2
        app.toggle_selected();
        assert!(app.is_selected("p2"));
        assert_eq!(app.selected_count(), 1);
        app.toggle_selected();
        assert!(!app.is_selected("p2"));
        assert_eq!(app.selected_count(), 0);
    }

    #[test]
    fn invert_visible_toggles_filtered_set_only() {
        let mut app = app_with(3);
        app.invert_visible_selection(); // select p0,p1,p2
        assert_eq!(app.selected_count(), 3);
        // Narrow with search to just one row, then invert: only visible row flips.
        app.enter_search();
        for c in "2".chars() {
            app.search_push(c);
        }
        // Title of p2 is "2"; only p2 visible.
        assert_eq!(app.view.len(), 1);
        app.invert_visible_selection(); // p2 flips off; p0,p1 untouched (hidden)
        assert!(!app.is_selected("p2"));
        assert!(app.is_selected("p0"));
        assert!(app.is_selected("p1"));
        assert_eq!(app.selected_count(), 2);
    }

    #[test]
    fn selection_survives_filter_and_scope_changes() {
        let mut app = app_with(2); // p0,p1
        app.toggle_selected(); // select cursor (p1, newest)
        let sel_id = "p1";
        assert!(app.is_selected(sel_id));
        // Switch scope to All and back — selection is by id, silently preserved.
        app.toggle_scope();
        assert!(app.is_selected(sel_id));
        app.toggle_scope();
        assert!(app.is_selected(sel_id));
        // Search hides it, but it stays in the set (spec §4.5, silent retain).
        app.enter_search();
        for c in "zzz".chars() {
            app.search_push(c);
        }
        assert!(app.view.is_empty());
        assert!(app.is_selected(sel_id));
        app.search_clear();
        assert!(app.is_selected(sel_id));
    }

    // --- Runtime robustness / refresh / help (ticket 07) -------------------

    #[test]
    fn manual_refresh_preserves_filters_selection_and_cursor_by_id() {
        let path = unique_db();
        let _ = std::fs::remove_file(&path);
        seed(&path, 3);
        let store = Store::open(path.clone()).unwrap();
        let rows = store.query_project_active("/proj").unwrap();
        let mut app = App::new(store, "/proj".to_string(), rows);

        app.toggle_scope(); // All · Active
        app.enter_search();
        app.search_push('p');
        app.search_commit();
        app.cursor_down();
        let anchor = app.selected_session().unwrap().id.clone();
        app.toggle_selected();

        let conn = Connection::open(&path).unwrap();
        conn.execute(
            "INSERT INTO threads (id,title,cwd,updated_at,updated_at_ms,archived,tokens_used)
             VALUES ('p-new','p-new','/proj',999,999999,0,0)",
            [],
        )
        .unwrap();
        drop(conn);

        app.refresh();

        assert_eq!(app.scope, Scope::All);
        assert_eq!(app.lifecycle, Lifecycle::Active);
        assert_eq!(app.search, "p");
        assert_eq!(app.mode, Mode::Normal);
        assert!(app.is_selected(&anchor));
        assert_eq!(app.selected_session().unwrap().id, anchor);
        assert!(app.all_rows.iter().any(|session| session.id == "p-new"));
    }

    #[test]
    fn external_changes_are_invisible_until_manual_refresh() {
        let path = unique_db();
        let _ = std::fs::remove_file(&path);
        seed(&path, 1);
        let store = Store::open(path.clone()).unwrap();
        let rows = store.query_project_active("/proj").unwrap();
        let mut app = App::new(store, "/proj".to_string(), rows);

        let conn = Connection::open(&path).unwrap();
        conn.execute(
            "INSERT INTO threads (id,title,cwd,updated_at,updated_at_ms,archived,tokens_used)
             VALUES ('external','external','/proj',999,999999,0,0)",
            [],
        )
        .unwrap();
        drop(conn);

        assert!(!app.all_rows.iter().any(|session| session.id == "external"));
        app.refresh();
        assert!(app.all_rows.iter().any(|session| session.id == "external"));
    }

    #[test]
    fn help_opens_and_dismisses_without_changing_view_state() {
        let mut app = app_with(2);
        let rows = app.all_rows.len();
        let cursor = app.table.selected();
        app.show_help();
        assert_eq!(app.mode, Mode::Help);
        app.dismiss_help();
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.all_rows.len(), rows);
        assert_eq!(app.table.selected(), cursor);
    }

    #[test]
    fn runtime_busy_message_is_actionable() {
        let err = anyhow::anyhow!("traex database is busy · try again");
        assert_eq!(runtime_query_message(&err), "库忙,按 R 重试");
        assert_eq!(
            runtime_query_message(&anyhow::anyhow!("disk I/O error")),
            "disk I/O error"
        );
    }

    // --- Rename (ticket 04) --------------------------------------------------

    #[test]
    fn rename_starts_from_raw_title_and_ignores_multi_selection() {
        let mut app = app_with(3);
        app.invert_visible_selection();
        let cursor_id = app.selected_session().unwrap().id.clone();
        let original = app.selected_session().unwrap().title.clone();

        app.start_rename();

        match &app.mode {
            Mode::Rename { id, buf, cursor } => {
                assert_eq!(id, &cursor_id);
                assert_eq!(buf, &original);
                assert_eq!(*cursor, original.chars().count());
            }
            other => panic!("expected Rename, got {other:?}"),
        }
        assert_eq!(app.selected_count(), 3);
    }

    #[test]
    fn rename_editor_handles_unicode_navigation_and_deletion() {
        let mut app = app_with(1);
        app.start_rename();
        app.rename_home();
        while matches!(&app.mode, Mode::Rename { buf, .. } if !buf.is_empty()) {
            app.rename_delete();
        }
        app.rename_insert('你');
        app.rename_insert('好');
        app.rename_left();
        app.rename_delete();
        app.rename_insert('界');
        app.rename_end();
        app.rename_backspace();

        match &app.mode {
            Mode::Rename { buf, cursor, .. } => {
                assert_eq!(buf, "你");
                assert_eq!(*cursor, 1);
            }
            other => panic!("expected Rename, got {other:?}"),
        }
    }

    #[test]
    fn empty_rename_is_rejected_without_leaving_edit_mode() {
        let mut app = app_with(1);
        let old_title = app.selected_session().unwrap().title.clone();
        app.start_rename();
        app.rename_home();
        while matches!(&app.mode, Mode::Rename { buf, .. } if !buf.is_empty()) {
            app.rename_delete();
        }
        app.rename_insert('\t');
        app.submit_rename();

        assert!(matches!(app.mode, Mode::Rename { .. }));
        assert_eq!(app.message.as_deref(), Some("标题不能为空"));
        assert_eq!(app.selected_session().unwrap().title, old_title);
    }

    #[test]
    fn successful_rename_refreshes_and_restores_cursor_by_id() {
        let mut app = app_with(3);
        app.cursor_down();
        let id = app.selected_session().unwrap().id.clone();
        app.start_rename();
        app.rename_home();
        while matches!(&app.mode, Mode::Rename { buf, .. } if !buf.is_empty()) {
            app.rename_delete();
        }
        for character in "  新标题\t第二段\n第三段  ".chars() {
            app.rename_insert(character);
        }
        app.submit_rename();

        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.selected_session().unwrap().id, id);
        assert_eq!(
            app.selected_session().unwrap().title,
            "新标题 第二段 第三段"
        );
        assert_eq!(app.message.as_deref(), Some("已重命名"));
    }

    #[test]
    fn cancelled_rename_discards_buffer() {
        let mut app = app_with(1);
        let original = app.selected_session().unwrap().title.clone();
        app.start_rename();
        app.rename_insert('x');
        app.cancel_rename();

        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.selected_session().unwrap().title, original);
    }

    #[test]
    fn missing_rename_target_refreshes_away_and_exits_editor() {
        let deleted_path = std::sync::Arc::new(std::sync::Mutex::new(None));
        let path_slot = std::sync::Arc::clone(&deleted_path);
        let runner: RenameRunner = std::sync::Arc::new(move |_id, _title| {
            let path = path_slot.lock().unwrap().clone().unwrap();
            Connection::open(path)
                .unwrap()
                .execute("DELETE FROM threads WHERE id = 'p0'", [])
                .unwrap();
            Ok(RenameOutcome::Missing)
        });
        let (mut app, path) = app_with_rename_runner(1, runner);
        *deleted_path.lock().unwrap() = Some(path);
        app.start_rename();
        app.submit_rename();

        assert_eq!(app.mode, Mode::Normal);
        assert!(app.all_rows.is_empty());
        assert_eq!(
            app.message.as_deref(),
            Some("会话已不存在,可能已在别处删除")
        );
    }

    #[test]
    fn busy_rename_keeps_buffer_cursor_and_editor_for_retry() {
        let runner: RenameRunner = std::sync::Arc::new(|_, _| Err(RenameError::Busy));
        let (mut app, _path) = app_with_rename_runner(1, runner);
        app.start_rename();
        app.rename_insert('x');
        let before = app.mode.clone();
        app.submit_rename();

        assert_eq!(app.mode, before);
        assert_eq!(app.message.as_deref(), Some("库忙,请重试"));
    }

    #[test]
    fn refresh_failure_after_write_is_not_hidden_by_success_toast() {
        let removed_path = std::sync::Arc::new(std::sync::Mutex::new(None));
        let path_slot = std::sync::Arc::clone(&removed_path);
        let runner: RenameRunner = std::sync::Arc::new(move |_id, _title| {
            let path = path_slot.lock().unwrap().clone().unwrap();
            std::fs::remove_file(path).unwrap();
            Ok(RenameOutcome::Renamed)
        });
        let (mut app, path) = app_with_rename_runner(1, runner);
        *removed_path.lock().unwrap() = Some(path);
        app.start_rename();
        app.submit_rename();

        assert_eq!(app.mode, Mode::Normal);
        assert_ne!(app.message.as_deref(), Some("已重命名"));
        assert!(
            app.message
                .as_deref()
                .is_some_and(|message| !message.is_empty())
        );
    }

    // --- Delete / batch engine (ticket 05) ---------------------------------

    #[test]
    fn delete_targets_selected_else_cursor() {
        let mut app = app_with(3); // cursor on p2 (newest)
        // No selection: just the cursor row.
        assert_eq!(app.batch_targets(), vec!["p2"]);
        // With a selection, targets are the selected set (cursor ignored).
        app.cursor_last(); // p0
        app.toggle_selected(); // select p0
        app.cursor_first(); // cursor back on p2, but p2 not selected
        assert_eq!(app.batch_targets(), vec!["p0"]);
    }

    #[test]
    fn request_delete_opens_confirm_with_titles() {
        let mut app = app_with(2);
        app.request_delete();
        match &app.mode {
            Mode::ConfirmDelete { ids } => assert_eq!(ids, &vec!["p1".to_string()]),
            other => panic!("expected ConfirmDelete, got {other:?}"),
        }
        // Single-delete rides the same modal with one item (spec §6.3).
        assert_eq!(app.title_for("p1"), "p1");
    }

    #[test]
    fn request_delete_noop_on_empty_view() {
        let mut app = app_with(0);
        app.request_delete();
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn cancel_confirm_returns_to_normal() {
        let mut app = app_with(2);
        app.request_delete();
        app.cancel_confirm();
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.all_rows.len(), 2); // nothing deleted
    }

    #[test]
    fn full_success_deletes_requeries_and_auto_closes() {
        let mut app = app_with_batch(3, &[]);
        app.invert_visible_selection(); // select p0,p1,p2
        app.request_delete();
        app.confirm_delete();
        drive(&mut app);
        // Auto-closed on full success with a toast (spec §6.6).
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.message.as_deref(), Some("Deleted 3."));
        // All rows gone from the store, selection cleared (spec §6.7).
        assert!(app.all_rows.is_empty());
        assert_eq!(app.selected_count(), 0);
    }

    #[test]
    fn partial_failure_shows_result_and_keeps_failures_selected() {
        let mut app = app_with_batch(3, &["p1"]); // p1 fails
        app.invert_visible_selection(); // select p0,p1,p2
        app.request_delete();
        app.confirm_delete();
        drive(&mut app);
        // Result face lists the failure (spec §6.6).
        match &app.mode {
            Mode::Result { op } => assert_eq!(*op, Op::Delete),
            other => panic!("expected Result, got {other:?}"),
        }
        assert_eq!(app.progress.failed.len(), 1);
        assert_eq!(app.progress.failed[0].0, "p1");
        // Successes removed from store + selection; failure stays selected.
        assert!(app.all_rows.iter().any(|s| s.id == "p1"));
        assert!(!app.all_rows.iter().any(|s| s.id == "p0"));
        assert!(app.is_selected("p1"));
        assert!(!app.is_selected("p0"));
    }

    #[test]
    fn retry_reruns_still_selected_failures() {
        let mut app = app_with_batch(2, &["p1"]);
        app.invert_visible_selection(); // p0,p1
        app.request_delete();
        app.confirm_delete();
        drive(&mut app);
        assert!(matches!(app.mode, Mode::Result { .. }));
        assert!(app.is_selected("p1"));
        // Retry re-runs the still-selected failure set (spec §6.6). The injected
        // runner keeps failing p1, so we land back on the result face with p1
        // still selected — proving retry re-ran the failure through the pipeline.
        app.retry_failed();
        drive(&mut app);
        assert!(matches!(app.mode, Mode::Result { .. }));
        assert!(app.is_selected("p1"));
    }

    #[test]
    fn retry_with_nothing_selected_closes() {
        let mut app = app_with_batch(2, &["p1"]);
        app.invert_visible_selection();
        app.request_delete();
        app.confirm_delete();
        drive(&mut app);
        // Deselect the failure, then retry: nothing to do, close.
        app.selected.remove("p1");
        app.retry_failed();
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn dismiss_result_keeps_failures_selected() {
        let mut app = app_with_batch(2, &["p1"]);
        app.invert_visible_selection();
        app.request_delete();
        app.confirm_delete();
        drive(&mut app);
        app.dismiss_result();
        assert_eq!(app.mode, Mode::Normal);
        assert!(app.is_selected("p1"));
    }

    // --- Archive / unarchive (ticket 06) -----------------------------------

    /// Like [`app_with_batch`], but the runner flips `archived` to match the op
    /// (archive → 1, unarchive → 0) so the post-batch re-query (spec §6.7)
    /// reflects the row leaving the current lifecycle view. `fail_ids` still
    /// return a canned error and leave the row untouched.
    fn app_with_archive_batch(fail_ids: &[&str]) -> App {
        let path = unique_db();
        let _ = std::fs::remove_file(&path);
        seed(&path, 2); // p0,p1 active in /proj; parch archived in /proj
        let store = Store::open(path.clone()).unwrap();
        let rows = store.query_project_active("/proj").unwrap();
        let fails: HashSet<String> = fail_ids.iter().map(|s| s.to_string()).collect();
        let db = path.clone();
        let runner: Runner = std::sync::Arc::new(move |op, id| {
            if fails.contains(id) {
                return Some(format!("Error: boom {id}"));
            }
            let archived = match op {
                Op::Archive => 1,
                Op::Unarchive => 0,
                Op::Delete => panic!("archive test runner got Delete"),
            };
            let conn = Connection::open(&db).unwrap();
            conn.execute(
                "UPDATE threads SET archived = ?1 WHERE id = ?2",
                rusqlite::params![archived, id],
            )
            .unwrap();
            None
        });
        App::with_runner(store, "/proj".to_string(), rows, runner)
    }

    #[test]
    fn archive_op_follows_lifecycle_view() {
        let mut app = app_with(1);
        assert_eq!(app.archive_op(), Op::Archive); // Active view
        app.toggle_lifecycle();
        assert_eq!(app.archive_op(), Op::Unarchive); // Archived view
    }

    #[test]
    fn archive_fires_immediately_without_confirm() {
        // `a` is reversible → no ConfirmDelete gate; it goes straight to Running
        // (spec §6.3). Use a gated runner so we can observe Running before it
        // finishes.
        let mut app = app_with_archive_batch(&[]);
        app.request_archive();
        // No confirm modal — either Running now or already finished on this fast
        // in-process runner; never ConfirmDelete.
        assert!(
            !matches!(app.mode, Mode::ConfirmDelete { .. }),
            "archive must not open a confirm modal (spec §6.3)"
        );
    }

    #[test]
    fn archive_single_cursor_row_leaves_active_view() {
        let mut app = app_with_archive_batch(&[]);
        // No selection: archive acts on the cursor row (p1, newest active).
        let target = app.selected_session().unwrap().id.clone();
        app.request_archive();
        drive(&mut app);
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.message.as_deref(), Some("Archived 1."));
        // Full re-query: the archived row is gone from the Active view (spec §6.7).
        assert!(!app.all_rows.iter().any(|s| s.id == target));
    }

    #[test]
    fn archive_batch_selected_all_leave_active_view() {
        let mut app = app_with_archive_batch(&[]);
        app.invert_visible_selection(); // select p0,p1
        app.request_archive();
        drive(&mut app);
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.message.as_deref(), Some("Archived 2."));
        assert!(app.all_rows.is_empty()); // both archived, gone from Active
        assert_eq!(app.selected_count(), 0); // successes de-selected + pruned
    }

    #[test]
    fn unarchive_from_archived_view_leaves_it() {
        let mut app = app_with_archive_batch(&[]);
        app.toggle_lifecycle(); // Project · Archived: parch
        assert_eq!(app.all_rows.len(), 1);
        assert_eq!(app.archive_op(), Op::Unarchive);
        app.request_archive(); // acts on cursor row parch
        drive(&mut app);
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.message.as_deref(), Some("Unarchived 1."));
        // parch flipped to active → gone from the Archived view (spec §6.7).
        assert!(app.all_rows.is_empty());
    }

    #[test]
    fn archive_partial_failure_shows_result_and_keeps_failure_selected() {
        let mut app = app_with_archive_batch(&["p1"]); // p1 fails to archive
        app.invert_visible_selection(); // p0,p1
        app.request_archive();
        drive(&mut app);
        match &app.mode {
            Mode::Result { op } => assert_eq!(*op, Op::Archive),
            other => panic!("expected Result, got {other:?}"),
        }
        assert_eq!(app.progress.failed.len(), 1);
        assert_eq!(app.progress.failed[0].0, "p1");
        // p0 archived (gone from Active view); p1 stays selected & retryable.
        assert!(!app.all_rows.iter().any(|s| s.id == "p0"));
        assert!(app.is_selected("p1"));
        assert!(!app.is_selected("p0"));
    }

    #[test]
    fn archive_noop_on_empty_view() {
        let mut app = app_with(0);
        app.request_archive();
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn cancel_lands_on_result_even_when_all_dispatched_succeed() {
        // A gated runner: workers block on a flag so we can cancel while they are
        // in-flight; released workers all succeed. This proves the spec §6.8
        // rule that a cancelled batch shows the result face even with zero
        // failures, and the unfired tail stays selected + retryable.
        let path = unique_db();
        let _ = std::fs::remove_file(&path);
        seed(&path, 0);
        let conn = Connection::open(&path).unwrap();
        for i in 0..20 {
            conn.execute(
                "INSERT INTO threads (id,title,cwd,updated_at,updated_at_ms,archived,tokens_used)
                 VALUES (?1,?1,'/proj',?2,?3,0,0)",
                rusqlite::params![format!("g{i}"), i as i64, i as i64 * 1000],
            )
            .unwrap();
        }
        drop(conn);
        let store = Store::open(path.clone()).unwrap();
        let rows = store.query_project_active("/proj").unwrap();

        let gate = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let db = path.clone();
        let g = std::sync::Arc::clone(&gate);
        let runner: Runner = std::sync::Arc::new(move |_op, id| {
            while !g.load(Ordering::Acquire) {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            let conn = Connection::open(&db).unwrap();
            conn.execute("DELETE FROM threads WHERE id = ?1", rusqlite::params![id])
                .unwrap();
            None
        });
        let mut app = App::with_runner(store, "/proj".to_string(), rows, runner);
        app.invert_visible_selection(); // select all 20
        app.request_delete();
        app.confirm_delete();
        // Workers are blocked on the gate; cancel, then release them.
        app.cancel_batch();
        gate.store(true, Ordering::Release);
        drive(&mut app);

        assert!(
            matches!(app.mode, Mode::Result { .. }),
            "mode = {:?}",
            app.mode
        );
        assert!(app.progress.cancelled);
        let unfired = app.progress.cancelled_ids();
        assert!(!unfired.is_empty(), "expected an unfired tail after cancel");
        for id in &unfired {
            assert!(app.is_selected(id), "unfired {id} should stay selected");
        }
        // `d` retries the still-selected unfired set through the pipeline.
        app.retry_failed();
        assert!(matches!(app.mode, Mode::Running { .. }));
    }

    #[test]
    fn progress_aggregates_report_in_flight() {
        let p = BatchProgress {
            ids: (0..10).map(|i| i.to_string()).collect(),
            succeeded: vec!["a".into(), "b".into()],
            failed: vec![("c".into(), "e".into())],
            cancelled: false,
        };
        assert_eq!(p.total(), 10);
        assert_eq!(p.done(), 3);
        assert_eq!(p.in_flight(), 7);
    }

    #[test]
    fn cancelled_ids_are_the_unfired_set() {
        let p = BatchProgress {
            ids: vec!["a".into(), "b".into(), "c".into(), "d".into()],
            succeeded: vec!["a".into()],
            failed: vec![("b".into(), "boom".into())],
            cancelled: true,
        };
        // c, d never resolved → cancelled/unfired (spec §6.8).
        let mut got = p.cancelled_ids();
        got.sort();
        assert_eq!(got, vec!["c".to_string(), "d".to_string()]);
    }
}
