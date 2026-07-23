//! In-memory turn loop over a [`turnbase::Game`]: seat agents plus a stepper.

use std::collections::HashMap;
use std::fmt::Debug;

use turnbase::{Error, Game, PlayerId};
use turnbase_bots::Bot;

/// Who is deciding a seat's moves.
///
/// A seat with no entry in [`Simulator`]'s agent map behaves like [`Human`]:
/// it blocks [`Simulator::step`] rather than panicking or being skipped, so a
/// game can add players without wiring every seat up front.
///
/// [`Human`]: PlayerAgent::Human
pub enum PlayerAgent<G: Game> {
    /// A seat driven by external input via [`Simulator::select_human_action`].
    Human,
    /// A seat driven by a [`Bot`], asked for a move every time it is active.
    Ai(Box<dyn Bot<G>>),
}

/// Coordinates one match: the rules (`G`), its current position, which agent
/// controls each seat, and a running log of committed actions.
///
/// Pure in-memory bookkeeping: [`Simulator::step`] and
/// [`Simulator::select_human_action`] are the only ways the position changes,
/// and both just forward to [`Game::apply`]. There is no terminal, socket, or
/// timer here, so a simulator runs identically under `cargo test` and under
/// the `ui`-feature dashboard.
pub struct Simulator<G: Game> {
    game: G,
    state: G::State,
    agents: HashMap<PlayerId, PlayerAgent<G>>,
    log_history: Vec<String>,
}

impl<G: Game> Simulator<G> {
    /// Starts a match: `game.new_initial_state(seed)` seeded with `seed`, with
    /// `agents` controlling each seat.
    #[must_use]
    pub fn new(game: G, seed: u64, agents: HashMap<PlayerId, PlayerAgent<G>>) -> Self {
        let state = game.new_initial_state(seed);
        Self {
            game,
            state,
            agents,
            log_history: Vec::new(),
        }
    }

    /// Returns the rules governing this match.
    #[must_use]
    pub const fn game(&self) -> &G {
        &self.game
    }

    /// Returns the current position.
    #[must_use]
    pub const fn state(&self) -> &G::State {
        &self.state
    }

    /// Returns every action committed so far, oldest first, formatted for
    /// display (a log monitor, a test assertion) rather than for replay.
    #[must_use]
    pub fn log_history(&self) -> &[String] {
        &self.log_history
    }

    /// Returns the seat a human is expected to act for right now, if any.
    ///
    /// The first (lowest seat index) active [`PlayerAgent::Human`] seat, since
    /// [`turnbase::ActivePlayers`] iterates in ascending order; simultaneous
    /// phases with more than one human seat resolve one at a time, same as
    /// [`Simulator::step`] resolves AI seats one at a time.
    #[must_use]
    pub fn awaiting_human(&self) -> Option<PlayerId> {
        self.game
            .active_players(&self.state)
            .iter()
            .find(|player| matches!(self.agents.get(player), Some(PlayerAgent::Human) | None))
    }

    /// Returns whether the match has ended.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        self.game.is_terminal(&self.state)
    }

    /// Returns the lowest-indexed seat controlled by [`PlayerAgent::Human`],
    /// if any.
    ///
    /// Iterates a [`HashMap`], so this picks by seat index rather than
    /// insertion order, keeping the result deterministic regardless of hash
    /// order. `turnbase-simulator`'s dashboard uses this once, at
    /// construction, to decide whose [`Game::View`] it renders from for the
    /// whole match: a human should never see another seat's hidden
    /// information (their cards, say) just because it happened to be an AI's
    /// turn when the frame was drawn.
    #[must_use]
    pub fn primary_human(&self) -> Option<PlayerId> {
        self.agents
            .iter()
            .filter_map(|(&player, agent)| matches!(agent, PlayerAgent::Human).then_some(player))
            .min()
    }
}

// Logging formats the committed action with `{action:?}`, so these two entry
// points need `Action: Debug`; every other method above works for any game.
impl<G: Game> Simulator<G>
where
    G::Action: Debug,
{
    /// Advances the match by one atomic decision.
    ///
    /// Returns `Ok(true)` if the active seat was AI-controlled and its action
    /// was committed, `Ok(false)` if the match is already over or the active
    /// seat is waiting on [`Simulator::select_human_action`], or `Err` if the
    /// bot chose an action [`Game::is_legal`] rejects.
    ///
    /// # Errors
    /// Returns [`Error::IllegalAction`] if the active bot's chosen action is
    /// not legal for it in the current state.
    pub fn step(&mut self) -> Result<bool, Error> {
        if self.game.is_terminal(&self.state) {
            return Ok(false);
        }
        let Some(player) = self.game.active_players(&self.state).iter().next() else {
            return Ok(false);
        };
        let Some(PlayerAgent::Ai(bot)) = self.agents.get_mut(&player) else {
            return Ok(false);
        };
        let Some(action) = bot.choose(&self.game, &self.state, player) else {
            return Ok(false);
        };
        if !self.game.is_legal(&self.state, player, &action) {
            return Err(Error::IllegalAction { player });
        }
        log::debug!("{player} chose: {action:?}");
        self.log_history.push(format!("{player} chose: {action:?}"));
        self.game.apply(&mut self.state, player, action);
        Ok(true)
    }

    /// Externally forces `player`'s action into the state machine.
    ///
    /// For a [`PlayerAgent::Human`] seat's UI (or a test) to commit a choice
    /// once [`Simulator::awaiting_human`] names that seat.
    ///
    /// # Errors
    /// Returns [`Error::NotActive`] if `player` is not currently owed a
    /// decision, or [`Error::IllegalAction`] if `action` is not legal for
    /// them.
    pub fn select_human_action(
        &mut self,
        player: PlayerId,
        action: G::Action,
    ) -> Result<(), Error> {
        if !self.game.active_players(&self.state).contains(player) {
            return Err(Error::NotActive { player });
        }
        if !self.game.is_legal(&self.state, player, &action) {
            return Err(Error::IllegalAction { player });
        }
        log::debug!("{player} chose: {action:?}");
        self.log_history.push(format!("{player} chose: {action:?}"));
        self.game.apply(&mut self.state, player, action);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use turnbase::{ActivePlayers, Game, PlayerId};
    use turnbase_bots::RandomBot;

    use super::{PlayerAgent, Simulator};

    const P0: PlayerId = PlayerId::new(0);
    const P1: PlayerId = PlayerId::new(1);

    /// Two seats alternately add 1 to a shared total; whoever reaches 3 wins.
    /// A self-contained game so this crate's tests need no example crate.
    struct CountToThree;

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    struct Bump;

    impl Game for CountToThree {
        type State = u32;
        type Action = Bump;
        type View = u32;

        fn new_initial_state(&self, _seed: u64) -> Self::State {
            0
        }
        fn num_players(&self) -> usize {
            2
        }
        fn active_players(&self, state: &Self::State) -> ActivePlayers {
            if self.is_terminal(state) {
                ActivePlayers::none()
            } else {
                ActivePlayers::one(PlayerId::new(state % 2))
            }
        }
        fn legal_actions(&self, state: &Self::State, _player: PlayerId) -> Vec<Self::Action> {
            if self.is_terminal(state) {
                Vec::new()
            } else {
                vec![Bump]
            }
        }
        fn apply(&self, state: &mut Self::State, _player: PlayerId, _action: Self::Action) {
            *state += 1;
        }
        fn is_terminal(&self, state: &Self::State) -> bool {
            *state >= 3
        }
        fn reward(&self, state: &Self::State, player: PlayerId) -> f64 {
            let winner = (state + 1) % 2;
            if player.index() == winner { 1.0 } else { -1.0 }
        }
        fn view(&self, state: &Self::State, _viewer: Option<PlayerId>) -> Self::View {
            *state
        }
    }

    fn ai_agents() -> HashMap<PlayerId, PlayerAgent<CountToThree>> {
        let mut agents = HashMap::new();
        agents.insert(P0, PlayerAgent::Ai(Box::new(RandomBot::new(1))));
        agents.insert(P1, PlayerAgent::Ai(Box::new(RandomBot::new(2))));
        agents
    }

    #[test]
    fn steps_all_ai_seats_to_a_terminal_state() {
        let mut sim = Simulator::new(CountToThree, 0, ai_agents());
        let mut steps = 0;
        while !sim.is_terminal() && sim.step().unwrap() {
            steps += 1;
        }
        assert_eq!(steps, 3, "three bumps reach the target from zero");
        assert!(sim.is_terminal());
        assert_eq!(sim.log_history().len(), 3);
    }

    #[test]
    fn step_blocks_on_a_human_seat_until_driven() {
        let mut agents: HashMap<PlayerId, PlayerAgent<CountToThree>> = HashMap::new();
        agents.insert(P0, PlayerAgent::Human);
        agents.insert(P1, PlayerAgent::Ai(Box::new(RandomBot::new(7))));
        let mut sim = Simulator::new(CountToThree, 0, agents);

        assert_eq!(sim.awaiting_human(), Some(P0));
        assert_eq!(sim.step(), Ok(false), "step refuses to act for a human");
        assert!(sim.log_history().is_empty());

        sim.select_human_action(P0, Bump).unwrap();
        assert_eq!(sim.awaiting_human(), None);
        assert!(sim.step().unwrap(), "the AI seat advances once unblocked");
        assert_eq!(sim.log_history().len(), 2);
    }

    #[test]
    fn primary_human_picks_the_lowest_seat() {
        let mut agents: HashMap<PlayerId, PlayerAgent<CountToThree>> = HashMap::new();
        agents.insert(P0, PlayerAgent::Ai(Box::new(RandomBot::new(1))));
        agents.insert(P1, PlayerAgent::Human);
        let sim = Simulator::new(CountToThree, 0, agents);
        assert_eq!(sim.primary_human(), Some(P1));
    }
}
