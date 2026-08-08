//! UI — ratatui rendering (spec §5). Ticket 02 grows the single-table layout
//! into the two orthogonal filter dimensions: the All view gains a `cwd` column
//! and drops `tokens`, the Archived view swaps `tokens` for `archived_at`, a
//! bottom preview panel shows the cursor row, and the title bar reflects
//! `scope · lifecycle`. Ticket 07 adds the complete-keymap help overlay and
//! degraded-mode footer status.

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Clear, Gauge, Paragraph, Row, Table, Wrap};

use crate::app::{App, EmptyReason, Lifecycle, Mode, Scope};
use crate::format::{
    cwd_relative_home, format_tokens, format_updated, session_display, truncate_display,
    truncate_middle,
};

/// plasticine dark theme (spec §5.6).
mod theme {
    use ratatui::style::Color;
    pub const BG: Color = Color::Rgb(0x16, 0x16, 0x1e);
    pub const FG: Color = Color::Rgb(0xc0, 0xca, 0xf5);
    pub const DIM: Color = Color::Rgb(0x56, 0x5f, 0x89);
    pub const CYAN: Color = Color::Rgb(0x7d, 0xcf, 0xff);
    pub const PURPLE: Color = Color::Rgb(0xbb, 0x9a, 0xf7);
    /// Subtle band behind selected rows (spec §5.3), a lift off `BG`.
    pub const SELECT_BG: Color = Color::Rgb(0x2a, 0x2b, 0x3d);
    /// Success / failure / warning accents for the batch modals (spec §5.6/§6.5).
    pub const GREEN: Color = Color::Rgb(0x9e, 0xce, 0x6a);
    pub const RED: Color = Color::Rgb(0xf7, 0x76, 0x8e);
    pub const YELLOW: Color = Color::Rgb(0xe0, 0xaf, 0x68);
}

/// Terminal columns below which the preview panel auto-hides (spec §5.1).
const PREVIEW_MIN_WIDTH: u16 = 100;
/// Height of the bottom preview panel when shown (1 border + content rows).
const PREVIEW_HEIGHT: u16 = 8;

/// Fixed column widths (display columns); `session` flexes to fill the rest.
const W_CHECK: u16 = 1;
const W_UPDATED: u16 = 11; // "MM-DD HH:MM"
const W_MODEL: u16 = 16;
const W_TOKENS: u16 = 6;
const W_ARCHIVED_AT: u16 = 11; // "MM-DD HH:MM"
const W_CWD: u16 = 24; // relative-~ path, middle-truncated

/// Whether the preview panel is actually shown: user intent AND wide enough
/// (spec §5.1 narrow-screen degradation).
fn preview_visible(app: &App, width: u16) -> bool {
    app.show_preview && width >= PREVIEW_MIN_WIDTH
}

/// Render the whole frame.
pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    // Paint the background.
    f.render_widget(
        Block::default().style(Style::default().bg(theme::BG)),
        area,
    );

    let show_preview = preview_visible(app, area.width);
    let constraints = if show_preview {
        vec![
            Constraint::Length(1),                // title bar
            Constraint::Min(1),                   // table
            Constraint::Length(PREVIEW_HEIGHT),   // preview
            Constraint::Length(1),                // footer
        ]
    } else {
        vec![
            Constraint::Length(1), // title bar
            Constraint::Min(1),    // table
            Constraint::Length(1), // footer
        ]
    };
    let chunks = Layout::vertical(constraints).split(area);

    draw_title_bar(f, chunks[0], app);
    draw_table(f, chunks[1], app);
    if show_preview {
        draw_preview(f, chunks[2], app);
        draw_footer(f, chunks[3], app);
    } else {
        draw_footer(f, chunks[2], app);
    }

    // Batch modals overlay the list (spec §6): a centered floating block over
    // the whole frame (spec §5.5). Only one is active at a time.
    match &app.mode {
        Mode::ConfirmDelete { ids } => draw_confirm_delete(f, area, app, ids),
        Mode::Running { op } => draw_running(f, area, app, *op),
        Mode::Result { op } => draw_result(f, area, app, *op),
        Mode::Help => draw_help(f, area),
        _ => {}
    }
}

/// Center a `width`×`height` rect inside `area` for a modal overlay.
fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let [h] = Layout::horizontal([Constraint::Length(width.min(area.width))])
        .flex(Flex::Center)
        .areas(area);
    let [v] = Layout::vertical([Constraint::Length(height.min(area.height))])
        .flex(Flex::Center)
        .areas(h);
    v
}

/// Delete-confirmation modal (spec §6.3): title list (first 10 + "and M more"),
/// count, an irreversible-warning line, and the confirm/cancel hint.
fn draw_confirm_delete(f: &mut Frame, area: Rect, app: &App, ids: &[String]) {
    let n = ids.len();
    // Up to 10 titles, then an overflow line (spec §6.3).
    let shown = ids.len().min(10);
    let mut lines: Vec<Line> = Vec::new();
    for id in &ids[..shown] {
        lines.push(Line::from(vec![
            Span::styled("  • ", Style::default().fg(theme::DIM)),
            Span::styled(
                truncate_display(&app.title_for(id), area.width.saturating_sub(20) as usize),
                Style::default().fg(theme::FG),
            ),
        ]));
    }
    if n > shown {
        lines.push(Line::from(Span::styled(
            format!("  … and {} more", n - shown),
            Style::default().fg(theme::DIM),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!("Delete {n} session{} — irreversible.", plural(n)),
        Style::default().fg(theme::RED).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(Span::styled(
        "This also deletes the rollout files on disk.",
        Style::default().fg(theme::YELLOW),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("[D]", Style::default().fg(theme::RED).add_modifier(Modifier::BOLD)),
        Span::styled(" confirm   ", Style::default().fg(theme::DIM)),
        Span::styled("[Esc/n]", Style::default().fg(theme::PURPLE)),
        Span::styled(" cancel", Style::default().fg(theme::DIM)),
    ]));

    let height = (lines.len() as u16 + 2).min(area.height);
    let width = 64.min(area.width);
    let rect = centered_rect(area, width, height);
    let block = modal_block(" Confirm delete ", theme::RED);
    render_modal(f, rect, block, lines);
}

/// Progress modal (spec §6.5): `Deleting… N/total` + gauge, then the aggregate
/// `✓ x  ✗ y  ⟳ z` line, then a small failure list. Never renders per-item rows.
fn draw_running(f: &mut Frame, area: Rect, app: &App, op: crate::mutate::Op) {
    let p = &app.progress;
    let ratio = if p.total() == 0 {
        1.0
    } else {
        p.done() as f64 / p.total() as f64
    };
    let verb = op.progress_verb();
    let head = if p.cancelled {
        format!("{verb}… {}/{}  (finishing in-flight)", p.done(), p.total())
    } else {
        format!("{verb}… {}/{}", p.done(), p.total())
    };

    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(theme::CYAN).bg(theme::SELECT_BG))
        .ratio(ratio)
        .label(head);

    let counts = Line::from(vec![
        Span::styled(format!("✓ {}", p.succeeded.len()), Style::default().fg(theme::GREEN)),
        Span::raw("   "),
        Span::styled(format!("✗ {}", p.failed.len()), Style::default().fg(theme::RED)),
        Span::raw("   "),
        Span::styled(format!("⟳ {}", p.in_flight()), Style::default().fg(theme::YELLOW)),
    ]);

    // Small accumulating failure list at the bottom (spec §6.5), most recent last.
    let mut fail_lines = failure_lines(&p.failed, area.width);
    let mut body = vec![counts, Line::from("")];
    body.append(&mut fail_lines);
    body.push(Line::from(Span::styled(
        "[Esc] stop dispatching (in-flight finish)",
        Style::default().fg(theme::DIM),
    )));

    let width = 66.min(area.width);
    let height = (body.len() as u16 + 4).min(area.height);
    let rect = centered_rect(area, width, height);

    f.render_widget(Clear, rect);
    let block = modal_block(&format!(" {verb} "), theme::CYAN);
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    // Gauge on the first inner row, the rest below.
    let [gauge_area, body_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(inner);
    f.render_widget(gauge, gauge_area);
    f.render_widget(
        Paragraph::new(body).style(Style::default().bg(theme::BG)),
        body_area,
    );
}

/// Result face (spec §6.6): the failed ids + their stderr lines, and the retry
/// hint. Reached only when a batch had at least one failure.
fn draw_result(f: &mut Frame, area: Rect, app: &App, op: crate::mutate::Op) {
    let p = &app.progress;
    let cancelled = p.cancelled_ids();
    let mut lines = vec![Line::from(vec![
        Span::styled(
            format!("{} succeeded, ", p.succeeded.len()),
            Style::default().fg(theme::GREEN),
        ),
        Span::styled(
            format!("{} failed", p.failed.len()),
            Style::default().fg(theme::RED),
        ),
        Span::styled(
            if cancelled.is_empty() {
                String::new()
            } else {
                format!(", {} cancelled", cancelled.len())
            },
            Style::default().fg(theme::YELLOW),
        ),
    ])];
    lines.push(Line::from(""));
    lines.append(&mut failure_lines(&p.failed, area.width));
    // Cancelled/unfired ids are retryable too (spec §6.8); show them dim so the
    // user knows `d` will re-run them alongside the failures.
    for id in &cancelled {
        let short_id: String = id.chars().take(8).collect();
        lines.push(Line::from(vec![
            Span::styled(format!("  {short_id}  "), Style::default().fg(theme::DIM)),
            Span::styled("cancelled — not attempted", Style::default().fg(theme::YELLOW)),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("[d]", Style::default().fg(theme::CYAN).add_modifier(Modifier::BOLD)),
        Span::styled(" retry   ", Style::default().fg(theme::DIM)),
        Span::styled("[Esc]", Style::default().fg(theme::PURPLE)),
        Span::styled(" close", Style::default().fg(theme::DIM)),
    ]));

    let width = 72.min(area.width);
    let height = (lines.len() as u16 + 2).min(area.height);
    let rect = centered_rect(area, width, height);
    let title = if p.failed.is_empty() {
        format!(" {} — cancelled ", op.progress_verb())
    } else {
        format!(" {} — failures ", op.progress_verb())
    };
    let block = modal_block(&title, theme::RED);
    render_modal(f, rect, block, lines);
}

/// Complete v1 key reference from spec §5.7. Rename is included because it is
/// part of the finalized cross-ticket keymap, even while ticket 04 owns wiring.
fn draw_help(f: &mut Frame, area: Rect) {
    let rows = [
        ("j / ↓ · k / ↑", "move down / up"),
        ("g / G", "jump to top / bottom"),
        ("Space", "toggle current selection"),
        ("*", "invert visible selection"),
        ("d", "delete selected or current row"),
        ("a", "archive / unarchive in current lifecycle"),
        ("r", "rename current row"),
        ("/", "search"),
        ("Enter", "keep search / toggle preview"),
        ("Esc", "clear search / cancel or close"),
        ("p / Tab", "toggle scope / lifecycle"),
        ("R", "refresh from database"),
        ("?", "show this key reference"),
        ("q / Ctrl-c", "quit"),
        ("D · Esc / n", "confirm · cancel delete"),
    ];
    let mut lines = Vec::with_capacity(rows.len() + 2);
    for (key, action) in rows {
        lines.push(Line::from(vec![
            Span::styled(format!("{key:<17}"), Style::default().fg(theme::CYAN)),
            Span::styled(action, Style::default().fg(theme::FG)),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Press any key to close",
        Style::default().fg(theme::DIM),
    )));

    let rect = centered_rect(area, 62, lines.len() as u16 + 2);
    render_modal(f, rect, modal_block(" Help ", theme::PURPLE), lines);
}

/// Render the failure list shared by the progress modal and the result face:
/// one `id: Error…` line per failure (spec §6.5/§6.6), truncated to width.
fn failure_lines(failed: &[(String, String)], width: u16) -> Vec<Line<'static>> {
    let w = width.saturating_sub(6) as usize;
    failed
        .iter()
        .map(|(id, err)| {
            let short_id: String = id.chars().take(8).collect();
            Line::from(vec![
                Span::styled(format!("  {short_id}  "), Style::default().fg(theme::DIM)),
                Span::styled(truncate_display(err, w), Style::default().fg(theme::RED)),
            ])
        })
        .collect()
}

/// A centered modal block with a colored border and title.
fn modal_block(title: &str, border: ratatui::style::Color) -> Block<'static> {
    Block::default()
        .title(title.to_string())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border))
        .style(Style::default().bg(theme::BG))
}

/// Clear the area, draw `block`, and render `lines` inside it.
fn render_modal(f: &mut Frame, rect: Rect, block: Block, lines: Vec<Line>) {
    f.render_widget(Clear, rect);
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .style(Style::default().bg(theme::BG)),
        inner,
    );
}

/// `""`/`"s"` suffix for a count.
fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// `scope · lifecycle` label for the title bar (spec §5.4).
fn scope_lifecycle_label(app: &App) -> String {
    let scope = match app.scope {
        Scope::Project => "project",
        Scope::All => "all",
    };
    let lifecycle = match app.lifecycle {
        Lifecycle::Active => "active",
        Lifecycle::Archived => "archived",
    };
    format!("{scope} · {lifecycle}")
}

fn draw_title_bar(f: &mut Frame, area: Rect, app: &App) {
    // Search mode: the title row becomes the live input line `/term▏` with the
    // hit/total count on the right (spec §5.4). A committed filter (Normal mode
    // with a non-empty term) shows `/term  N/M` instead (spec §4.4).
    if app.mode == Mode::Search {
        let left = Span::styled(
            format!("/{}\u{258f}", app.search),
            Style::default().fg(theme::CYAN),
        );
        let right = Span::styled(
            format!("{}/{}", app.view.len(), app.all_rows.len()),
            Style::default().fg(theme::DIM),
        );
        draw_split_line(f, area, left, right);
        return;
    }

    let left = Span::styled(
        " tsm ",
        Style::default()
            .fg(theme::PURPLE)
            .add_modifier(Modifier::BOLD),
    );
    let right = if app.search.is_empty() {
        Span::styled(scope_lifecycle_label(app), Style::default().fg(theme::DIM))
    } else {
        Span::styled(
            format!("/{}  {}/{}", app.search, app.view.len(), app.all_rows.len()),
            Style::default().fg(theme::CYAN),
        )
    };
    draw_split_line(f, area, left, right);
}

/// Render a single line with `left` flush-left and `right` flush-right, padded
/// to fill `area`.
fn draw_split_line(f: &mut Frame, area: Rect, left: Span, right: Span) {
    let used = left.width() as u16 + right.width() as u16;
    let pad = area.width.saturating_sub(used) as usize;
    let line = Line::from(vec![left, Span::raw(" ".repeat(pad)), right]);
    f.render_widget(line.style(Style::default().bg(theme::BG)), area);
}

/// The column set for the current scope×lifecycle (spec §5.2).
///
/// - All view: insert `cwd` between `updated` and `session`, drop `tokens`.
/// - Archived view: put `archived_at` where `tokens` would be (or append it in
///   the All view, which has already dropped `tokens`).
fn draw_table(f: &mut Frame, area: Rect, app: &mut App) {
    let all_view = matches!(app.scope, Scope::All);
    let archived_view = matches!(app.lifecycle, Lifecycle::Archived);

    // Trailing metadata column: tokens (default), archived_at (archived), or
    // none (All + Active, which drops tokens for density).
    let trailing = if archived_view {
        Some(("archived", W_ARCHIVED_AT))
    } else if all_view {
        None
    } else {
        Some(("tokens", W_TOKENS))
    };

    // Sum the fixed-width columns to leave the rest for `session` (and `cwd`).
    let mut fixed = W_CHECK + W_UPDATED + W_MODEL + 1 /* leading pad guess */;
    if all_view {
        fixed += W_CWD;
    }
    if let Some((_, w)) = trailing {
        fixed += w;
    }
    // One column of spacing between each column.
    let n_cols = 3 + usize::from(all_view) + usize::from(trailing.is_some());
    fixed += (n_cols as u16).saturating_sub(1);
    let session_w = area.width.saturating_sub(fixed).max(4) as usize;

    let mut header_cells = vec![Cell::from(""), Cell::from("updated")];
    if all_view {
        header_cells.push(Cell::from("cwd"));
    }
    header_cells.push(Cell::from("session"));
    header_cells.push(Cell::from("model"));
    if let Some((label, _)) = trailing {
        header_cells.push(Cell::from(label));
    }
    let header = Row::new(header_cells)
        .style(Style::default().fg(theme::DIM).add_modifier(Modifier::BOLD));

    let home = app.home.clone();
    let rows: Vec<Row> = app
        .visible_sessions()
        .map(|s| {
            let session =
                truncate_display(&session_display(&s.title, &s.first_user_message), session_w);
            let model = truncate_display(s.model.as_deref().unwrap_or(""), W_MODEL as usize);

            // Checkbox: ▣ selected (cyan) / ▢ unselected (dim), spec §5.3.
            let selected = app.is_selected(&s.id);
            let check = if selected {
                Cell::from("▣").style(Style::default().fg(theme::CYAN))
            } else {
                Cell::from("▢").style(Style::default().fg(theme::DIM))
            };

            let mut cells = vec![check, Cell::from(format_updated(s.updated_at))];
            if all_view {
                let rel = cwd_relative_home(&s.cwd, &home);
                cells.push(Cell::from(truncate_middle(&rel, W_CWD as usize)));
            }
            cells.push(Cell::from(session));
            cells.push(Cell::from(model));
            if archived_view {
                let at = s.archived_at.map(format_updated).unwrap_or_default();
                cells.push(Cell::from(at));
            } else if !all_view {
                cells.push(Cell::from(format_tokens(s.tokens_used)));
            }
            // Selected rows carry a subtle highlight band (spec §5.3); the cursor
            // row's own highlight style still wins where they overlap.
            let row_style = if selected {
                Style::default().fg(theme::FG).bg(theme::SELECT_BG)
            } else {
                Style::default().fg(theme::FG)
            };
            Row::new(cells).style(row_style)
        })
        .collect();

    // Width constraints, matching the header/cell order above.
    let mut widths = vec![
        Constraint::Length(W_CHECK),
        Constraint::Length(W_UPDATED),
    ];
    if all_view {
        widths.push(Constraint::Length(W_CWD));
    }
    widths.push(Constraint::Min(4)); // session flexes
    widths.push(Constraint::Length(W_MODEL));
    if let Some((_, w)) = trailing {
        widths.push(Constraint::Length(w));
    }

    let table = Table::new(rows, widths)
        .header(header)
        .column_spacing(1)
        .row_highlight_style(
            Style::default()
                .bg(theme::CYAN)
                .fg(theme::BG)
                .add_modifier(Modifier::BOLD),
        )
        .style(Style::default().bg(theme::BG));

    f.render_stateful_widget(table, area, &mut app.table);
}

/// Bottom horizontal preview panel for the cursor row (spec §5.1).
fn draw_preview(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme::DIM))
        .style(Style::default().bg(theme::BG));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(s) = app.selected_session() else {
        let hint = Paragraph::new(Line::from(Span::styled(
            "no session selected",
            Style::default().fg(theme::DIM),
        )))
        .style(Style::default().bg(theme::BG));
        f.render_widget(hint, inner);
        return;
    };

    let id_w = (inner.width as usize).saturating_sub(6).max(8);
    let label = |k: &str| Span::styled(format!("{k:<8}"), Style::default().fg(theme::DIM));
    let val = |v: String| Span::styled(v, Style::default().fg(theme::FG));

    let mut lines = vec![
        Line::from(vec![
            label("title"),
            Span::styled(
                session_display(&s.title, &s.first_user_message),
                Style::default().fg(theme::CYAN),
            ),
        ]),
        Line::from(vec![label("id"), val(truncate_middle(&s.id, id_w))]),
        Line::from(vec![
            label("git"),
            val(s.git_branch.clone().unwrap_or_else(|| "—".to_string())),
            Span::raw("   "),
            label("model"),
            val(s.model.clone().unwrap_or_else(|| "—".to_string())),
            Span::raw("   "),
            label("tokens"),
            val(format_tokens(s.tokens_used)),
        ]),
        Line::from(vec![label("cwd"), val(s.cwd.clone())]),
    ];

    if !s.first_user_message.trim().is_empty() {
        lines.push(Line::from(vec![
            label("first"),
            val(truncate_display(&session_display("", &s.first_user_message), id_w)),
        ]));
    }

    let para = Paragraph::new(lines)
        .wrap(Wrap { trim: true })
        .style(Style::default().bg(theme::BG));
    f.render_widget(para, inner);
}

/// The empty-state guidance line for the current cause (spec §4.6 / §8). The
/// search-no-match case embeds the live term, so this takes the whole app.
fn empty_line(app: &App) -> String {
    match app.empty_reason() {
        EmptyReason::SearchNoMatch => format!(
            "no sessions match \"{}\" in this scope · clear search, or switch to all projects",
            app.search
        ),
        EmptyReason::ProjectEmpty => {
            "no sessions in this project · switch to all projects".to_string()
        }
        EmptyReason::ArchivedEmpty => {
            "no archived sessions in this scope · switch lifecycle back to active".to_string()
        }
        EmptyReason::NoSessions => "no sessions".to_string(),
    }
}

fn draw_footer(f: &mut Frame, area: Rect, app: &App) {
    // Left-slot precedence: a transient runtime message wins; then, when the
    // view is empty, the cause-specific guidance (spec §4.6) — this must beat
    // `N selected`, since the "search a batch → select all" flow (spec §4.4) can
    // filter down to an empty view while selections survive (spec §4.5); then
    // the selection count; then the plain row count.
    let left = if let Some(msg) = &app.message {
        Span::styled(msg.clone(), Style::default().fg(theme::PURPLE))
    } else if !app.traex_available {
        Span::styled(
            "traex not found · delete/archive/unarchive unavailable",
            Style::default().fg(theme::DIM),
        )
    } else if app.view.is_empty() {
        Span::styled(empty_line(app), Style::default().fg(theme::FG))
    } else if app.selected_count() > 0 {
        Span::styled(
            format!("{} selected", app.selected_count()),
            Style::default().fg(theme::CYAN),
        )
    } else {
        Span::styled(
            format!("{} sessions", app.view.len()),
            Style::default().fg(theme::FG),
        )
    };
    let hint = if app.mode == Mode::Search {
        "  ·  [type] filter  [Enter] keep  [Esc] clear".to_string()
    } else {
        // `a` reads as archive in the Active view, unarchive in the Archived
        // view (spec §5.7 lifecycle gating); label it from the op itself so the
        // footer can't drift from `archive_op`'s mapping.
        let archive_verb = match app.archive_op() {
            crate::mutate::Op::Unarchive => "unarchive",
            _ => "archive",
        };
        format!(
            "  ·  [j/k] move  [Space] select  [d] delete  [a] {archive_verb}  [/] search  [R] refresh  [?] help  [q] quit"
        )
    };
    let line = Line::from(vec![left, Span::styled(hint, Style::default().fg(theme::DIM))]);
    f.render_widget(line.style(Style::default().bg(theme::BG)), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mutate::Runner;
    use crate::store::Store;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use rusqlite::Connection;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn app(traex_available: bool) -> App {
        static N: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "tsm-ui-{}-{}.sqlite",
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
        let runner: Runner = Arc::new(|_, _| None);
        App::with_runner_and_availability(
            store,
            "/proj".to_string(),
            rows,
            runner,
            traex_available,
        )
    }

    fn render_text(app: &mut App) -> String {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn help_overlay_contains_complete_ticket_key_reference() {
        let mut app = app(true);
        app.show_help();
        let text = render_text(&mut app);

        for expected in [
            "j / ↓ · k / ↑",
            "Space",
            "d",
            "a",
            "r",
            "p / Tab",
            "R",
            "?",
            "q / Ctrl-c",
        ] {
            assert!(text.contains(expected), "missing help key {expected:?}");
        }
        assert!(text.contains("Press any key to close"));
    }

    #[test]
    fn missing_traex_notice_is_visible_without_blocking_render() {
        let mut app = app(false);
        let text = render_text(&mut app);
        assert!(text.contains(
            "traex not found · delete/archive/unarchive unavailable"
        ));
        assert!(text.contains("one"));
    }
}
