//! Shared drawing for the fixed-layout dashboard: the panel [`Layout`] and the
//! viewport/stats/log rendering that both the auto-only [`SimulationRunner`]
//! and the interactive [`SessionApp`] draw identically.
//!
//! Each App fills the actions panel and the bottom status bar itself; those
//! differ (the session App shows run controls and a setup modal the runner has
//! no concept of).
//!
//! [`SimulationRunner`]: crate::SimulationRunner
//! [`SessionApp`]: crate::SessionApp

use retroglyph_core::grid::Rect;
use retroglyph_core::{Backend, Terminal};

use crate::PrintableGame;

/// Blank cells left between adjacent panels.
pub const GUTTER: u16 = 1;

/// The fixed panels of the dashboard.
///
/// Plain rect arithmetic, not a constraint solver: viewport left 70%, stats
/// and action menu stacked in the remaining top-right 30%, a log strip across
/// the bottom, and a one-row status bar reserved along the very bottom edge.
pub struct Layout {
    pub viewport: Rect,
    pub stats: Rect,
    pub actions: Rect,
    pub log: Rect,
    pub status: Rect,
}

impl Layout {
    pub const fn new(full: Rect) -> Self {
        let width = full.width();
        let height = full.height();

        // The very bottom row is the status/help bar; everything else lays out
        // in the body above it.
        let body_height = height.saturating_sub(1);
        let status = Rect::new(0, body_height, width, 1);

        let left_width = width * 7 / 10;
        let right_x = left_width.saturating_add(GUTTER);
        let right_width = width.saturating_sub(right_x);
        let log_height = body_height / 4;
        let top_height = body_height
            .saturating_sub(log_height)
            .saturating_sub(GUTTER);
        let log_y = top_height.saturating_add(GUTTER);
        let log_h = body_height.saturating_sub(log_y);
        let stats_height = top_height / 2;
        let actions_y = stats_height.saturating_add(GUTTER);
        let actions_height = top_height.saturating_sub(actions_y);

        Self {
            viewport: Rect::new(0, 0, left_width, top_height),
            stats: Rect::new(right_x, 0, right_width, stats_height),
            actions: Rect::new(right_x, actions_y, right_width, actions_height),
            log: Rect::new(0, log_y, width, log_h),
            status,
        }
    }
}

/// Prints `lines` one per row starting `top_offset` rows below `rect`'s top
/// edge, clipping to `rect`'s width and dropping rows past its bottom edge, so
/// overlong content clips instead of spilling into the next panel.
pub fn print_rows<B: Backend>(
    term: &mut Terminal<B>,
    rect: Rect,
    top_offset: u16,
    lines: impl IntoIterator<Item = String>,
) {
    let max_chars = usize::from(rect.width());
    let mut y = rect.top().saturating_add(top_offset);
    for line in lines {
        if y >= rect.bottom() {
            break;
        }
        let clipped: String = line.chars().take(max_chars).collect();
        term.print(rect.left(), y, &clipped);
        y = y.saturating_add(1);
    }
}

/// Draws the viewport, the stats panel, and the log strip -- the parts every
/// dashboard renders identically from one seat's `view`.
pub fn draw_board_stats_log<G, B>(
    game: &G,
    view: &G::View,
    log: &[String],
    term: &mut Terminal<B>,
    layout: &Layout,
) where
    G: PrintableGame,
    B: Backend,
{
    game.draw_viewport(view, term, layout.viewport);

    term.print(layout.stats.left(), layout.stats.top(), "-- stats --");
    print_rows(
        term,
        layout.stats,
        1,
        game.get_stats(view)
            .into_iter()
            .map(|(key, value)| format!("{key}: {value}")),
    );

    let capacity = usize::from(layout.log.height());
    let start = log.len().saturating_sub(capacity);
    print_rows(term, layout.log, 0, log[start..].iter().cloned());
}
