//! The nested combat mini-game.
//!
//! Combat is one phase of the top-level run, but it is a complete mini-game
//! with its own turn loop (hero plays cards against energy, then a scripted
//! enemy resolves its telegraphed intent), so it gets its own state and step
//! function rather than bloating the run-level `Game` impl. The top-level
//! `SpireRun::apply` just dispatches `Action::Combat(_)` into
//! [`CombatState::apply`] while `Phase::Combat` is active, per
//! ARCHITECTURE.md's "multi-phase run structures" convention (composition,
//! not a new trait).

use serde::{Deserialize, Serialize};
use turnbase::{Pile, Prng};

/// Energy granted to the hero at the start of every combat turn.
pub const ENERGY_PER_TURN: u8 = 3;

/// Cards drawn into the hero's hand at the start of every combat turn.
const HAND_SIZE: usize = 5;

/// Combat is declared a loss if it runs this many hero turns without a
/// decisive result, so a pathological deck (all `Defend`, say) still
/// terminates a match rather than looping forever.
pub const TURN_CAP: u16 = 50;

/// Amount of block gained by an enemy `Defend` intent.
const ENEMY_DEFEND_BLOCK: i32 = 5;

/// A card in the hero's deck.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum CardKind {
    /// Deal 6 damage. Costs 1 energy.
    Strike,
    /// Gain 5 block. Costs 1 energy.
    Defend,
    /// Deal 8 damage. Costs 2 energy.
    Bash,
}

impl CardKind {
    /// Energy cost to play this card.
    #[must_use]
    pub const fn cost(self) -> u8 {
        match self {
            Self::Strike | Self::Defend => 1,
            Self::Bash => 2,
        }
    }

    /// Short display name, used by the CLI/UI.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Strike => "Strike",
            Self::Defend => "Defend",
            Self::Bash => "Bash",
        }
    }
}

/// The enemy's telegraphed next move, visible to the hero before they act.
///
/// Slay the Spire's "intent" convention. Set from the in-state generator at
/// the start of each hero turn: a cheap, uncommitted roll per ARCHITECTURE.md
/// ("Randomness, part 2"), since no strategic decision branches on *predicting*
/// it before it is revealed -- only on reacting to the revealed value.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Intent {
    /// Attack for this much, reduced by the hero's block.
    Attack(i32),
    /// Gain this much block; the enemy does not attack this turn.
    Defend(i32),
}

/// One enemy archetype: starting health and base attack. `name` and the
/// numbers are fixed per encounter in `ENCOUNTERS` (in `lib.rs`).
#[derive(Clone, Copy, Debug)]
pub struct EnemyKind {
    /// Display name.
    pub name: &'static str,
    /// Starting and maximum health.
    pub max_hp: i32,
    /// Base attack used to roll telegraphed intents.
    pub attack: i32,
}

/// One decision inside combat: play a hand card, or end the turn.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum CombatAction {
    /// Play the card at this index in the hero's hand.
    Play(usize),
    /// End the hero's turn, letting the enemy resolve its telegraphed intent.
    EndTurn,
}

/// The result of one [`CombatState::apply`] call.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// Combat continues.
    Ongoing,
    /// The enemy's health reached zero: the hero won the encounter.
    HeroWon,
    /// The hero's health reached zero, or the turn cap was hit.
    HeroLost,
}

/// One combat encounter: hero resources plus the enemy and its telegraphed
/// intent.
///
/// Owns its own draw/hand/discard piles for the fight; the hero's deck
/// composition (which cards exist at all) lives on the run-level state and is
/// copied in at [`CombatState::start`].
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct CombatState {
    hero_hp: i32,
    hero_max_hp: i32,
    hero_block: i32,
    energy: u8,
    draw: Pile<CardKind>,
    hand: Pile<CardKind>,
    discard: Pile<CardKind>,
    enemy_name: String,
    enemy_hp: i32,
    enemy_max_hp: i32,
    enemy_block: i32,
    enemy_attack: i32,
    intent: Intent,
    turn: u16,
}

impl CombatState {
    /// Starts a fresh encounter: shuffles a copy of `deck` into the draw pile,
    /// draws the opening hand, and telegraphs the enemy's first intent.
    #[must_use]
    pub fn start(
        hero_hp: i32,
        hero_max_hp: i32,
        deck: &[CardKind],
        enemy: EnemyKind,
        rng: &mut Prng,
    ) -> Self {
        let mut draw: Pile<CardKind> = deck.iter().copied().collect();
        draw.shuffle(rng);
        let mut state = Self {
            hero_hp,
            hero_max_hp,
            hero_block: 0,
            energy: ENERGY_PER_TURN,
            draw,
            hand: Pile::new(),
            discard: Pile::new(),
            enemy_name: enemy.name.to_owned(),
            enemy_hp: enemy.max_hp,
            enemy_max_hp: enemy.max_hp,
            enemy_block: 0,
            enemy_attack: enemy.attack,
            intent: Intent::Attack(enemy.attack),
            turn: 1,
        };
        state.draw_up_to_hand_size(rng);
        state.intent = roll_intent(state.enemy_attack, rng);
        state
    }

    /// The hero's current health.
    #[must_use]
    pub const fn hero_hp(&self) -> i32 {
        self.hero_hp
    }

    /// The hero's maximum health.
    #[must_use]
    pub const fn hero_max_hp(&self) -> i32 {
        self.hero_max_hp
    }

    /// The hero's current block (absorbs incoming damage before health).
    #[must_use]
    pub const fn hero_block(&self) -> i32 {
        self.hero_block
    }

    /// Energy remaining this turn.
    #[must_use]
    pub const fn energy(&self) -> u8 {
        self.energy
    }

    /// The hero's hand, in play order.
    #[must_use]
    pub fn hand(&self) -> &[CardKind] {
        self.hand.as_slice()
    }

    /// Cards left in the draw pile.
    #[must_use]
    pub const fn draw_len(&self) -> usize {
        self.draw.len()
    }

    /// Cards in the discard pile.
    #[must_use]
    pub const fn discard_len(&self) -> usize {
        self.discard.len()
    }

    /// The enemy's display name.
    #[must_use]
    pub fn enemy_name(&self) -> &str {
        &self.enemy_name
    }

    /// The enemy's current health.
    #[must_use]
    pub const fn enemy_hp(&self) -> i32 {
        self.enemy_hp
    }

    /// The enemy's maximum health.
    #[must_use]
    pub const fn enemy_max_hp(&self) -> i32 {
        self.enemy_max_hp
    }

    /// The enemy's telegraphed next move.
    #[must_use]
    pub const fn intent(&self) -> Intent {
        self.intent
    }

    /// The current hero turn number (starts at 1).
    #[must_use]
    pub const fn turn(&self) -> u16 {
        self.turn
    }

    /// Returns the actions available to the hero right now: `EndTurn`, plus
    /// `Play(i)` for every hand card whose energy cost fits.
    #[must_use]
    pub fn legal_actions(&self) -> Vec<CombatAction> {
        let mut actions = vec![CombatAction::EndTurn];
        for (index, card) in self.hand.as_slice().iter().enumerate() {
            if card.cost() <= self.energy {
                actions.push(CombatAction::Play(index));
            }
        }
        actions
    }

    /// Advances combat by one hero decision. Drawing randomness (the enemy's
    /// next telegraphed intent, and any discard reshuffle) comes from `rng`,
    /// the run's own generator, so a replayed seed reproduces the same fight.
    pub fn apply(&mut self, action: CombatAction, rng: &mut Prng) -> Outcome {
        match action {
            CombatAction::Play(index) => self.play(index),
            CombatAction::EndTurn => self.end_turn(rng),
        }
    }

    fn play(&mut self, index: usize) -> Outcome {
        let Some(&card) = self.hand.as_slice().get(index) else {
            return Outcome::Ongoing;
        };
        if card.cost() > self.energy {
            return Outcome::Ongoing;
        }
        self.hand.remove(index);
        self.energy -= card.cost();
        match card {
            CardKind::Strike => self.damage_enemy(6),
            CardKind::Bash => self.damage_enemy(8),
            CardKind::Defend => self.hero_block += 5,
        }
        self.discard.put(card);
        if self.enemy_hp <= 0 {
            Outcome::HeroWon
        } else {
            Outcome::Ongoing
        }
    }

    fn end_turn(&mut self, rng: &mut Prng) -> Outcome {
        match self.intent {
            Intent::Attack(amount) => self.damage_hero(amount),
            Intent::Defend(amount) => self.enemy_block += amount,
        }
        if self.hero_hp <= 0 {
            return Outcome::HeroLost;
        }

        self.turn += 1;
        if self.turn > TURN_CAP {
            return Outcome::HeroLost;
        }

        self.hero_block = 0;
        self.energy = ENERGY_PER_TURN;
        self.draw_up_to_hand_size(rng);
        self.intent = roll_intent(self.enemy_attack, rng);
        Outcome::Ongoing
    }

    /// Discards the remaining hand, then draws back up to [`HAND_SIZE`],
    /// reshuffling the discard pile into the draw pile if it runs dry.
    fn draw_up_to_hand_size(&mut self, rng: &mut Prng) {
        while let Some(card) = self.hand.draw() {
            self.discard.put(card);
        }
        while self.hand.len() < HAND_SIZE {
            if self.draw.is_empty() {
                if self.discard.is_empty() {
                    break;
                }
                while let Some(card) = self.discard.draw() {
                    self.draw.put(card);
                }
                self.draw.shuffle(rng);
            }
            match self.draw.draw() {
                Some(card) => self.hand.put(card),
                None => break,
            }
        }
    }

    const fn damage_enemy(&mut self, amount: i32) {
        let past_block = amount - self.enemy_block;
        if past_block > 0 {
            self.enemy_hp -= past_block;
            self.enemy_block = 0;
        } else {
            self.enemy_block -= amount;
        }
    }

    const fn damage_hero(&mut self, amount: i32) {
        let past_block = amount - self.hero_block;
        if past_block > 0 {
            self.hero_hp -= past_block;
            self.hero_block = 0;
        } else {
            self.hero_block -= amount;
        }
    }
}

/// Rolls the enemy's next telegraphed intent from `rng`: mostly its base
/// attack, sometimes a heavier attack, occasionally a defensive turn.
const fn roll_intent(base_attack: i32, rng: &mut Prng) -> Intent {
    match rng.below(4) {
        0 => Intent::Defend(ENEMY_DEFEND_BLOCK),
        1 => Intent::Attack(base_attack * 2),
        _ => Intent::Attack(base_attack),
    }
}

#[cfg(test)]
mod tests {
    use super::{CardKind, CombatAction, EnemyKind, Outcome};
    use turnbase::Prng;

    const ENEMY: EnemyKind = EnemyKind {
        name: "Test Dummy",
        max_hp: 20,
        attack: 5,
    };

    fn deck() -> Vec<CardKind> {
        vec![
            CardKind::Strike,
            CardKind::Strike,
            CardKind::Strike,
            CardKind::Strike,
            CardKind::Strike,
            CardKind::Defend,
            CardKind::Defend,
            CardKind::Defend,
            CardKind::Defend,
            CardKind::Bash,
        ]
    }

    #[test]
    fn a_strike_deals_six_damage() {
        let mut rng = Prng::new(1);
        let mut combat = super::CombatState::start(50, 50, &deck(), ENEMY, &mut rng);
        let before = combat.enemy_hp();
        let strike_index = combat
            .hand()
            .iter()
            .position(|c| *c == CardKind::Strike)
            .expect("starter deck has a strike");
        combat.apply(CombatAction::Play(strike_index), &mut rng);
        assert_eq!(combat.enemy_hp(), before - 6);
    }

    #[test]
    fn a_defend_grants_block() {
        let mut rng = Prng::new(2);
        let mut combat = super::CombatState::start(50, 50, &deck(), ENEMY, &mut rng);
        let defend_index = combat
            .hand()
            .iter()
            .position(|c| *c == CardKind::Defend)
            .expect("starter deck has a defend");
        combat.apply(CombatAction::Play(defend_index), &mut rng);
        assert_eq!(combat.hero_block(), 5);
    }

    #[test]
    fn energy_limits_plays_per_turn() {
        // Greedily play the cheapest-first legal card each step; total energy
        // spent this turn must never exceed `ENERGY_PER_TURN`, and once no
        // affordable card remains, only `EndTurn` is legal.
        let mut rng = Prng::new(3);
        let mut combat = super::CombatState::start(50, 50, &deck(), ENEMY, &mut rng);
        let mut spent = 0u8;
        while let Some(CombatAction::Play(index)) = combat
            .legal_actions()
            .into_iter()
            .find(|a| matches!(a, CombatAction::Play(_)))
        {
            let cost = combat.hand()[index].cost();
            combat.apply(CombatAction::Play(index), &mut rng);
            spent += cost;
            assert!(
                spent <= super::ENERGY_PER_TURN,
                "overspent this turn's energy"
            );
        }
        assert_eq!(
            combat.legal_actions(),
            vec![CombatAction::EndTurn],
            "only EndTurn remains once no card is affordable"
        );
    }

    #[test]
    fn ending_the_turn_lets_the_scripted_enemy_act() {
        let mut rng = Prng::new(4);
        let mut combat = super::CombatState::start(50, 50, &deck(), ENEMY, &mut rng);
        let hp_before = combat.hero_hp();
        combat.apply(CombatAction::EndTurn, &mut rng);
        // Either the enemy attacked (hp dropped) or it braced (hp unchanged);
        // either way the turn counter advanced.
        assert!(combat.hero_hp() <= hp_before);
        assert_eq!(combat.turn(), 2);
    }

    #[test]
    fn a_relentless_scripted_enemy_can_kill_the_hero() {
        // Low hero HP against a hard-hitting enemy: repeatedly ending the
        // turn (never blocking) must eventually lose.
        let hard_hitter = EnemyKind {
            name: "Brute",
            max_hp: 999,
            attack: 20,
        };
        let mut rng = Prng::new(5);
        let mut combat = super::CombatState::start(10, 10, &deck(), hard_hitter, &mut rng);
        let mut outcome = Outcome::Ongoing;
        for _ in 0..super::TURN_CAP {
            outcome = combat.apply(CombatAction::EndTurn, &mut rng);
            if outcome != Outcome::Ongoing {
                break;
            }
        }
        assert_eq!(outcome, Outcome::HeroLost);
    }

    #[test]
    fn the_turn_cap_forces_termination() {
        // A do-nothing hero against an enemy too weak to ever kill them (0
        // attack rolls only ever brace) must still terminate via the cap.
        let harmless = EnemyKind {
            name: "Statue",
            max_hp: 999_999,
            attack: 0,
        };
        let mut rng = Prng::new(6);
        let mut combat = super::CombatState::start(999, 999, &deck(), harmless, &mut rng);
        let mut outcome = Outcome::Ongoing;
        for _ in 0..(super::TURN_CAP + 5) {
            outcome = combat.apply(CombatAction::EndTurn, &mut rng);
            if outcome != Outcome::Ongoing {
                break;
            }
        }
        assert_eq!(outcome, Outcome::HeroLost, "turn cap must end the fight");
    }
}
