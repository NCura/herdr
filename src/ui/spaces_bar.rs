//! Horizontal spaces bar: workspaces rendered as chips on a single top row,
//! replacing the vertical sidebar. Names truncate to fit the available width.

use ratatui::{
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use super::status::state_dot;
use super::text::{display_width_u16, truncate_end};
use crate::app::state::WorkspaceCardArea;
use crate::app::{AppState, Mode};
use crate::terminal::TerminalRuntimeRegistry;

/// Minimum chip width when space is tight: dot + at least a couple of name cells.
const MIN_CHIP_WIDTH: u16 = 8;
/// Width reserved for the trailing new-space button (" + ").
const NEW_SPACE_WIDTH: u16 = 3;

/// Chip chrome around the name: leading pad, state dot, gap, trailing pad.
const CHIP_CHROME_WIDTH: u16 = 4;

fn natural_chip_width(name: &str) -> u16 {
    display_width_u16(name).saturating_add(CHIP_CHROME_WIDTH)
}

fn workspace_chip_labels(
    app: &AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
) -> Vec<String> {
    app.workspaces
        .iter()
        .map(|ws| ws.display_name_from(&app.terminals, terminal_runtimes))
        .collect()
}

/// Lay the workspaces out left-to-right in `area`. When the natural widths
/// overflow, every chip is capped to an equal share (never below
/// `MIN_CHIP_WIDTH`); chips that still don't fit are clipped at the right edge.
/// Returns the per-workspace hit rects plus the new-space button rect.
pub(crate) fn compute_spaces_bar_areas(
    app: &AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    area: Rect,
) -> (Vec<WorkspaceCardArea>, Rect) {
    if area.width == 0 || area.height == 0 || app.workspaces.is_empty() {
        return (Vec::new(), Rect::default());
    }

    let labels = workspace_chip_labels(app, terminal_runtimes);
    let new_space_reserved = if app.mouse_capture { NEW_SPACE_WIDTH } else { 0 };
    // The global menu launcher sits at the far right of the bar.
    let menu_reserved = if app.global_menu_attention_badge_visible() {
        8
    } else {
        6
    };
    let avail = area
        .width
        .saturating_sub(new_space_reserved)
        .saturating_sub(menu_reserved);

    let naturals: Vec<u16> = labels.iter().map(|l| natural_chip_width(l)).collect();
    let total: u16 = naturals
        .iter()
        .fold(0u16, |acc, w| acc.saturating_add(*w));
    let cap = if total <= avail {
        u16::MAX
    } else {
        (avail / labels.len().max(1) as u16).max(MIN_CHIP_WIDTH)
    };

    let mut cards = Vec::with_capacity(labels.len());
    let mut x = area.x;
    let right = area.x + avail;
    for (ws_idx, natural) in naturals.iter().enumerate() {
        let width = (*natural).min(cap).min(right.saturating_sub(x));
        cards.push(WorkspaceCardArea {
            ws_idx,
            rect: Rect::new(x, area.y, width, 1),
            indented: false,
        });
        x = x.saturating_add(width);
    }

    let new_space_hit_area = if app.mouse_capture {
        let chrome_right = (area.x + area.width).saturating_sub(menu_reserved);
        Rect::new(x, area.y, chrome_right.saturating_sub(x).min(NEW_SPACE_WIDTH), 1)
    } else {
        Rect::default()
    };

    (cards, new_space_hit_area)
}

pub(super) fn render_spaces_bar(
    app: &AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    frame: &mut Frame,
    area: Rect,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let p = &app.palette;
    let is_navigating = matches!(app.mode, Mode::Navigate);

    frame.render_widget(
        Paragraph::new(" ".repeat(area.width as usize)).style(Style::default().bg(p.panel_bg)),
        area,
    );

    let labels = workspace_chip_labels(app, terminal_runtimes);
    for card in &app.view.workspace_card_areas {
        if card.rect.width == 0 {
            continue;
        }
        let i = card.ws_idx;
        let Some(ws) = app.workspaces.get(i) else {
            continue;
        };
        let selected = i == app.selected && is_navigating;
        let is_active = Some(i) == app.active;

        let bg = if selected {
            p.surface0
        } else if is_active {
            p.surface_dim
        } else {
            p.panel_bg
        };
        let name_style = if selected || is_active {
            Style::default()
                .fg(p.text)
                .bg(bg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.subtext0).bg(bg)
        };

        let (agg_state, agg_seen) = ws.aggregate_state(&app.terminals);
        let (dot, dot_style) = state_dot(agg_state, agg_seen, p);

        let name_width = card.rect.width.saturating_sub(CHIP_CHROME_WIDTH) as usize;
        let name = labels
            .get(i)
            .map(|l| truncate_end(l, name_width))
            .unwrap_or_default();

        let line = Line::from(vec![
            Span::styled(" ", Style::default().bg(bg)),
            Span::styled(dot, dot_style.bg(bg)),
            Span::styled(" ", Style::default().bg(bg)),
            Span::styled(name, name_style),
            Span::styled(" ", Style::default().bg(bg)),
        ]);
        frame.render_widget(Paragraph::new(line), card.rect);
    }

    if app.mouse_capture && app.view.new_space_hit_area.width > 0 {
        frame.render_widget(
            Paragraph::new(" + ").style(Style::default().fg(p.overlay1).bg(p.panel_bg)),
            app.view.new_space_hit_area,
        );
    }

    let menu_rect = app.global_launcher_rect();
    if menu_rect.width > 0 {
        let menu_line = if app.global_menu_attention_badge_visible() {
            Line::from(vec![
                Span::styled(
                    "● ",
                    Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
                ),
                Span::styled("menu", Style::default().fg(p.overlay0)),
            ])
        } else {
            Line::from(vec![Span::styled("menu", Style::default().fg(p.overlay0))])
        };
        frame.render_widget(
            Paragraph::new(menu_line).alignment(Alignment::Right),
            menu_rect,
        );
    }
}
