//! Alpha-beta minimax for two-player, zero-sum, perfect-information games.

use turnbase::{Game, PlayerId, Reversible};

use crate::{Bot, RankedBot};

/// Depth-limited alpha-beta minimax.
///
/// Assumes exactly one active player per node (sequential play) and two-player
/// zero-sum outcomes. Leaves are evaluated with [`Game::reward`], so give the
/// search enough depth to reach terminals (or a game whose `reward` doubles as
/// a heuristic at non-terminal nodes).
///
/// Two search paths are offered and always agree: [`Minimax::best_action`]
/// clones state per node (needs `State: Clone`), and
/// [`Minimax::best_action_unmake`] uses a game's [`Reversible`] make/unmake.
pub struct Minimax {
    max_depth: u32,
}

impl Minimax {
    /// Creates a search that looks up to `max_depth` plies ahead.
    #[must_use]
    pub const fn new(max_depth: u32) -> Self {
        Self { max_depth }
    }

    /// Returns the value-maximizing action for `player` using cloned search.
    pub fn best_action<G>(&self, game: &G, state: &G::State, player: PlayerId) -> Option<G::Action>
    where
        G: Game,
        G::State: Clone,
        G::Action: Clone,
    {
        let mut best = None;
        let mut best_value = f64::NEG_INFINITY;
        let mut alpha = f64::NEG_INFINITY;
        for action in game.legal_actions(state, player) {
            let mut child = state.clone();
            game.apply(&mut child, player, action.clone());
            let value = value_clone(game, &child, player, alpha, f64::INFINITY, self.max_depth);
            if value > best_value {
                best_value = value;
                best = Some(action);
            }
            alpha = alpha.max(best_value);
        }
        best
    }

    /// Returns the value-maximizing action for `player` using make/unmake.
    ///
    /// Mutates `state` during the search but restores it exactly, so `state` is
    /// unchanged on return.
    pub fn best_action_unmake<G>(
        &self,
        game: &G,
        state: &mut G::State,
        player: PlayerId,
    ) -> Option<G::Action>
    where
        G: Reversible,
        G::Action: Clone,
    {
        let mut best = None;
        let mut best_value = f64::NEG_INFINITY;
        let mut alpha = f64::NEG_INFINITY;
        for action in game.legal_actions(state, player) {
            let record = game.apply_undoable(state, player, action.clone());
            let value = value_unmake(game, state, player, alpha, f64::INFINITY, self.max_depth);
            game.undo(state, record);
            if value > best_value {
                best_value = value;
                best = Some(action);
            }
            alpha = alpha.max(best_value);
        }
        best
    }
}

fn value_clone<G>(
    game: &G,
    state: &G::State,
    root: PlayerId,
    mut alpha: f64,
    mut beta: f64,
    depth: u32,
) -> f64
where
    G: Game,
    G::State: Clone,
{
    let Some(active) = leaf_or_active(game, state, depth) else {
        return game.reward(state, root);
    };
    let maximizing = active == root;
    let mut value = bound(maximizing);
    for action in game.legal_actions(state, active) {
        let mut child = state.clone();
        game.apply(&mut child, active, action);
        let child_value = value_clone(game, &child, root, alpha, beta, depth - 1);
        (value, alpha, beta) = tighten(maximizing, value, child_value, alpha, beta);
        if alpha >= beta {
            break;
        }
    }
    value
}

fn value_unmake<G>(
    game: &G,
    state: &mut G::State,
    root: PlayerId,
    mut alpha: f64,
    mut beta: f64,
    depth: u32,
) -> f64
where
    G: Reversible,
{
    let Some(active) = leaf_or_active(game, state, depth) else {
        return game.reward(state, root);
    };
    let maximizing = active == root;
    let mut value = bound(maximizing);
    for action in game.legal_actions(state, active) {
        let record = game.apply_undoable(state, active, action);
        let child_value = value_unmake(game, state, root, alpha, beta, depth - 1);
        game.undo(state, record);
        (value, alpha, beta) = tighten(maximizing, value, child_value, alpha, beta);
        if alpha >= beta {
            break;
        }
    }
    value
}

/// Returns the single active player if the node should be expanded, or `None`
/// if it is a leaf (depth exhausted, terminal, or no active player).
fn leaf_or_active<G: Game>(game: &G, state: &G::State, depth: u32) -> Option<PlayerId> {
    if depth == 0 || game.is_terminal(state) {
        return None;
    }
    game.active_players(state).iter().next()
}

/// The worst possible starting value for a maximizing or minimizing node.
const fn bound(maximizing: bool) -> f64 {
    if maximizing {
        f64::NEG_INFINITY
    } else {
        f64::INFINITY
    }
}

/// Folds one child's value into the running (value, alpha, beta) window.
const fn tighten(
    maximizing: bool,
    value: f64,
    child: f64,
    alpha: f64,
    beta: f64,
) -> (f64, f64, f64) {
    if maximizing {
        let value = value.max(child);
        (value, alpha.max(value), beta)
    } else {
        let value = value.min(child);
        (value, alpha, beta.min(value))
    }
}

impl<G> Bot<G> for Minimax
where
    G: Game,
    G::State: Clone,
    G::Action: Clone,
{
    fn choose(&mut self, game: &G, state: &G::State, player: PlayerId) -> Option<G::Action> {
        self.best_action(game, state, player)
    }
}

impl<G> RankedBot<G> for Minimax
where
    G: Game,
    G::State: Clone,
    G::Action: Clone,
{
    /// Scores each root action with a full alpha-beta window (no cross-sibling
    /// pruning), so every score is the move's exact minimax value rather than a
    /// bound. Costlier than [`Minimax::best_action`]; use it for analysis, not
    /// hot self-play.
    fn rank(&mut self, game: &G, state: &G::State, player: PlayerId) -> Vec<(G::Action, f64)> {
        let mut ranked: Vec<(G::Action, f64)> = game
            .legal_actions(state, player)
            .into_iter()
            .map(|action| {
                let mut child = state.clone();
                game.apply(&mut child, player, action.clone());
                let value = value_clone(
                    game,
                    &child,
                    player,
                    f64::NEG_INFINITY,
                    f64::INFINITY,
                    self.max_depth,
                );
                (action, value)
            })
            .collect();
        // Descending by score; total_cmp gives a deterministic total order.
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
        ranked
    }
}

#[cfg(test)]
mod tests {
    use super::Minimax;
    use crate::{Bot, RandomBot, RankedBot};
    use proptest::prelude::*;
    use tic_tac_toe::{Cell, Move, TicTacToe};
    use turnbase::{Game, PlayerId, Prng, Reversible};

    const P0: PlayerId = PlayerId::new(0);

    /// Plays a full game, seat 0 driven by `x`, seat 1 by `o`. Returns the
    /// terminal board.
    fn run_match<X: Bot<TicTacToe>, O: Bot<TicTacToe>>(
        x: &mut X,
        o: &mut O,
    ) -> <TicTacToe as Game>::State {
        let game = TicTacToe;
        let mut state = game.new_initial_state(0);
        while !game.is_terminal(&state) {
            let player = game.active_players(&state).iter().next().unwrap();
            let action = if player.index() == 0 {
                x.choose(&game, &state, player)
            } else {
                o.choose(&game, &state, player)
            }
            .expect("non-terminal state has a legal action");
            game.apply(&mut state, player, action);
        }
        state
    }

    #[test]
    #[allow(clippy::float_cmp)] // reward() is exactly 0.0 / ±1.0
    fn minimax_never_loses_against_random() {
        let game = TicTacToe;
        for seed in 0..25 {
            let mut x = Minimax::new(9);
            let mut o = RandomBot::new(seed);
            let end = run_match(&mut x, &mut o);
            assert!(
                game.reward(&end, P0) >= 0.0,
                "optimal X lost to random O (seed {seed})"
            );
        }
    }

    #[test]
    #[allow(clippy::float_cmp)] // reward() is exactly 0.0 / ±1.0
    fn optimal_play_is_a_draw() {
        let game = TicTacToe;
        let mut x = Minimax::new(9);
        let mut o = Minimax::new(9);
        let end = run_match(&mut x, &mut o);
        assert!(game.is_terminal(&end));
        assert_eq!(game.reward(&end, P0), 0.0);
    }

    #[test]
    fn clone_and_unmake_choose_the_same_move() {
        let game = TicTacToe;
        let search = Minimax::new(9);
        let mut state = game.new_initial_state(0);
        let script = [4u8, 0, 8, 2, 6];
        for &cell in &script {
            let player = game.active_players(&state).iter().next().unwrap();
            let clone_pick = search.best_action(&game, &state, player);
            let mut scratch = state.clone();
            let unmake_pick = search.best_action_unmake(&game, &mut scratch, player);
            assert_eq!(clone_pick, unmake_pick);
            assert_eq!(scratch, state, "unmake search must leave state unchanged");
            game.apply(&mut state, player, Move(cell));
        }
    }

    #[test]
    #[allow(clippy::float_cmp)] // reward()/minimax values here are exactly 0.0 / ±1.0
    fn rank_puts_the_winning_move_first() {
        // X (seat 0) at 0,1 with an open 2 -> an immediate win; O at 3,4.
        let game = TicTacToe;
        let mut state = game.new_initial_state(0);
        for (seat, cell) in [(0u32, 0u8), (1, 3), (0, 1), (1, 4)] {
            game.apply(&mut state, PlayerId::new(seat), Move(cell));
        }

        let mut search = Minimax::new(9);
        let ranked = search.rank(&game, &state, P0);

        assert_eq!(ranked.len(), game.legal_actions(&state, P0).len());
        assert_eq!(ranked[0].0, Move(2), "the winning move should rank first");
        assert_eq!(ranked[0].1, 1.0);
        assert!(
            ranked.windows(2).all(|w| w[0].1 >= w[1].1),
            "scores must be sorted best-first"
        );
        assert_eq!(
            ranked.first().map(|(a, _)| *a),
            search.best_action(&game, &state, P0),
            "rank().first() agrees with best_action()"
        );
    }

    #[test]
    fn random_bot_plays_only_legal_moves() {
        let game = TicTacToe;
        let mut bot = RandomBot::new(7);
        let mut state = game.new_initial_state(0);
        while !game.is_terminal(&state) {
            let player = game.active_players(&state).iter().next().unwrap();
            let action = bot.choose(&game, &state, player).unwrap();
            assert!(game.is_legal(&state, player, &action));
            game.apply(&mut state, player, action);
        }
    }

    proptest! {
        /// On every reachable position, `apply_undoable` then `undo` restores
        /// the board exactly.
        #[test]
        fn undo_restores_the_board(seed in any::<u64>()) {
            let game = TicTacToe;
            let mut rng = Prng::new(seed);
            let mut state = game.new_initial_state(0);
            while !game.is_terminal(&state) {
                let player = game.active_players(&state).iter().next().unwrap();
                let actions = game.legal_actions(&state, player);
                let index = usize::try_from(rng.below(actions.len() as u64)).unwrap();
                let action = actions[index];

                let before = state.clone();
                let record = game.apply_undoable(&mut state, player, action);
                prop_assert_ne!(&state, &before);
                game.undo(&mut state, record);
                prop_assert_eq!(&state, &before);

                game.apply(&mut state, player, action);
            }
            prop_assert!(matches!(
                game.view(&state, None).cell(0),
                Cell::Empty | Cell::X | Cell::O
            ));
        }
    }
}
