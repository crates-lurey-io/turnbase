//! The retroglyph dashboard rendering for Hanabi (the `ui` feature).
//!
//! The dashboard fixes one viewing seat for the whole match (per
//! `turnbase_simulator::SimulationRunner`), which is exactly what makes the
//! inverted visibility rule visible on screen: the viewer's own row shows
//! hint-only slots while every other seat's row is drawn in full.

use retroglyph_core::grid::Rect;
use retroglyph_core::{AnsiColor, Backend, Color, Terminal};
use turnbase_simulator::PrintableGame;

use crate::{Action, Card, Hanabi, HanabiView, VisibleCard};

/// Short color labels for the standard five-color palette; a match configured
/// with more colors falls back to a numeric label past this list.
const COLOR_NAMES: [&str; 5] = ["Red", "Yellow", "Green", "White", "Blue"];

fn color_label(color: u8) -> String {
    COLOR_NAMES
        .get(color as usize)
        .map_or_else(|| format!("c{color}"), |&name| name.to_owned())
}

fn card_label(card: Card) -> String {
    format!("{}{}", color_label(card.color), card.rank)
}

/// The seat whose hand is drawn hint-only in `view` (the viewer), or `None`
/// for a spectator (every hand shown in full, so there is no "own" row).
fn own_seat(view: &HanabiView) -> Option<usize> {
    view.hands
        .iter()
        .position(|hand| hand.iter().any(|c| matches!(c, VisibleCard::Own { .. })))
}

impl PrintableGame for Hanabi {
    fn draw_viewport<B: Backend>(&self, view: &Self::View, term: &mut Terminal<B>, area: Rect) {
        let mut y = area.top();
        term.print(area.left(), y, "== Hanabi ==");
        y = y.saturating_add(2);

        let fireworks: String = view
            .fireworks
            .iter()
            .enumerate()
            .map(|(color, &top)| {
                let top = if top == 0 {
                    "-".to_owned()
                } else {
                    top.to_string()
                };
                format!("{}:{top}", color_label(u8::try_from(color).unwrap()))
            })
            .collect::<Vec<_>>()
            .join("  ");
        term.print(area.left(), y, &format!("fireworks: {fireworks}"));
        y = y.saturating_add(1);

        let score: u32 = view.fireworks.iter().map(|&r| u32::from(r)).sum();
        term.print(
            area.left(),
            y,
            &format!(
                "score: {score}  hints: {}/8  fuses: {}/3  deck: {}",
                view.hint_tokens, view.fuse_tokens, view.deck_size
            ),
        );
        y = y.saturating_add(1);

        let discards: String = if view.discard.is_empty() {
            "none".to_owned()
        } else {
            view.discard
                .iter()
                .map(|&c| card_label(c))
                .collect::<Vec<_>>()
                .join(", ")
        };
        term.print(area.left(), y, &format!("discards: {discards}"));
        y = y.saturating_add(2);

        let viewer = own_seat(view);
        let current = usize::try_from(view.current).unwrap_or(usize::MAX);
        for (seat, hand) in view.hands.iter().enumerate() {
            let marker = if seat == current { '>' } else { ' ' };
            if Some(seat) == viewer {
                term.fg(Color::Ansi(AnsiColor::BrightGreen));
                term.print(
                    area.left(),
                    y,
                    &format!("{marker} p{seat} (you): {}", describe_own_hand(hand)),
                );
                term.reset_style();
            } else {
                term.print(
                    area.left(),
                    y,
                    &format!("{marker} p{seat}: {}", describe_full_hand(hand)),
                );
            }
            y = y.saturating_add(1);
        }

        if view.over {
            y = y.saturating_add(1);
            term.print(
                area.left(),
                y,
                &format!("match over -- final score {score}"),
            );
        }
    }

    fn get_stats(&self, view: &Self::View) -> Vec<(String, String)> {
        let score: u32 = view.fireworks.iter().map(|&r| u32::from(r)).sum();
        vec![
            ("score".to_owned(), score.to_string()),
            ("hints".to_owned(), format!("{}/8", view.hint_tokens)),
            ("fuses".to_owned(), format!("{}/3", view.fuse_tokens)),
            ("deck".to_owned(), view.deck_size.to_string()),
        ]
    }

    fn format_action(&self, action: &Self::Action) -> String {
        match action {
            Action::Play(index) => format!("play {index}"),
            Action::Discard(index) => format!("discard {index}"),
            Action::HintColor(target, color) => {
                format!("hint p{target} color {}", color_label(*color))
            }
            Action::HintRank(target, rank) => format!("hint p{target} rank {rank}"),
            Action::Deal(card) => format!("deal {}", card_label(*card)),
        }
    }
}

/// Renders another seat's hand in full: `"0=Red3, 1=Blue1, ..."`.
fn describe_full_hand(hand: &[VisibleCard]) -> String {
    if hand.is_empty() {
        return "empty".to_owned();
    }
    hand.iter()
        .enumerate()
        .map(|(i, c)| match c {
            VisibleCard::Full(card) => format!("{i}={}", card_label(*card)),
            VisibleCard::Own { .. } => format!("{i}=?"), // never reached in practice
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Renders the viewer's own hand as hint-only slots, e.g.
/// `"0=color? rank3, 1=color?rank?"`.
fn describe_own_hand(hand: &[VisibleCard]) -> String {
    if hand.is_empty() {
        return "empty".to_owned();
    }
    hand.iter()
        .enumerate()
        .map(|(i, c)| match c {
            VisibleCard::Own {
                known_color,
                known_rank,
            } => {
                let color = known_color.map_or_else(|| "?".to_owned(), color_label);
                let rank = known_rank.map_or_else(|| "?".to_owned(), |r| r.to_string());
                format!("{i}=color:{color} rank:{rank}")
            }
            VisibleCard::Full(card) => format!("{i}={}", card_label(*card)), // never reached
        })
        .collect::<Vec<_>>()
        .join(", ")
}
