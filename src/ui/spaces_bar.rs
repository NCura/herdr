//! Horizontal spaces bar: workspaces rendered as chips on a single bottom row,
//! replacing the vertical sidebar. Each chip shows the workspace state dot,
//! name, git branch, a dirty marker (`*`, uncommitted changes), and
//! ahead/behind counters. When space is tight the branch truncates first;
//! the name, dirty marker, and counters survive longest.

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

/// Chip chrome around the content: leading pad, state dot, gap, trailing pad.
const CHIP_CHROME_WIDTH: u16 = 4;

struct ChipGit {
    branch: String,
    dirty: bool,
    ahead: usize,
    behind: usize,
}

impl ChipGit {
    fn arrows(&self) -> (Option<String>, Option<String>) {
        let ahead = (self.ahead > 0).then(|| format!("↑{}", self.ahead));
        let behind = (self.behind > 0).then(|| format!("↓{}", self.behind));
        (ahead, behind)
    }

    /// Width of " ↑a ↓b" including its leading gap, 0 when both counters are 0.
    fn arrows_width(&self) -> u16 {
        let (ahead, behind) = self.arrows();
        let inner = ahead.as_deref().map_or(0, display_width_u16)
            + behind.as_deref().map_or(0, display_width_u16)
            + u16::from(ahead.is_some() && behind.is_some());
        if inner == 0 {
            0
        } else {
            inner + 1
        }
    }

    fn dirty_width(&self) -> u16 {
        u16::from(self.dirty)
    }
}

struct ChipContent {
    name: String,
    git: Option<ChipGit>,
}

impl ChipContent {
    fn natural_width(&self) -> u16 {
        let git_w = self.git.as_ref().map_or(0, |git| {
            1 + display_width_u16(&git.branch) + git.dirty_width() + git.arrows_width()
        });
        display_width_u16(&self.name)
            .saturating_add(CHIP_CHROME_WIDTH)
            .saturating_add(git_w)
    }
}

fn chip_contents(app: &AppState, terminal_runtimes: &TerminalRuntimeRegistry) -> Vec<ChipContent> {
    app.workspaces
        .iter()
        .map(|ws| ChipContent {
            name: ws.display_name_from(&app.terminals, terminal_runtimes),
            git: ws.branch().map(|branch| ChipGit {
                branch,
                dirty: ws.git_dirty().unwrap_or(false),
                ahead: ws.git_ahead_behind().map_or(0, |(a, _)| a),
                behind: ws.git_ahead_behind().map_or(0, |(_, b)| b),
            }),
        })
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

    let contents = chip_contents(app, terminal_runtimes);
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

    let naturals: Vec<u16> = contents.iter().map(ChipContent::natural_width).collect();
    let total: u16 = naturals
        .iter()
        .fold(0u16, |acc, w| acc.saturating_add(*w));
    let cap = if total <= avail {
        u16::MAX
    } else {
        (avail / contents.len().max(1) as u16).max(MIN_CHIP_WIDTH)
    };

    let mut cards = Vec::with_capacity(contents.len());
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

    let contents = chip_contents(app, terminal_runtimes);
    for card in &app.view.workspace_card_areas {
        if card.rect.width == 0 {
            continue;
        }
        let i = card.ws_idx;
        let (Some(ws), Some(content)) = (app.workspaces.get(i), contents.get(i)) else {
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
        let branch_style = Style::default()
            .fg(if selected || is_active {
                p.mauve
            } else {
                p.overlay0
            })
            .bg(bg);

        let (agg_state, agg_seen) = ws.aggregate_state(&app.terminals);
        let (dot, dot_style) = state_dot(agg_state, agg_seen, p);

        // Fit content into the chip: the name wins, then the dirty marker,
        // then the ahead/behind counters; the branch shrinks or drops first.
        let content_budget = card.rect.width.saturating_sub(CHIP_CHROME_WIDTH) as usize;
        let name = truncate_end(&content.name, content_budget);
        let mut budget =
            content_budget.saturating_sub(display_width_u16(&name) as usize);

        let mut spans = vec![
            Span::styled(" ", Style::default().bg(bg)),
            Span::styled(dot, dot_style.bg(bg)),
            Span::styled(" ", Style::default().bg(bg)),
            Span::styled(name, name_style),
        ];

        if let Some(git) = &content.git {
            let dirty_w = git.dirty_width() as usize;
            let show_dirty = dirty_w > 0 && budget >= dirty_w;
            if show_dirty {
                budget -= dirty_w;
            }
            let arrows_w = git.arrows_width() as usize;
            let show_arrows = arrows_w > 0 && budget >= arrows_w;
            if show_arrows {
                budget -= arrows_w;
            }
            // Leading gap + at least two branch cells, otherwise drop it.
            let branch_avail = budget.saturating_sub(1);
            if branch_avail >= 2 {
                spans.push(Span::styled(" ", Style::default().bg(bg)));
                spans.push(Span::styled(
                    truncate_end(&git.branch, branch_avail),
                    branch_style,
                ));
            }
            if show_dirty {
                spans.push(Span::styled(
                    "*",
                    Style::default()
                        .fg(p.yellow)
                        .bg(bg)
                        .add_modifier(Modifier::BOLD),
                ));
            }
            if show_arrows {
                let (ahead, behind) = git.arrows();
                spans.push(Span::styled(" ", Style::default().bg(bg)));
                if let Some(ahead) = ahead {
                    spans.push(Span::styled(ahead, Style::default().fg(p.green).bg(bg)));
                }
                if git.ahead > 0 && git.behind > 0 {
                    spans.push(Span::styled(" ", Style::default().bg(bg)));
                }
                if let Some(behind) = behind {
                    spans.push(Span::styled(behind, Style::default().fg(p.red).bg(bg)));
                }
            }
        }

        spans.push(Span::styled(" ", Style::default().bg(bg)));
        frame.render_widget(Paragraph::new(Line::from(spans)), card.rect);
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
