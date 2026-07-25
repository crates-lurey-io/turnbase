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
use retroglyph_core::{Backend, Style, Terminal};
use retroglyph_widgets::{Panel, Scrollbar, Theme, Widget};

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

/// Draws a selectable menu of `items` into `rect` (below `top_offset` rows),
/// scrolling so the `selected` row stays visible when the list is taller than
/// the panel, and marking the selection with a `>`.
///
/// A long legal-action list (Risk's fortify options, say) otherwise clips at
/// the panel's bottom edge and can hide the very row the cursor is on; this
/// windows the list around the selection instead. The caller shows the total
/// count in the panel header, so the window is a plain slice with no in-panel
/// scroll chrome.
pub fn draw_menu<B: Backend>(
    term: &mut Terminal<B>,
    rect: Rect,
    top_offset: u16,
    items: &[String],
    selected: usize,
) {
    let capacity = usize::from(rect.height().saturating_sub(top_offset));
    if capacity == 0 || items.is_empty() {
        return;
    }
    let selected = selected.min(items.len() - 1);
    // Keep `selected` on-screen: scroll only once it would fall past the last
    // visible row, and never past the point that leaves a blank tail.
    let start = selected
        .saturating_sub(capacity - 1)
        .min(items.len().saturating_sub(capacity));
    let rows = items[start..(start + capacity).min(items.len())]
        .iter()
        .enumerate()
        .map(|(offset, label)| {
            let marker = if start + offset == selected { '>' } else { ' ' };
            format!("{marker} {label}")
        });
    print_rows(term, rect, top_offset, rows);
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

/// Draws a themed, titled border in `rect` and returns the interior content
/// rect (the area inside the border).
///
/// The interior is deliberately left on the terminal's own background rather
/// than filled with the theme's panel color: a game draws its viewport (and
/// the dashboard prints its rows) with the default background, so a fill would
/// only show through on the cells nothing was drawn on, giving a patchy look.
/// A border-only frame keeps every cell on one consistent background.
///
/// Returns a degenerate (possibly zero-sized) inner rect when `rect` is too
/// small to hold a border; the row/menu helpers all clip to it safely.
pub fn panel<B: Backend>(term: &mut Terminal<B>, rect: Rect, theme: Theme, title: &str) -> Rect {
    if rect.width() >= 2 && rect.height() >= 2 {
        Panel::new()
            .border_style(Style::new().fg(theme.border))
            .render(rect, term);
        if !title.is_empty() && rect.width() > 4 {
            // Overwrite the top border with an inset, padded title in the
            // accent color, one cell in from the left corner.
            let max = usize::from(rect.width().saturating_sub(2));
            let label: String = format!(" {title} ").chars().take(max).collect();
            term.reset_style().fg(theme.accent);
            term.print(rect.left().saturating_add(1), rect.top(), &label);
            term.reset_style();
        }
    }
    Rect::new(
        rect.left().saturating_add(1),
        rect.top().saturating_add(1),
        rect.width().saturating_sub(2),
        rect.height().saturating_sub(2),
    )
}

/// Draws the actions panel frame and returns its interior rect for the caller
/// to fill with a menu or a status line.
///
/// `position`, when given, is shown as `selected/total` in the title so a
/// scrolled menu still says how many actions there are.
pub fn actions_panel<B: Backend>(
    term: &mut Terminal<B>,
    rect: Rect,
    theme: Theme,
    position: Option<(usize, usize)>,
) -> Rect {
    let title = match position {
        Some((selected, total)) => format!("Actions {selected}/{total}"),
        None => "Actions".to_owned(),
    };
    panel(term, rect, theme, &title)
}

/// Draws the log panel: a themed frame, the tail of `log` windowed by
/// `offset` (0 pins to the newest line, each increment scrolls one line back),
/// and a one-cell [`Scrollbar`] on the right edge once there is more history
/// than fits.
///
/// Returns the number of visible log rows, so an interactive caller can clamp
/// its scroll offset and size a page jump.
pub fn draw_log<B: Backend>(
    log: &[String],
    term: &mut Terminal<B>,
    rect: Rect,
    theme: Theme,
    offset: usize,
) -> usize {
    let title = if offset > 0 { "Log (scrolled)" } else { "Log" };
    let inner = panel(term, rect, theme, title);
    let visible = usize::from(inner.height());
    if visible == 0 || inner.width() == 0 {
        return visible;
    }

    let total = log.len();
    let max_back = total.saturating_sub(visible);
    let start = max_back.saturating_sub(offset.min(max_back));

    let has_bar = total > visible;
    if has_bar {
        let bar = Rect::new(
            inner.right().saturating_sub(1),
            inner.top(),
            1,
            inner.height(),
        );
        Scrollbar::new(total, visible)
            .offset(start)
            .theme(theme)
            .render(bar, term);
    }
    let text = if has_bar {
        Rect::new(
            inner.left(),
            inner.top(),
            inner.width().saturating_sub(1),
            inner.height(),
        )
    } else {
        inner
    };

    let end = (start + visible).min(total);
    print_rows(term, text, 0, log[start..end].iter().cloned());
    visible
}

/// Draws the viewport, the stats panel, and the log strip -- the parts every
/// dashboard renders identically from one seat's `view`, each in a themed
/// titled frame. `log_offset` scrolls the log back through history (0 pins to
/// the newest line); the return value is the number of visible log rows, for
/// an interactive caller to clamp scrolling against.
pub fn draw_board_stats_log<G, B>(
    game: &G,
    view: &G::View,
    log: &[String],
    term: &mut Terminal<B>,
    layout: &Layout,
    theme: Theme,
    log_offset: usize,
) -> usize
where
    G: PrintableGame,
    B: Backend,
{
    let board = panel(term, layout.viewport, theme, "");
    game.draw_viewport(view, term, board);

    let stats = panel(term, layout.stats, theme, "Stats");
    print_rows(
        term,
        stats,
        0,
        game.get_stats(view)
            .into_iter()
            .map(|(key, value)| format!("{key}: {value}")),
    );

    draw_log(log, term, layout.log, theme, log_offset)
}
