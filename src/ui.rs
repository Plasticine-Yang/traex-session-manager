//! UI — ratatui rendering (spec §5). Ticket 01: the single session table with
//! the default current-project columns and the plasticine dark theme.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Row, Table};

use crate::app::App;
use crate::format::{format_tokens, format_updated, session_display, truncate_display};

/// plasticine dark theme (spec §5.6).
mod theme {
    use ratatui::style::Color;
    pub const BG: Color = Color::Rgb(0x16, 0x16, 0x1e);
    pub const FG: Color = Color::Rgb(0xc0, 0xca, 0xf5);
    pub const DIM: Color = Color::Rgb(0x56, 0x5f, 0x89);
    pub const CYAN: Color = Color::Rgb(0x7d, 0xcf, 0xff);
    pub const PURPLE: Color = Color::Rgb(0xbb, 0x9a, 0xf7);
}

/// Fixed column widths (display columns); `session` flexes to fill the rest.
const W_CHECK: u16 = 1;
const W_UPDATED: u16 = 11; // "MM-DD HH:MM"
const W_MODEL: u16 = 16;
const W_TOKENS: u16 = 6;

/// Render the whole frame.
pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    // Paint the background.
    f.render_widget(
        Block::default().style(Style::default().bg(theme::BG)),
        area,
    );

    let chunks = Layout::vertical([
        Constraint::Length(1), // title bar
        Constraint::Min(1),    // table
        Constraint::Length(1), // footer
    ])
    .split(area);

    draw_title_bar(f, chunks[0], app);
    draw_table(f, chunks[1], app);
    draw_footer(f, chunks[2], app);
}

fn draw_title_bar(f: &mut Frame, area: Rect, _app: &App) {
    let left = Span::styled(
        " tsm ",
        Style::default()
            .fg(theme::PURPLE)
            .add_modifier(Modifier::BOLD),
    );
    let right = Span::styled("project · active", Style::default().fg(theme::DIM));
    let used = 5 + right.width() as u16;
    let pad = area.width.saturating_sub(used) as usize;
    let line = Line::from(vec![left, Span::raw(" ".repeat(pad)), right]);
    f.render_widget(line.style(Style::default().bg(theme::BG)), area);
}

fn draw_table(f: &mut Frame, area: Rect, app: &mut App) {
    // Width available to the flexible `session` column.
    let fixed: u16 = W_CHECK + W_UPDATED + W_MODEL + W_TOKENS + 4 /* column spacing */;
    let session_w = area.width.saturating_sub(fixed).max(4) as usize;

    let header = Row::new(vec![
        Cell::from(""),
        Cell::from("updated"),
        Cell::from("session"),
        Cell::from("model"),
        Cell::from("tokens"),
    ])
    .style(Style::default().fg(theme::DIM).add_modifier(Modifier::BOLD));

    let rows: Vec<Row> = app
        .visible_sessions()
        .map(|s| {
            let session = truncate_display(
                &session_display(&s.title, &s.first_user_message),
                session_w,
            );
            let model = truncate_display(s.model.as_deref().unwrap_or(""), W_MODEL as usize);
            Row::new(vec![
                Cell::from("·"),
                Cell::from(format_updated(s.updated_at)),
                Cell::from(session),
                Cell::from(model),
                Cell::from(format_tokens(s.tokens_used)),
            ])
            .style(Style::default().fg(theme::FG))
        })
        .collect();

    let widths = [
        Constraint::Length(W_CHECK),
        Constraint::Length(W_UPDATED),
        Constraint::Min(4),
        Constraint::Length(W_MODEL),
        Constraint::Length(W_TOKENS),
    ];

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

fn draw_footer(f: &mut Frame, area: Rect, app: &App) {
    let text = if app.all_rows.is_empty() {
        // Current-project empty state (spec §4.6 / §8).
        "no sessions in this project · switch to all projects".to_string()
    } else {
        format!("{} sessions", app.all_rows.len())
    };
    let hint = "  ·  [j/k] move  [g/G] top/bottom  [q] quit";
    let line = Line::from(vec![
        Span::styled(text, Style::default().fg(theme::FG)),
        Span::styled(hint, Style::default().fg(theme::DIM)),
    ]);
    f.render_widget(line.style(Style::default().bg(theme::BG)), area);
}
