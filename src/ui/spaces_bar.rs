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

/// Width reserved for the trailing new-space button (" + ").
const NEW_SPACE_WIDTH: u16 = 3;

/// Chip chrome besides the agent dots: leading pad, gap after the dots,
/// trailing pad. Each agent dot adds one more cell.
const CHIP_BASE_CHROME_WIDTH: u16 = 3;

/// Branch names longer than this are elided with `…` in the chip.
const MAX_BRANCH_WIDTH: usize = 11;

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

/// One `(state, seen)` entry per detected agent in the workspace, in stable
/// order (tab order, then pane creation number — the pane map is unordered).
/// Empty when no agent is running anywhere in the workspace.
fn agent_dots(
    ws: &crate::workspace::Workspace,
    terminals: &std::collections::HashMap<
        crate::terminal::TerminalId,
        crate::terminal::TerminalState,
    >,
) -> Vec<(crate::detect::AgentState, bool)> {
    let mut dots = Vec::new();
    for tab in &ws.tabs {
        let mut panes: Vec<_> = tab.panes.iter().collect();
        panes.sort_by_key(|(pane_id, _)| {
            ws.public_pane_numbers
                .get(pane_id)
                .copied()
                .unwrap_or(usize::MAX)
        });
        for (_, pane) in panes {
            if let Some(terminal) = terminals.get(&pane.attached_terminal_id) {
                if terminal.detected_agent.is_some() {
                    dots.push((terminal.state, pane.seen));
                }
            }
        }
    }
    dots
}

fn chip_contents(app: &AppState, terminal_runtimes: &TerminalRuntimeRegistry) -> Vec<ChipContent> {
    app.workspaces
        .iter()
        .map(|ws| ChipContent {
            name: ws.display_name_from(&app.terminals, terminal_runtimes),
            git: ws.branch().map(|branch| ChipGit {
                branch: truncate_end(&branch, MAX_BRANCH_WIDTH),
                dirty: ws.git_dirty().unwrap_or(false),
                ahead: ws.git_ahead_behind().map_or(0, |(a, _)| a),
                behind: ws.git_ahead_behind().map_or(0, |(_, b)| b),
            }),
        })
        .collect()
}

/// Lay the workspaces out left-to-right in `area`, splitting the row into
/// equal shares: one chip takes the full width, two chips take half each, and
/// so on. The new-space button and the menu launcher keep the right edge.
/// Returns the per-workspace hit rects plus the new-space button rect.
pub(crate) fn compute_spaces_bar_areas(app: &AppState, area: Rect) -> (Vec<WorkspaceCardArea>, Rect) {
    if area.width == 0 || area.height == 0 || app.workspaces.is_empty() {
        return (Vec::new(), Rect::default());
    }

    let count = app.workspaces.len() as u16;
    let new_space_reserved = if app.mouse_capture { NEW_SPACE_WIDTH } else { 0 };
    let avail = area.width.saturating_sub(new_space_reserved);

    let base = avail / count;
    let extra = avail % count;

    let mut cards = Vec::with_capacity(count as usize);
    let mut x = area.x;
    for ws_idx in 0..count {
        let width = base + u16::from(ws_idx < extra);
        cards.push(WorkspaceCardArea {
            ws_idx: ws_idx as usize,
            rect: Rect::new(x, area.y, width, 1),
            indented: false,
        });
        x = x.saturating_add(width);
    }

    let new_space_hit_area = if app.mouse_capture {
        let chrome_right = area.x + area.width;
        Rect::new(x, area.y, chrome_right.saturating_sub(x).min(NEW_SPACE_WIDTH), 1)
    } else {
        Rect::default()
    };

    (cards, new_space_hit_area)
}

/// Column of the drop indicator for a workspace drag: the left edge of the
/// chip at `insert_idx`, or the right edge of the last chip for an append.
fn drop_indicator_x(cards: &[WorkspaceCardArea], insert_idx: usize) -> Option<u16> {
    if let Some(card) = cards.iter().find(|card| card.ws_idx == insert_idx) {
        return Some(card.rect.x);
    }
    let last = cards.last()?;
    (insert_idx == last.ws_idx + 1).then(|| last.rect.x + last.rect.width)
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

        // One dot per running agent; a neutral dot when there are none.
        let dots = agent_dots(ws, &app.terminals);
        let dots_width = dots.len().max(1) as u16;

        // Fit content into the chip: the name wins, then the dirty marker,
        // then the ahead/behind counters; the branch shrinks or drops first.
        let content_budget = card
            .rect
            .width
            .saturating_sub(CHIP_BASE_CHROME_WIDTH + dots_width) as usize;
        let name = truncate_end(&content.name, content_budget);
        let mut budget =
            content_budget.saturating_sub(display_width_u16(&name) as usize);

        let mut spans = vec![Span::styled(" ", Style::default().bg(bg))];
        if dots.is_empty() {
            let (dot, dot_style) = state_dot(crate::detect::AgentState::Unknown, true, p);
            spans.push(Span::styled(dot, dot_style.bg(bg)));
        } else {
            for (state, seen) in &dots {
                let (dot, dot_style) = state_dot(*state, *seen, p);
                spans.push(Span::styled(dot, dot_style.bg(bg)));
            }
        }
        spans.push(Span::styled(" ", Style::default().bg(bg)));
        spans.push(Span::styled(name, name_style));

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
        // Paragraph-level style paints the chip background across the full
        // rect (spans only cover the text); centered so wide chips read well.
        frame.render_widget(
            Paragraph::new(Line::from(spans))
                .alignment(Alignment::Center)
                .style(Style::default().bg(bg)),
            card.rect,
        );
    }

    if app.mouse_capture && app.view.new_space_hit_area.width > 0 {
        frame.render_widget(
            Paragraph::new(" + ").style(Style::default().fg(p.overlay1).bg(p.panel_bg)),
            app.view.new_space_hit_area,
        );
    }

    if let Some(crate::app::state::DragState {
        target:
            crate::app::state::DragTarget::WorkspaceReorder {
                insert_idx: Some(insert_idx),
                ..
            },
    }) = &app.drag
    {
        if let Some(x) = drop_indicator_x(&app.view.workspace_card_areas, *insert_idx) {
            frame.buffer_mut()[(x.min(area.x + area.width.saturating_sub(1)), area.y)]
                .set_symbol("│")
                .set_style(Style::default().fg(p.accent));
        }
    }

}
