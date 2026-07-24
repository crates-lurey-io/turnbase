//! The retroglyph dashboard rendering for Risk (the `ui` feature).
//!
//! Risk has no hidden information, so the viewport renders the whole map from
//! any seat's view: the three triangular continents, each territory's owner and
//! army count, and a per-seat territory/army summary with the seat to move
//! highlighted.

use retroglyph_core::grid::Rect;
use retroglyph_core::{AnsiColor, Backend, Color, Terminal};
use turnbase::Game;
use turnbase_simulator::PrintableGame;

use crate::{Action, NAMES, Risk, RiskView};

impl PrintableGame for Risk {
    fn draw_viewport<B: Backend>(&self, view: &Self::View, term: &mut Terminal<B>, area: Rect) {
        let mut y = area.top();
        term.print(area.left(), y, "== Risk ==");
        y = y.saturating_add(2);

        term.fg(Color::Ansi(AnsiColor::BrightYellow));
        term.print(
            area.left(),
            y,
            &format!("p{} to move, {:?} phase", view.current, view.phase),
        );
        y = y.saturating_add(1);
        if view.reinforcements > 0 {
            term.print(
                area.left(),
                y,
                &format!("reinforcements to place: {}", view.reinforcements),
            );
            y = y.saturating_add(1);
        }
        term.reset_style();
        y = y.saturating_add(1);

        // Territories, one continent (three territories) per row.
        for continent in 0..3usize {
            let line = (0..3usize)
                .map(|slot| {
                    let t = continent * 3 + slot;
                    format!("{}:p{}x{}", NAMES[t], view.owner[t], view.armies[t])
                })
                .collect::<Vec<_>>()
                .join("   ");
            term.print(area.left(), y, &line);
            y = y.saturating_add(1);
        }
        y = y.saturating_add(1);

        for seat in 0..self.num_players() {
            let owned = territories_owned(view, seat);
            let armies = armies_owned(view, seat);
            let deciding = usize::from(view.current) == seat;
            let marker = if deciding { '>' } else { ' ' };
            if deciding {
                term.fg(Color::Ansi(AnsiColor::BrightGreen));
            }
            term.print(
                area.left(),
                y,
                &format!("{marker} p{seat}: {owned} territories, {armies} armies"),
            );
            term.reset_style();
            y = y.saturating_add(1);
        }
    }

    fn get_stats(&self, view: &Self::View) -> Vec<(String, String)> {
        let mut stats = vec![
            ("turn".to_owned(), format!("p{}", view.current)),
            ("phase".to_owned(), format!("{:?}", view.phase)),
            ("place".to_owned(), view.reinforcements.to_string()),
        ];
        for seat in 0..self.num_players() {
            stats.push((
                format!("p{seat} terr"),
                territories_owned(view, seat).to_string(),
            ));
        }
        stats
    }

    fn format_action(&self, action: &Self::Action) -> String {
        match action {
            Action::Place(t) => format!("place on {}", NAMES[usize::from(*t)]),
            Action::Attack(from, to) => {
                format!(
                    "attack {} from {}",
                    NAMES[usize::from(*to)],
                    NAMES[usize::from(*from)]
                )
            }
            Action::EndAttack => "end attacks".to_owned(),
            Action::Fortify(from, to) => {
                format!(
                    "fortify {} from {}",
                    NAMES[usize::from(*to)],
                    NAMES[usize::from(*from)]
                )
            }
            Action::EndTurn => "end turn".to_owned(),
        }
    }
}

/// Territories `seat` owns.
fn territories_owned(view: &RiskView, seat: usize) -> usize {
    view.owner
        .iter()
        .filter(|&&owner| usize::from(owner) == seat)
        .count()
}

/// Total armies across `seat`'s territories.
fn armies_owned(view: &RiskView, seat: usize) -> u32 {
    view.owner
        .iter()
        .zip(&view.armies)
        .filter(|&(&owner, _)| usize::from(owner) == seat)
        .map(|(_, &armies)| armies)
        .sum()
}
