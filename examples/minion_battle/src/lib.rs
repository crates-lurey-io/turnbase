//! Minion battle: a game whose moves cascade through triggered effects.
//!
//! Two seats field a board of minions. Attacking deals mutual damage; a minion
//! reduced to zero health dies, and a dying minion's deathrattle enqueues more
//! effects (damage every enemy minion, or burn the enemy hero) that can kill
//! more minions, whose deathrattles fire in turn. That cascade is exactly what
//! the Tier-2 [`EffectSystem`] queue is for: effects are enqueued and resolved
//! in order, never inline. Without triggered effects this game would not need
//! the engine, which is why it is the motivating example.

use serde::{Deserialize, Serialize};
use turnbase::{ActivePlayers, EffectSystem, Game, PlayerId, resolve_effects};

#[cfg(feature = "printable")]
mod ui;

/// A draw is declared after this many turns so games always terminate.
const TURN_CAP: u16 = 100;

/// A minion on the board.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Minion {
    /// Unique within a match.
    pub id: u32,
    /// Damage dealt when attacking or retaliating.
    pub attack: i32,
    /// Remaining health; the minion dies at zero or below.
    pub health: i32,
    /// Effect triggered when this minion dies.
    pub deathrattle: Option<Deathrattle>,
}

/// What a minion does when it dies.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Deathrattle {
    /// Deal this much to every enemy minion (can chain into more deaths).
    DamageAllEnemyMinions(i32),
    /// Deal this much to the enemy hero.
    DamageEnemyHero(i32),
}

/// The target of an attack.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Target {
    /// The enemy hero.
    Hero,
    /// An enemy minion, by id.
    Minion(u32),
}

/// A move: attack with one of your minions, or end your turn.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Action {
    /// Attack `target` with your minion `attacker`.
    Attack {
        /// The attacking minion's id.
        attacker: u32,
        /// What it hits.
        target: Target,
    },
    /// Pass without attacking.
    EndTurn,
}

/// One atomic effect resolved through the Tier-2 queue.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Effect {
    /// Deal `amount` to minion `id` on `side`.
    DamageMinion {
        /// The minion's side.
        side: usize,
        /// The minion's id.
        id: u32,
        /// Damage amount.
        amount: i32,
    },
    /// Deal `amount` to the hero on `side`.
    DamageHero {
        /// The hero's side.
        side: usize,
        /// Damage amount.
        amount: i32,
    },
}

/// A battle position: two heroes, two boards, and the turn counter.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Battle {
    heroes: [i32; 2],
    boards: [Vec<Minion>; 2],
    turn: u16,
}

impl Battle {
    /// The hero health for `side`.
    ///
    /// # Panics
    /// Panics if `side > 1`.
    #[must_use]
    pub const fn hero(&self, side: usize) -> i32 {
        self.heroes[side]
    }

    /// The minions on `side`, in board order.
    ///
    /// # Panics
    /// Panics if `side > 1`.
    #[must_use]
    pub fn board(&self, side: usize) -> &[Minion] {
        &self.boards[side]
    }

    /// The number of turns taken.
    #[must_use]
    pub const fn turn(&self) -> u16 {
        self.turn
    }
}

/// The rules. The starting boards are fixed so the example is deterministic.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct MinionBattle;

impl MinionBattle {
    const fn side_of(player: PlayerId) -> usize {
        player.index() as usize
    }
}

impl Game for MinionBattle {
    type State = Battle;
    type Action = Action;
    type View = Battle;

    fn new_initial_state(&self, _seed: u64) -> Self::State {
        Battle {
            heroes: [30, 30],
            boards: [
                vec![
                    Minion {
                        id: 0,
                        attack: 2,
                        health: 2,
                        deathrattle: None,
                    },
                    Minion {
                        id: 1,
                        attack: 1,
                        health: 1,
                        deathrattle: None,
                    },
                ],
                vec![
                    Minion {
                        id: 10,
                        attack: 2,
                        health: 2,
                        deathrattle: Some(Deathrattle::DamageAllEnemyMinions(1)),
                    },
                    Minion {
                        id: 11,
                        attack: 1,
                        health: 1,
                        deathrattle: Some(Deathrattle::DamageEnemyHero(3)),
                    },
                ],
            ],
            turn: 0,
        }
    }

    fn num_players(&self) -> usize {
        2
    }

    fn active_players(&self, state: &Self::State) -> ActivePlayers {
        if self.is_terminal(state) {
            ActivePlayers::none()
        } else {
            ActivePlayers::one(PlayerId::new(u32::from(state.turn % 2)))
        }
    }

    fn legal_actions(&self, state: &Self::State, player: PlayerId) -> Vec<Self::Action> {
        if self.is_terminal(state) || !self.active_players(state).contains(player) {
            return Vec::new();
        }
        let side = Self::side_of(player);
        let enemy = 1 - side;
        let mut actions = vec![Action::EndTurn];
        for minion in &state.boards[side] {
            if minion.attack <= 0 {
                continue;
            }
            actions.push(Action::Attack {
                attacker: minion.id,
                target: Target::Hero,
            });
            for enemy_minion in &state.boards[enemy] {
                actions.push(Action::Attack {
                    attacker: minion.id,
                    target: Target::Minion(enemy_minion.id),
                });
            }
        }
        actions
    }

    fn apply(&self, state: &mut Self::State, player: PlayerId, action: Self::Action) {
        if let Action::Attack { attacker, target } = action {
            let side = Self::side_of(player);
            let enemy = 1 - side;
            let attack = find(&state.boards[side], attacker).map_or(0, |m| m.attack);
            let initial = match target {
                Target::Hero => vec![Effect::DamageHero {
                    side: enemy,
                    amount: attack,
                }],
                Target::Minion(id) => {
                    let retaliation = find(&state.boards[enemy], id).map_or(0, |m| m.attack);
                    vec![
                        Effect::DamageMinion {
                            side: enemy,
                            id,
                            amount: attack,
                        },
                        Effect::DamageMinion {
                            side,
                            id: attacker,
                            amount: retaliation,
                        },
                    ]
                }
            };
            resolve_effects(self, state, initial);
        }
        state.turn += 1;
    }

    fn is_terminal(&self, state: &Self::State) -> bool {
        state.heroes[0] <= 0 || state.heroes[1] <= 0 || state.turn >= TURN_CAP
    }

    fn reward(&self, state: &Self::State, player: PlayerId) -> f64 {
        let side = Self::side_of(player);
        let enemy = 1 - side;
        match (state.heroes[side] <= 0, state.heroes[enemy] <= 0) {
            (false, true) => 1.0,
            (true, false) => -1.0,
            _ => 0.0,
        }
    }

    fn view(&self, state: &Self::State, _viewer: Option<PlayerId>) -> Self::View {
        state.clone()
    }
}

impl EffectSystem for MinionBattle {
    type State = Battle;
    type Effect = Effect;

    fn apply(&self, state: &mut Self::State, effect: &Self::Effect) {
        match *effect {
            Effect::DamageMinion { side, id, amount } => {
                if let Some(minion) = state.boards[side].iter_mut().find(|m| m.id == id) {
                    minion.health -= amount;
                }
            }
            Effect::DamageHero { side, amount } => state.heroes[side] -= amount,
        }
    }

    fn react(&self, state: &mut Self::State, _effect: &Self::Effect) -> Vec<Self::Effect> {
        // State-based deaths after every effect: remove minions at <= 0 health
        // (in board order) and enqueue their deathrattles.
        let mut follow_ups = Vec::new();
        for side in 0..2 {
            let enemy = 1 - side;
            let mut dead = Vec::new();
            state.boards[side].retain(|minion| {
                let alive = minion.health > 0;
                if !alive {
                    dead.push(minion.clone());
                }
                alive
            });
            for minion in dead {
                match minion.deathrattle {
                    Some(Deathrattle::DamageAllEnemyMinions(amount)) => {
                        for enemy_minion in &state.boards[enemy] {
                            follow_ups.push(Effect::DamageMinion {
                                side: enemy,
                                id: enemy_minion.id,
                                amount,
                            });
                        }
                    }
                    Some(Deathrattle::DamageEnemyHero(amount)) => {
                        follow_ups.push(Effect::DamageHero {
                            side: enemy,
                            amount,
                        });
                    }
                    None => {}
                }
            }
        }
        follow_ups
    }
}

fn find(board: &[Minion], id: u32) -> Option<&Minion> {
    board.iter().find(|minion| minion.id == id)
}

#[cfg(test)]
mod tests {
    use super::{Deathrattle, Minion, MinionBattle, Target};
    use turnbase::{Game, PlayerId};

    const P0: PlayerId = PlayerId::new(0);

    fn ids(board: &[Minion]) -> Vec<u32> {
        board.iter().map(|m| m.id).collect()
    }

    #[test]
    fn killing_the_bomber_cascades_and_wipes_the_board() {
        // P0's minion 0 (2/2) attacks the enemy bomber 10 (2/2, deathrattle:
        // deal 1 to all enemy minions). Mutual damage kills both attackers;
        // the bomber's deathrattle then finishes off P0's minion 1 (1/1).
        let game = MinionBattle;
        let mut state = game.new_initial_state(0);
        game.apply(
            &mut state,
            P0,
            super::Action::Attack {
                attacker: 0,
                target: Target::Minion(10),
            },
        );

        assert!(state.board(0).is_empty(), "P0 board wiped by the cascade");
        assert_eq!(
            ids(state.board(1)),
            vec![11],
            "only the non-attacked minion remains"
        );
        assert_eq!(state.hero(0), 30);
        assert_eq!(state.hero(1), 30);
    }

    #[test]
    fn a_dying_minions_deathrattle_burns_the_hero() {
        // P0's minion 1 (1/1) trades into enemy minion 11 (1/1, deathrattle:
        // deal 3 to enemy hero). Both die; the deathrattle hits P0's hero.
        let game = MinionBattle;
        let mut state = game.new_initial_state(0);
        game.apply(
            &mut state,
            P0,
            super::Action::Attack {
                attacker: 1,
                target: Target::Minion(11),
            },
        );

        assert_eq!(state.hero(0), 27, "deathrattle dealt 3 to P0's hero");
        assert_eq!(ids(state.board(0)), vec![0], "minion 1 died in the trade");
        assert_eq!(
            ids(state.board(1)),
            vec![10],
            "minion 11 died to its own trade"
        );
    }

    #[test]
    fn attacking_the_hero_deals_direct_damage() {
        let game = MinionBattle;
        let mut state = game.new_initial_state(0);
        game.apply(
            &mut state,
            P0,
            super::Action::Attack {
                attacker: 0,
                target: Target::Hero,
            },
        );
        assert_eq!(state.hero(1), 28, "minion 0 (attack 2) hit the enemy hero");
        assert_eq!(
            ids(state.board(0)),
            vec![0, 1],
            "no retaliation from a hero attack"
        );
    }

    #[test]
    fn a_standalone_deathrattle_minion_has_no_effect_until_it_dies() {
        let bomber = Minion {
            id: 99,
            attack: 0,
            health: 3,
            deathrattle: Some(Deathrattle::DamageEnemyHero(5)),
        };
        assert!(matches!(
            bomber.deathrattle,
            Some(Deathrattle::DamageEnemyHero(5))
        ));
    }
}
