//! The retroglyph dashboard rendering for Spire Run (the `ui` feature).
//!
//! The viewport adapts to the active phase: `Map` shows the path position,
//! `Combat` shows hero/enemy vitals plus the indexed hand and the enemy's
//! telegraphed intent, and `Reward` shows the offered cards. No hidden
//! information exists here (solo run, one seat), so the view is just a clone
//! of the public state, mirroring `minion_battle`'s no-redaction approach.

use retroglyph_core::grid::Rect;
use retroglyph_core::{AnsiColor, Backend, Color, Terminal};
use turnbase_simulator::PrintableGame;

use crate::{Action, CombatAction, Intent, Phase, SpireRun};

impl PrintableGame for SpireRun {
    fn draw_viewport<B: Backend>(&self, view: &Self::View, term: &mut Terminal<B>, area: Rect) {
        let mut y = area.top();
        term.print(area.left(), y, "== Spire Run ==");
        y = y.saturating_add(2);

        match view.phase() {
            Phase::Map => {
                term.print(
                    area.left(),
                    y,
                    &format!(
                        "on the path -- floor {}/{}",
                        view.floor() + 1,
                        ENCOUNTER_COUNT
                    ),
                );
                y = y.saturating_add(1);
                term.print(
                    area.left(),
                    y,
                    &format!("hero: {}/{} hp", view.hero_hp(), view.hero_max_hp()),
                );
            }
            Phase::Combat(combat) => {
                term.fg(Color::Ansi(AnsiColor::BrightYellow));
                term.print(
                    area.left(),
                    y,
                    &format!(
                        "vs {} (floor {}/{}, turn {})",
                        combat.enemy_name(),
                        view.floor() + 1,
                        ENCOUNTER_COUNT,
                        combat.turn()
                    ),
                );
                term.reset_style();
                y = y.saturating_add(2);

                term.print(
                    area.left(),
                    y,
                    &format!(
                        "hero: {} hp, {} block, {} energy",
                        combat.hero_hp(),
                        combat.hero_block(),
                        combat.energy()
                    ),
                );
                y = y.saturating_add(1);
                term.print(
                    area.left(),
                    y,
                    &format!(
                        "enemy: {}/{} hp -- intent: {}",
                        combat.enemy_hp(),
                        combat.enemy_max_hp(),
                        describe_intent(combat.intent())
                    ),
                );
                y = y.saturating_add(2);

                term.print(area.left(), y, "hand:");
                y = y.saturating_add(1);
                for (index, card) in combat.hand().iter().enumerate() {
                    term.print(
                        area.left().saturating_add(2),
                        y,
                        &format!("{index}: {} (cost {})", card.name(), card.cost()),
                    );
                    y = y.saturating_add(1);
                }
            }
            Phase::Reward(reward) => {
                term.print(area.left(), y, "victory! choose a reward:");
                y = y.saturating_add(1);
                match &reward.offer {
                    Some(offer) => {
                        for (index, card) in offer.iter().enumerate() {
                            term.print(
                                area.left().saturating_add(2),
                                y,
                                &format!("{index}: {}", card.name()),
                            );
                            y = y.saturating_add(1);
                        }
                        term.print(area.left().saturating_add(2), y, "or heal / skip");
                    }
                    None => {
                        term.print(area.left(), y, "(revealing offer...)");
                    }
                }
            }
            Phase::GameOver { won } => {
                if *won {
                    term.fg(Color::Ansi(AnsiColor::BrightGreen));
                    term.print(area.left(), y, "the spire is cleared!");
                } else {
                    term.fg(Color::Ansi(AnsiColor::BrightRed));
                    term.print(area.left(), y, "the hero has fallen.");
                }
                term.reset_style();
            }
        }
    }

    fn get_stats(&self, view: &Self::View) -> Vec<(String, String)> {
        vec![
            ("phase".to_owned(), phase_name(view.phase()).to_owned()),
            (
                "hp".to_owned(),
                format!("{}/{}", view.hero_hp(), view.hero_max_hp()),
            ),
            (
                "floor".to_owned(),
                format!("{}/{}", view.floor() + 1, ENCOUNTER_COUNT),
            ),
            ("deck size".to_owned(), view.deck().len().to_string()),
        ]
    }

    fn format_action(&self, action: &Self::Action) -> String {
        match action {
            Action::Advance => "advance".to_owned(),
            Action::Combat(CombatAction::Play(index)) => format!("play hand[{index}]"),
            Action::Combat(CombatAction::EndTurn) => "end turn".to_owned(),
            Action::Offer(index) => format!("reveal offer {index}"),
            Action::Take(index) => format!("take offer[{index}]"),
            Action::Heal => "heal".to_owned(),
            Action::Skip => "skip".to_owned(),
        }
    }
}

/// Number of encounters on the path, including the boss.
const ENCOUNTER_COUNT: u8 = 4;

const fn phase_name(phase: &Phase) -> &'static str {
    match phase {
        Phase::Map => "map",
        Phase::Combat(_) => "combat",
        Phase::Reward(_) => "reward",
        Phase::GameOver { won: true } => "cleared",
        Phase::GameOver { won: false } => "dead",
    }
}

fn describe_intent(intent: Intent) -> String {
    match intent {
        Intent::Attack(amount) => format!("attack for {amount}"),
        Intent::Defend(amount) => format!("brace for {amount} block"),
    }
}
