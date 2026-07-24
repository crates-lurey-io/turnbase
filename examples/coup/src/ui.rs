//! The retroglyph dashboard rendering for Coup (the `ui` feature).
//!
//! Implemented directly on [`Coup`] now that the game owns its crate: no
//! newtype is needed to satisfy the orphan rule, since `Coup` is local here.
//! [`crate::CoupView::pending`] is what makes the dashboard legible: Coup's real
//! complexity is its response windows ("p0 claims Tax, do you challenge or
//! block?"), and rendering that context turns a bare Pass/Challenge/Block menu
//! into a readable decision.

use retroglyph_core::grid::Rect;
use retroglyph_core::{AnsiColor, Backend, Color, Terminal};
use turnbase_simulator::PrintableGame;

use crate::{Action, Character, Coup, PendingView};

impl PrintableGame for Coup {
    fn draw_viewport<B: Backend>(&self, view: &Self::View, term: &mut Terminal<B>, area: Rect) {
        let mut y = area.top();
        term.print(area.left(), y, "== Coup ==");
        y = y.saturating_add(2);

        // The pending description can run past a narrow viewport ("p0 declares
        // Foreign Aid -- p1 may pass, challenge, or block" is 58 columns), so
        // wrap it rather than letting it bleed into the stats panel.
        term.fg(Color::Ansi(AnsiColor::BrightYellow));
        for line in wrap(&describe_pending(&view.pending), area.width()) {
            term.print(area.left(), y, &line);
            y = y.saturating_add(1);
        }
        term.reset_style();
        y = y.saturating_add(1);

        let deciding = active_seat(&view.pending);
        for (seat, &coins) in view.coins.iter().enumerate() {
            let marker = if seat == deciding { '>' } else { ' ' };
            if seat == deciding {
                term.fg(Color::Ansi(AnsiColor::BrightGreen));
            }
            let lost = describe_cards(&view.lost[seat]);
            term.print(
                area.left(),
                y,
                &format!(
                    "{marker} p{seat}: {coins} coins, {} influence (lost: {lost})",
                    view.influence[seat]
                ),
            );
            term.reset_style();
            y = y.saturating_add(1);
        }

        y = y.saturating_add(1);
        term.print(
            area.left(),
            y,
            &format!("deck: {} cards left", view.deck_size),
        );
        y = y.saturating_add(1);

        // During your own exchange your hand is briefly empty (its cards moved
        // into the pool), so show the pool -- with the indices `Action::Return`
        // needs -- instead of an empty "your hand" line.
        if let PendingView::ExchangeReturn { pool, .. } = &view.pending
            && !pool.is_empty()
        {
            term.print(
                area.left(),
                y,
                &format!("your exchange pool: {}", describe_indexed(pool)),
            );
        } else {
            term.print(
                area.left(),
                y,
                &format!("your hand: {}", describe_indexed(&view.own_hand)),
            );
        }
    }

    fn get_stats(&self, view: &Self::View) -> Vec<(String, String)> {
        // Keep every value short: this panel is narrow enough that a longer
        // line wraps into the row below and corrupts it. The full narrative
        // lives in the wider viewport instead.
        let mut stats: Vec<(String, String)> = view
            .coins
            .iter()
            .enumerate()
            .map(|(seat, &coins)| (format!("p{seat} coins"), coins.to_string()))
            .collect();
        stats.push(("deck".to_owned(), view.deck_size.to_string()));
        stats.push(("phase".to_owned(), phase_tag(&view.pending).to_owned()));
        stats
    }

    fn format_action(&self, action: &Self::Action) -> String {
        // Kept short (the actions panel is a narrow column); the wider
        // viewport's pending description carries the longer narrative.
        match action {
            Action::Income => "income (+1)".to_owned(),
            Action::ForeignAid => "foreign aid (+2)".to_owned(),
            Action::Coup(target) => format!("coup p{target}"),
            Action::Tax => "tax (Duke, +3)".to_owned(),
            Action::Assassinate(target) => format!("assassinate p{target} (Assassin)"),
            Action::Steal(target) => format!("steal p{target} (Captain)"),
            Action::Exchange => "exchange (Ambassador)".to_owned(),
            Action::Return(index) => format!("return card {index}"),
            Action::Pass => "pass".to_owned(),
            Action::Challenge => "challenge".to_owned(),
            Action::Block(character) => format!("block ({character:?})"),
            Action::Lose(index) => format!("discard card {index}"),
        }
    }
}

/// The seat [`PendingView`] says is currently owed a decision, as a `usize`
/// ready to compare against an `enumerate()` index.
fn active_seat(pending: &PendingView) -> usize {
    use PendingView as P;
    let seat = match pending {
        P::ChooseAction { actor } => *actor,
        P::Respond { responder, .. } | P::RespondToBlock { responder, .. } => *responder,
        P::Lose { who } => *who,
        P::ExchangeReturn { player, .. } => *player,
        P::GameOver => return usize::MAX, // no seat is deciding anything
    };
    usize::from(seat)
}

/// A one-line narrative of the current decision point, for the viewport.
fn describe_pending(pending: &PendingView) -> String {
    use PendingView as P;
    match pending {
        P::ChooseAction { actor } => format!("p{actor}'s turn: choose an action"),
        P::Respond {
            actor,
            action,
            claim,
            responder,
        } => {
            let claim = claim.map(|c| format!(", claims {c:?}")).unwrap_or_default();
            let can_block = matches!(
                action,
                Action::ForeignAid | Action::Assassinate(_) | Action::Steal(_)
            );
            let options = if can_block {
                "pass, challenge, or block"
            } else {
                "pass or challenge"
            };
            format!(
                "p{actor} declares {}{claim} -- p{responder} may {options}",
                declared_action_name(*action)
            )
        }
        P::RespondToBlock {
            actor,
            blocker,
            block_as,
            responder,
            ..
        } => format!(
            "p{blocker} blocks p{actor}, claims {block_as:?} -- p{responder} may pass or challenge"
        ),
        P::Lose { who } => format!("p{who} must reveal and discard an influence card"),
        P::ExchangeReturn {
            player,
            pool,
            returns_left,
        } => {
            if pool.is_empty() {
                // Redacted: not the viewer's own exchange, so the pool never
                // got populated.
                format!("p{player} is exchanging: choosing which cards to keep")
            } else {
                format!(
                    "p{player} is exchanging: return {returns_left} more of {}",
                    describe_indexed(pool)
                )
            }
        }
        P::GameOver => "match over".to_owned(),
    }
}

/// Short, fixed-width tag for the stats panel's "phase" row.
const fn phase_tag(pending: &PendingView) -> &'static str {
    use PendingView as P;
    match pending {
        P::ChooseAction { .. } => "choose",
        P::Respond { .. } => "respond",
        P::RespondToBlock { .. } => "block?",
        P::Lose { .. } => "lose",
        P::ExchangeReturn { .. } => "exchange",
        P::GameOver => "over",
    }
}

/// The claim-worthy name of a declared action (the ones a response window can
/// open on), for [`describe_pending`].
const fn declared_action_name(action: Action) -> &'static str {
    match action {
        Action::ForeignAid => "Foreign Aid",
        Action::Tax => "Tax",
        Action::Assassinate(_) => "Assassinate",
        Action::Steal(_) => "Steal",
        Action::Exchange => "Exchange",
        _ => "an action",
    }
}

/// Greedily word-wraps `text` to at most `width` columns per line, since
/// `Terminal::print` only wraps at the grid's width, not an arbitrary rect's.
fn wrap(text: &str, width: u16) -> Vec<String> {
    let width = usize::from(width);
    if width == 0 {
        return vec![text.to_owned()];
    }
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let extra = usize::from(!line.is_empty());
        if !line.is_empty() && line.len() + extra + word.len() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Renders a hand/pool as `"0=Duke, 1=Captain"`, or `"none"` when empty.
/// Indexed to match what `Action::Lose`/`Action::Return` expect.
fn describe_indexed(cards: &[Character]) -> String {
    if cards.is_empty() {
        return "none".to_owned();
    }
    cards
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{i}={c:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Renders a revealed-card list as `"Duke, Captain"` (no indices: revealed
/// cards are not addressable by any action), or `"none"` when empty.
fn describe_cards(cards: &[Character]) -> String {
    if cards.is_empty() {
        return "none".to_owned();
    }
    cards
        .iter()
        .map(|c| format!("{c:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}
