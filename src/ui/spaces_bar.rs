//! Horizontal spaces bar: workspaces rendered as chips on a single bottom row,
//! replacing the vertical sidebar. Each chip shows one clickable status dot
//! per running agent (tab order), the workspace name, git branch, a dirty
//! marker (`*`, uncommitted changes), and ahead/behind counters. When space is
//! tight the branch truncates first; the name, dirty marker, and counters
//! survive longest.
//!
//! Chip content is centered, so rendering and mouse hit-testing share
//! `build_chip_line` to agree on where every span (notably the agent dots)
//! actually lands.

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use super::status::{agent_icon, agent_icon_on_accent};
use super::text::{display_width_u16, truncate_end};
use super::widgets::panel_contrast_fg;
use crate::app::state::WorkspaceCardArea;
use crate::app::{AppState, Mode};
use crate::terminal::TerminalRuntimeRegistry;

/// Branch names longer than this are elided with `…` in the chip.
const MAX_BRANCH_WIDTH: usize = 11;

/// Chip chrome besides the agent dots: leading and trailing pad.
const CHIP_BASE_CHROME_WIDTH: u16 = 2;

/// Minimum useful branch width: below this the branch drops entirely
/// instead of degrading into a one-letter stub like "d…".
const MIN_BRANCH_WIDTH: usize = 5;

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

/// One entry per detected agent in the workspace, in stable order: tab order,
/// then pane creation number (the pane map itself is unordered). Empty when
/// no agent is running anywhere in the workspace.
fn agent_dots(
    ws: &crate::workspace::Workspace,
    terminals: &std::collections::HashMap<
        crate::terminal::TerminalId,
        crate::terminal::TerminalState,
    >,
) -> Vec<(crate::layout::PaneId, crate::detect::AgentState, bool)> {
    (0..ws.tabs.len())
        .flat_map(|tab_idx| super::tabs::tab_agent_dots(ws, tab_idx, terminals))
        .collect()
}

/// A chip's fully-styled line plus the geometry needed for dot hit-testing.
struct ChipLine {
    spans: Vec<Span<'static>>,
    width: u16,
    /// Offset of each agent dot from the start of the line.
    dot_offsets: Vec<(crate::layout::PaneId, u16)>,
    bg: ratatui::style::Color,
}

fn build_chip_line(
    app: &AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    ws_idx: usize,
    chip_width: u16,
) -> Option<ChipLine> {
    let ws = app.workspaces.get(ws_idx)?;
    let p = &app.palette;
    let is_navigating = matches!(app.mode, Mode::Navigate);
    let selected = ws_idx == app.selected && is_navigating;
    let is_active = Some(ws_idx) == app.active;

    // The active space gets the exact same accent-filled treatment as the
    // active tab. Status indicators receive hue-preserving contrast variants
    // below so their state remains recognizable on the accent background.
    let contrast = panel_contrast_fg(p);
    let bg = if is_active {
        p.accent
    } else if selected {
        p.surface0
    } else {
        p.panel_bg
    };
    let name_style = if is_active {
        Style::default()
            .fg(contrast)
            .bg(bg)
            .add_modifier(Modifier::BOLD)
    } else if selected {
        Style::default()
            .fg(p.text)
            .bg(bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(p.subtext0).bg(bg)
    };
    let prefix_style = if is_active {
        Style::default()
            .fg(contrast)
            .bg(bg)
            .add_modifier(Modifier::DIM)
    } else {
        Style::default().fg(p.overlay0).bg(bg)
    };
    let branch_style = if is_active {
        Style::default()
            .fg(contrast)
            .bg(bg)
            .add_modifier(Modifier::DIM)
    } else if selected {
        Style::default().fg(p.mauve).bg(bg)
    } else {
        Style::default().fg(p.overlay0).bg(bg)
    };
    let dirty_style = Style::default()
        .fg(if is_active { contrast } else { p.yellow })
        .bg(bg)
        .add_modifier(Modifier::BOLD);
    let ahead_style = Style::default()
        .fg(if is_active { contrast } else { p.green })
        .bg(bg);
    let behind_style = Style::default()
        .fg(if is_active { contrast } else { p.red })
        .bg(bg);

    // Status indicators render at the end of the chip: one cell per agent,
    // one gap between indicators. No indicators at all for agent-less
    // workspaces — the signal should stand out, not the absence of one.
    // Width includes the gap separating the block from the name/git content.
    let dots = agent_dots(ws, &app.terminals);
    let dots_block_width = if dots.is_empty() {
        0
    } else {
        (dots.len() * 2) as u16
    };

    // Chips read "<position> - <name> ...", matching the tab labels.
    let number_prefix = format!("{} - ", ws_idx + 1);
    let name_full = ws.display_name_from(&app.terminals, terminal_runtimes);
    let git = ws.branch().map(|branch| ChipGit {
        branch: truncate_end(&branch, MAX_BRANCH_WIDTH),
        dirty: ws.git_dirty().unwrap_or(false),
        ahead: ws.git_ahead_behind().map_or(0, |(a, _)| a),
        behind: ws.git_ahead_behind().map_or(0, |(_, b)| b),
    });

    // Fit content into the chip: the name wins, then the dirty marker, then
    // the ahead/behind counters; the branch shrinks or drops first.
    let content_budget = chip_width.saturating_sub(
        CHIP_BASE_CHROME_WIDTH + dots_block_width + display_width_u16(&number_prefix),
    ) as usize;
    let dirty_width = git.as_ref().map_or(0, |git| git.dirty_width()) as usize;
    let name = truncate_end(&name_full, content_budget.saturating_sub(dirty_width));
    let mut budget = content_budget.saturating_sub(display_width_u16(&name) as usize);

    let mut spans = vec![
        Span::styled(" ", Style::default().bg(bg)),
        Span::styled(number_prefix, prefix_style),
        Span::styled(name, name_style),
    ];

    if let Some(git) = &git {
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
        // Leading gap + a usefully-wide branch, otherwise drop it entirely.
        let branch_avail = budget.saturating_sub(1);
        if branch_avail >= MIN_BRANCH_WIDTH {
            spans.push(Span::styled(" ", Style::default().bg(bg)));
            spans.push(Span::styled(
                truncate_end(&git.branch, branch_avail),
                branch_style,
            ));
        }
        if show_dirty {
            spans.push(Span::styled("*", dirty_style));
        }
        if show_arrows {
            let (ahead, behind) = git.arrows();
            spans.push(Span::styled(" ", Style::default().bg(bg)));
            if let Some(ahead) = ahead {
                spans.push(Span::styled(ahead, ahead_style));
            }
            if git.ahead > 0 && git.behind > 0 {
                spans.push(Span::styled(" ", Style::default().bg(bg)));
            }
            if let Some(behind) = behind {
                spans.push(Span::styled(behind, behind_style));
            }
        }
    }
    // Working agents use the same animated spinner as the agent panel. Active
    // indicators keep their semantic hue but shift luminance for contrast.
    if !dots.is_empty() {
        for (_, state, seen) in &dots {
            spans.push(Span::styled(" ", Style::default().bg(bg)));
            let (indicator, indicator_style) = if is_active {
                agent_icon_on_accent(*state, *seen, app.spinner_tick, p)
            } else {
                agent_icon(*state, *seen, app.spinner_tick, p)
            };
            spans.push(Span::styled(indicator, indicator_style.bg(bg)));
        }
    }
    spans.push(Span::styled(" ", Style::default().bg(bg)));

    let width = spans
        .iter()
        .map(|span| display_width_u16(&span.content))
        .fold(0u16, |acc, w| acc.saturating_add(w));

    // The dots block ("· ● ● …" without its leading gap) sits right before
    // the trailing pad; the first dot lands one cell into the block.
    let first_dot_offset = width.saturating_sub(dots_block_width);
    let dot_offsets = dots
        .iter()
        .enumerate()
        .map(|(i, (pane_id, _, _))| (*pane_id, first_dot_offset + 2 * i as u16))
        .collect();

    Some(ChipLine {
        spans,
        width,
        dot_offsets,
        bg,
    })
}

/// Where a centered chip line starts inside its card (mirrors the render).
fn chip_line_start_x(card: &WorkspaceCardArea, line_width: u16) -> u16 {
    card.rect.x + card.rect.width.saturating_sub(line_width) / 2
}

/// The agent pane whose status dot sits at (`col`, `row`), if any.
pub(crate) fn spaces_bar_agent_dot_at(
    app: &AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    col: u16,
    row: u16,
) -> Option<(usize, crate::layout::PaneId)> {
    let bar = app.view.spaces_bar_rect;
    if bar.height == 0 || row != bar.y {
        return None;
    }
    let card = app
        .view
        .workspace_card_areas
        .iter()
        .find(|card| col >= card.rect.x && col < card.rect.x + card.rect.width)?;
    let line = build_chip_line(app, terminal_runtimes, card.ws_idx, card.rect.width)?;
    let start_x = chip_line_start_x(card, line.width);
    line.dot_offsets.iter().find_map(|(pane_id, offset)| {
        (start_x.saturating_add(*offset) == col).then_some((card.ws_idx, *pane_id))
    })
}

/// Lay the workspaces out left-to-right in `area` with a 1-cell gap between
/// chips. Every chip gets its natural width first; leftover row space is
/// distributed equally (so the row still fills edge-to-edge), and when the
/// row overflows, chips with short content keep their natural width while
/// long ones split what remains fairly. The new-space button keeps the right
/// edge with one cell of breathing room. Returns the per-workspace hit rects
/// plus the new-space button rect.
pub(crate) fn compute_spaces_bar_areas(
    app: &AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    area: Rect,
) -> Vec<WorkspaceCardArea> {
    if area.width == 0 || area.height == 0 || app.workspaces.is_empty() {
        return Vec::new();
    }

    let count = app.workspaces.len();
    let gaps = count.saturating_sub(1) as u16;
    let avail = area.width.saturating_sub(gaps);

    let naturals: Vec<u16> = (0..count)
        .map(|ws_idx| {
            build_chip_line(app, terminal_runtimes, ws_idx, u16::MAX).map_or(0, |line| line.width)
        })
        .collect();
    let total: u16 = naturals.iter().fold(0u16, |acc, w| acc.saturating_add(*w));

    let mut widths = naturals.clone();
    if total <= avail {
        // Surplus: everyone grows by an equal share.
        let surplus = avail - total;
        let per = surplus / count as u16;
        let extra = surplus % count as u16;
        for (i, width) in widths.iter_mut().enumerate() {
            *width += per + u16::from((i as u16) < extra);
        }
    } else {
        // Deficit: waterfill from the smallest chip up — short chips keep
        // their natural width, long ones split the remainder fairly.
        let mut order: Vec<usize> = (0..count).collect();
        order.sort_by_key(|&i| naturals[i]);
        let mut remaining = avail;
        let mut left = count as u16;
        for &i in &order {
            let fair = remaining / left.max(1);
            widths[i] = naturals[i].min(fair);
            remaining -= widths[i];
            left -= 1;
        }
        // Hand rounding leftovers to chips still below their natural width.
        while remaining > 0 {
            let mut progressed = false;
            for i in 0..count {
                if remaining == 0 {
                    break;
                }
                if widths[i] < naturals[i] {
                    widths[i] += 1;
                    remaining -= 1;
                    progressed = true;
                }
            }
            if !progressed {
                break;
            }
        }
    }

    let mut cards = Vec::with_capacity(count);
    let mut x = area.x;
    for (ws_idx, width) in widths.iter().enumerate() {
        cards.push(WorkspaceCardArea {
            ws_idx,
            rect: Rect::new(x, area.y, *width, 1),
            indented: false,
        });
        x = x.saturating_add(width + 1);
    }

    cards
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

    frame.render_widget(
        Paragraph::new(" ".repeat(area.width as usize)).style(Style::default().bg(p.panel_bg)),
        area,
    );

    for card in &app.view.workspace_card_areas {
        if card.rect.width == 0 {
            continue;
        }
        let Some(line) = build_chip_line(app, terminal_runtimes, card.ws_idx, card.rect.width)
        else {
            continue;
        };

        // Paint the chip background across the full rect, then render the
        // line manually centered so dot hit-testing knows exact positions.
        frame.render_widget(
            Paragraph::new("").style(Style::default().bg(line.bg)),
            card.rect,
        );
        let start_x = chip_line_start_x(card, line.width);
        let card_right = card.rect.x + card.rect.width;
        let line_rect = Rect::new(
            start_x,
            card.rect.y,
            line.width.min(card_right.saturating_sub(start_x)),
            1,
        );
        frame.render_widget(Paragraph::new(Line::from(line.spans)), line_rect);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Workspace;

    #[test]
    fn truncated_workspace_name_keeps_dirty_marker() {
        let mut app = AppState::test_new();
        let mut workspace = Workspace::test_new("my-very-long-name");
        workspace.cached_git_branch = Some("main".into());
        workspace.cached_git_dirty = Some(true);
        app.workspaces = vec![workspace];

        let line = build_chip_line(&app, &TerminalRuntimeRegistry::new(), 0, 16)
            .expect("workspace chip should be built");
        let text = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert!(text.contains("…*"), "chip text: {text:?}");
        assert!(line.width <= 16, "chip width: {}", line.width);
    }

    #[test]
    fn active_workspace_contrast_adjusts_working_color_on_accent_background() {
        let mut app = AppState::test_new();
        app.palette.accent = app.palette.peach;
        app.workspaces = vec![Workspace::test_new("test")];
        app.active = Some(0);
        app.ensure_test_terminals();
        let pane_id = app.workspaces[0].tabs[0].root_pane;
        let terminal_id = app.workspaces[0].tabs[0].panes[&pane_id]
            .attached_terminal_id
            .clone();
        let terminal = app
            .terminals
            .get_mut(&terminal_id)
            .expect("test terminal should exist");
        terminal.detected_agent = Some(crate::detect::Agent::Claude);
        terminal.state = crate::detect::AgentState::Working;

        let line = build_chip_line(&app, &TerminalRuntimeRegistry::new(), 0, 30)
            .expect("workspace chip should be built");
        let spinner = super::super::spinner_frame(app.spinner_tick);
        let spinner_idx = line
            .spans
            .iter()
            .position(|span| span.content.as_ref() == spinner)
            .expect("working workspace should show a spinner");

        for span in &line.spans[spinner_idx - 1..=spinner_idx + 1] {
            assert_eq!(span.style.bg, Some(app.palette.accent));
        }
        let (_, expected_style) = agent_icon_on_accent(
            crate::detect::AgentState::Working,
            false,
            app.spinner_tick,
            &app.palette,
        );
        assert_eq!(line.spans[spinner_idx].style.fg, expected_style.fg);
        assert_ne!(line.spans[spinner_idx].style.fg, Some(app.palette.yellow));
    }

    #[test]
    fn active_and_inactive_workspaces_use_teal_done_variants() {
        let mut app = AppState::test_new();
        app.palette.accent = app.palette.peach;
        app.workspaces = vec![
            Workspace::test_new("active"),
            Workspace::test_new("inactive"),
        ];
        app.active = Some(0);
        app.ensure_test_terminals();

        for ws_idx in 0..app.workspaces.len() {
            let pane_id = app.workspaces[ws_idx].tabs[0].root_pane;
            let terminal_id = app.workspaces[ws_idx].tabs[0].panes[&pane_id]
                .attached_terminal_id
                .clone();
            app.workspaces[ws_idx].tabs[0]
                .panes
                .get_mut(&pane_id)
                .expect("test pane should exist")
                .seen = false;
            let terminal = app
                .terminals
                .get_mut(&terminal_id)
                .expect("test terminal should exist");
            terminal.detected_agent = Some(crate::detect::Agent::Claude);
            terminal.state = crate::detect::AgentState::Idle;
        }

        let runtime_registry = TerminalRuntimeRegistry::new();
        let active = build_chip_line(&app, &runtime_registry, 0, 30)
            .expect("active workspace chip should be built");
        let inactive = build_chip_line(&app, &runtime_registry, 1, 30)
            .expect("inactive workspace chip should be built");
        let done_style = |line: &ChipLine| {
            line.spans
                .iter()
                .find(|span| span.content.as_ref() == "●")
                .expect("done workspace should show a filled circle")
                .style
        };

        let (_, active_done_style) = agent_icon_on_accent(
            crate::detect::AgentState::Idle,
            false,
            app.spinner_tick,
            &app.palette,
        );
        assert_eq!(done_style(&active).fg, active_done_style.fg);
        assert_eq!(done_style(&inactive).fg, Some(app.palette.teal));
        assert_ne!(done_style(&active).fg, done_style(&inactive).fg);
        assert_eq!(done_style(&active).bg, Some(app.palette.accent));
        assert_eq!(done_style(&inactive).bg, Some(app.palette.panel_bg));
    }
}
