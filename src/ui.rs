//! UI — ratatui rendering (spec §5). Ticket 02 grows the single-table layout
//! into the two orthogonal filter dimensions: the All view gains a `cwd` column
//! and drops `tokens`, the Archived view swaps `tokens` for `archived_at`, a
//! bottom preview panel shows the cursor row, and the title bar reflects
//! `scope · lifecycle`.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap};

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
        "  ·  [type] filter  [Enter] keep  [Esc] clear"
    } else {
        "  ·  [j/k] move  [Space] select  [*] invert  [/] search  [p] scope  [Tab] lifecycle  [q] quit"
    };
    let line = Line::from(vec![left, Span::styled(hint, Style::default().fg(theme::DIM))]);
    f.render_widget(line.style(Style::default().bg(theme::BG)), area);
}
