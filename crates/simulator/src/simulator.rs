//! Headless simulation core: an in-memory turn loop over a [`turnbase::Game`].

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
    /// order. [`crate::SimulationRunner`] uses this once, at construction, to
    /// decide whose [`Game::View`] the dashboard renders from for the whole
    /// match: a human should never see another seat's hidden information
    /// (their cards, say) just because it happened to be an AI's turn when
    /// the frame was drawn.
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
