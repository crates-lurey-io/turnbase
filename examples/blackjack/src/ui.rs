//! The retroglyph dashboard rendering for Blackjack (the `ui` feature).
//!
//! Hidden information: the dealer's hole card renders as `??` in the viewport
//! until it is revealed (the player stands or busts), mirroring
//! `examples/coup/src/ui.rs`'s pattern of describing pending/hidden state in
//! plain text rather than assuming the raw `State` shape.

use retroglyph_core::grid::Rect;
use retroglyph_core::{AnsiColor, Backend, Color, Terminal};
use turnbase_simulator::PrintableGame;

use crate::{Action, Blackjack, BlackjackView, Card, best_total};

impl PrintableGame for Blackjack {
    fn draw_viewport<B: Backend>(&self, view: &Self::View, term: &mut Terminal<B>, area: Rect) {
        let mut y = area.top();
        term.print(area.left(), y, "== Blackjack ==");
        y = y.saturating_add(1);

        term.print(
            area.left(),
            y,
            &format!(
                "hand {} of {}   score: you {} - {} dealer",
                (view.round + 1).min(view.hands),
                view.hands,
                view.player_wins,
                view.dealer_wins
            ),
        );
        y = y.saturating_add(2);

        term.print(
            area.left(),
            y,
            &format!(
                "your hand: {} (total {})",
                describe_hand(&view.player_hand),
                best_total(&view.player_hand)
            ),
        );
        y = y.saturating_add(1);

        term.print(
            area.left(),
            y,
            &format!("dealer showing: {}", describe_dealer(view)),
        );
        y = y.saturating_add(2);

        term.fg(Color::Ansi(AnsiColor::BrightYellow));
        term.print(area.left(), y, &format!("phase: {}", phase_tag(view.phase)));
        term.reset_style();
        y = y.saturating_add(1);

        if let Some(outcome) = view.outcome {
            let label = if matches!(view.phase, crate::Phase::Done) {
                "match over -- last hand"
            } else {
                "last hand"
            };
            term.print(
                area.left(),
                y,
                &format!("{label}: {}", describe_outcome(outcome)),
            );
            y = y.saturating_add(1);
        }
        if matches!(view.phase, crate::Phase::Done) {
            term.fg(Color::Ansi(AnsiColor::BrightGreen));
            term.print(area.left(), y, describe_match(view));
            term.reset_style();
        }
    }

    fn get_stats(&self, view: &Self::View) -> Vec<(String, String)> {
        vec![
            (
                "hand".to_owned(),
                format!("{} of {}", (view.round + 1).min(view.hands), view.hands),
            ),
            (
                "score".to_owned(),
                format!("{} - {}", view.player_wins, view.dealer_wins),
            ),
            (
                "your total".to_owned(),
                best_total(&view.player_hand).to_string(),
            ),
            ("dealer showing".to_owned(), describe_dealer(view)),
            ("phase".to_owned(), phase_tag(view.phase).to_owned()),
            ("shoe".to_owned(), view.shoe_size.to_string()),
        ]
    }

    fn format_action(&self, action: &Self::Action) -> String {
        match action {
            Action::Hit => "hit".to_owned(),
            Action::Stand => "stand".to_owned(),
            Action::Deal(card) => format!("deal {card}"),
        }
    }
}

/// Renders a hand as `"A, 7, K"`, or `"(none)"` if empty.
fn describe_hand(hand: &[Card]) -> String {
    if hand.is_empty() {
        return "(none)".to_owned();
    }
    hand.iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Renders the dealer's face-up cards, plus `??` for the hole card until it
/// is revealed. `view.own_hole_card` is `Some` only from the dealer's own
/// viewpoint (see [`Blackjack::view`](crate::Blackjack)), so an omniscient/
/// dealer view shows the true total too.
fn describe_dealer(view: &BlackjackView) -> String {
    let up = describe_hand(&view.dealer_hand);
    if view.hole_revealed {
        format!("{up} (total {})", best_total(&view.dealer_hand))
    } else if let Some(hole) = view.own_hole_card {
        let mut with_hole = view.dealer_hand.clone();
        with_hole.push(hole);
        format!("{up}, {hole} [hole, total {}]", best_total(&with_hole))
    } else {
        format!("{up}, ??")
    }
}

/// Short, fixed-width tag for the stats panel's "phase" row.
const fn phase_tag(phase: crate::Phase) -> &'static str {
    use crate::Phase;
    match phase {
        Phase::Opening(_) => "dealing",
        Phase::PlayerTurn => "your turn",
        Phase::PlayerDraw => "dealing to you",
        Phase::DealerTurn => "dealer's turn",
        Phase::DealerDraw => "dealing to dealer",
        Phase::Done => "over",
    }
}

/// Declares the match winner from the final hand tally.
fn describe_match(view: &BlackjackView) -> &'static str {
    match view.player_wins.cmp(&view.dealer_wins) {
        std::cmp::Ordering::Greater => "you win the match",
        std::cmp::Ordering::Less => "dealer wins the match",
        std::cmp::Ordering::Equal => "the match is a draw",
    }
}

/// One-line description of a single hand's outcome.
const fn describe_outcome(outcome: crate::Outcome) -> &'static str {
    use crate::Outcome;
    match outcome {
        Outcome::PlayerWin => "you win",
        Outcome::DealerWin => "dealer wins",
        Outcome::Push => "push",
    }
}
