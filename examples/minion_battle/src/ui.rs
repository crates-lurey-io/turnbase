//! The retroglyph dashboard rendering for Minion Battle (the `ui` feature).
//!
//! No hidden information: both boards are public, so the viewport shows each
//! hero's health and every minion's attack/health (and deathrattle), with the
//! side to move highlighted.

use retroglyph_core::grid::Rect;
use retroglyph_core::{AnsiColor, Backend, Color, Terminal};
use turnbase_simulator::PrintableGame;

use crate::{Action, Deathrattle, Minion, MinionBattle, Target};

impl PrintableGame for MinionBattle {
    fn draw_viewport<B: Backend>(&self, view: &Self::View, term: &mut Terminal<B>, area: Rect) {
        let mut y = area.top();
        term.print(area.left(), y, "== Minion Battle ==");
        y = y.saturating_add(2);

        let current = usize::from(view.turn() % 2);
        term.fg(Color::Ansi(AnsiColor::BrightYellow));
        term.print(
            area.left(),
            y,
            &format!("p{current} to move (turn {})", view.turn()),
        );
        term.reset_style();
        y = y.saturating_add(2);

        let indent = area.left().saturating_add(2);
        for side in 0..2usize {
            let deciding = side == current;
            let marker = if deciding { '>' } else { ' ' };
            if deciding {
                term.fg(Color::Ansi(AnsiColor::BrightGreen));
            }
            term.print(
                area.left(),
                y,
                &format!("{marker} p{side} hero: {} hp", view.hero(side)),
            );
            term.reset_style();
            y = y.saturating_add(1);

            let minions = view.board(side);
            if minions.is_empty() {
                term.print(indent, y, "(no minions)");
                y = y.saturating_add(1);
            } else {
                for minion in minions {
                    term.print(indent, y, &describe_minion(minion));
                    y = y.saturating_add(1);
                }
            }
            y = y.saturating_add(1);
        }
    }

    fn get_stats(&self, view: &Self::View) -> Vec<(String, String)> {
        vec![
            ("turn".to_owned(), view.turn().to_string()),
            ("p0 hero".to_owned(), view.hero(0).to_string()),
            ("p1 hero".to_owned(), view.hero(1).to_string()),
            ("p0 minions".to_owned(), view.board(0).len().to_string()),
            ("p1 minions".to_owned(), view.board(1).len().to_string()),
        ]
    }

    fn format_action(&self, action: &Self::Action) -> String {
        match action {
            Action::Attack { attacker, target } => match target {
                Target::Hero => format!("minion {attacker} hits enemy hero"),
                Target::Minion(id) => format!("minion {attacker} hits enemy minion {id}"),
            },
            Action::EndTurn => "end turn".to_owned(),
        }
    }
}

/// Renders a minion as `#id A/H` plus its deathrattle, if any.
fn describe_minion(minion: &Minion) -> String {
    let deathrattle = match minion.deathrattle {
        Some(Deathrattle::DamageAllEnemyMinions(amount)) => {
            format!("  [dr: {amount} to all enemy minions]")
        }
        Some(Deathrattle::DamageEnemyHero(amount)) => format!("  [dr: {amount} to enemy hero]"),
        None => String::new(),
    };
    format!(
        "#{} {}/{}{}",
        minion.id, minion.attack, minion.health, deathrattle
    )
}
