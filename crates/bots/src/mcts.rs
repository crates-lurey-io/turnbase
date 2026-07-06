//! Monte Carlo tree search (UCT) with chance-node support.

use std::f64::consts::SQRT_2;

use turnbase::{Game, PlayerId, Prng, sample_chance};

use crate::{Bot, RankedBot};

/// UCT Monte Carlo tree search for sequential games, with chance nodes.
///
/// Assumes exactly one active player per decision node (like [`Minimax`]) and
/// two-player zero-sum outcomes. All node values are kept from a fixed root
/// player's perspective: the root maximizes, the opponent minimizes (a sign
/// flip in selection), and chance nodes average their sampled children, which
/// is exactly the expectiminimax behavior. Chance nodes are descended by
/// sampling [`Game::chance_outcomes`], never by UCT.
///
/// Rollouts are uniform-random. Randomness (rollouts and chance sampling) comes
/// from an internal seeded generator, so a run is reproducible.
///
/// [`Minimax`]: crate::Minimax
pub struct Mcts {
    iterations: u32,
    exploration: f64,
    rng: Prng,
}

impl Mcts {
    /// Creates a search running `iterations` simulations per move, seeded from
    /// `seed`. Uses the standard UCT exploration constant (sqrt 2).
    #[must_use]
    pub const fn new(iterations: u32, seed: u64) -> Self {
        Self {
            iterations,
            exploration: SQRT_2,
            rng: Prng::new(seed),
        }
    }

    /// Sets the UCT exploration constant (higher explores more).
    #[must_use]
    pub const fn with_exploration(mut self, exploration: f64) -> Self {
        self.exploration = exploration;
        self
    }

    /// Estimates the value of `state` for `player` as the mean rollout reward,
    /// searching from `player`'s perspective. Works even when the root is a
    /// chance node (no decision to make), unlike [`Bot::choose`].
    pub fn evaluate<G>(&mut self, game: &G, state: &G::State, player: PlayerId) -> f64
    where
        G: Game,
        G::State: Clone,
        G::Action: Clone,
    {
        let tree = self.run(game, state, player);
        tree[0].mean()
    }

    fn run<G>(&mut self, game: &G, root_state: &G::State, root: PlayerId) -> Vec<Node<G::Action>>
    where
        G: Game,
        G::State: Clone,
        G::Action: Clone,
    {
        let mut nodes = vec![make_node(game, root_state)];
        for _ in 0..self.iterations {
            let mut state = root_state.clone();
            let mut path = vec![0usize];
            let mut current = 0usize;

            loop {
                if nodes[current].terminal {
                    break;
                }
                if nodes[current].chance {
                    let Some(action) = sample_chance(game, &state, &mut self.rng) else {
                        break;
                    };
                    game.apply(&mut state, PlayerId::CHANCE, action.clone());
                    current = child_for(&mut nodes, current, &action, game, &state);
                    path.push(current);
                    continue;
                }
                if let Some(action) = nodes[current].untried.pop() {
                    let mover = nodes[current].to_move;
                    game.apply(&mut state, mover, action.clone());
                    nodes.push(make_node(game, &state));
                    let child = nodes.len() - 1;
                    nodes[current].children.push((action, child));
                    path.push(child);
                    break;
                }
                let mover = nodes[current].to_move;
                let (action, child) = self.select(&nodes, current, mover == root);
                game.apply(&mut state, mover, action);
                current = child;
                path.push(current);
            }

            let value = self.rollout(game, state, root);
            for &id in &path {
                nodes[id].visits += 1;
                nodes[id].value += value;
            }
        }
        nodes
    }

    /// UCT: pick the child maximizing exploitation + exploration. Exploitation
    /// is the child's mean from the mover's perspective, so it is negated when
    /// the mover is the opponent (who minimizes the root's value).
    fn select<A: Clone>(&self, nodes: &[Node<A>], node: usize, maximizing: bool) -> (A, usize) {
        let parent_visits = f64::from(nodes[node].visits);
        let sign = if maximizing { 1.0 } else { -1.0 };
        let mut best = None;
        let mut best_score = f64::NEG_INFINITY;
        for (action, id) in &nodes[node].children {
            let child = &nodes[*id];
            let exploit = sign * child.mean();
            let explore = self.exploration * (parent_visits.ln() / f64::from(child.visits)).sqrt();
            let score = exploit + explore;
            if score > best_score {
                best_score = score;
                best = Some((action.clone(), *id));
            }
        }
        best.expect("a fully expanded node has children")
    }

    fn rollout<G>(&mut self, game: &G, mut state: G::State, root: PlayerId) -> f64
    where
        G: Game,
    {
        while !game.is_terminal(&state) {
            let Some(actor) = game.active_players(&state).iter().next() else {
                break;
            };
            if actor.is_chance() {
                let Some(action) = sample_chance(game, &state, &mut self.rng) else {
                    break;
                };
                game.apply(&mut state, PlayerId::CHANCE, action);
            } else {
                let mut actions = game.legal_actions(&state, actor);
                if actions.is_empty() {
                    break;
                }
                // Index is strictly below len, so the cast cannot truncate.
                #[allow(clippy::cast_possible_truncation)]
                let index = self.rng.below(actions.len() as u64) as usize;
                game.apply(&mut state, actor, actions.swap_remove(index));
            }
        }
        game.reward(&state, root)
    }
}

/// One search-tree node. State is not stored; it is replayed from the root each
/// iteration by cloning and applying actions along the path.
struct Node<A> {
    to_move: PlayerId,
    chance: bool,
    terminal: bool,
    visits: u32,
    value: f64,
    untried: Vec<A>,
    children: Vec<(A, usize)>,
}

impl<A> Node<A> {
    fn mean(&self) -> f64 {
        if self.visits == 0 {
            0.0
        } else {
            self.value / f64::from(self.visits)
        }
    }
}

fn make_node<G>(game: &G, state: &G::State) -> Node<G::Action>
where
    G: Game,
{
    let mut node = Node {
        to_move: PlayerId::CHANCE,
        chance: false,
        terminal: false,
        visits: 0,
        value: 0.0,
        untried: Vec::new(),
        children: Vec::new(),
    };
    let actor = game.active_players(state).iter().next();
    match actor {
        None => node.terminal = true,
        Some(player) if player.is_chance() => {
            node.to_move = PlayerId::CHANCE;
            node.chance = true;
        }
        Some(player) => {
            node.to_move = player;
            node.untried = game.legal_actions(state, player);
        }
    }
    node
}

/// Finds or creates the chance child reached by `action`.
fn child_for<G>(
    nodes: &mut Vec<Node<G::Action>>,
    parent: usize,
    action: &G::Action,
    game: &G,
    state: &G::State,
) -> usize
where
    G: Game,
    G::Action: Clone,
{
    if let Some((_, id)) = nodes[parent].children.iter().find(|(a, _)| a == action) {
        return *id;
    }
    nodes.push(make_node(game, state));
    let id = nodes.len() - 1;
    nodes[parent].children.push((action.clone(), id));
    id
}

impl<G> Bot<G> for Mcts
where
    G: Game,
    G::State: Clone,
    G::Action: Clone,
{
    fn choose(&mut self, game: &G, state: &G::State, player: PlayerId) -> Option<G::Action> {
        if game.legal_actions(state, player).is_empty() {
            return None;
        }
        let tree = self.run(game, state, player);
        tree[0]
            .children
            .iter()
            .max_by_key(|(_, id)| tree[*id].visits)
            .map(|(action, _)| action.clone())
    }
}

impl<G> RankedBot<G> for Mcts
where
    G: Game,
    G::State: Clone,
    G::Action: Clone,
{
    /// Ranks root actions by visit share (the MCTS-recommended policy). Scores
    /// are the fraction of simulations spent on each move and sum to 1.
    fn rank(&mut self, game: &G, state: &G::State, player: PlayerId) -> Vec<(G::Action, f64)> {
        let tree = self.run(game, state, player);
        let total = f64::from(tree[0].visits.max(1));
        let mut ranked: Vec<(G::Action, f64)> = tree[0]
            .children
            .iter()
            .map(|(action, id)| (action.clone(), f64::from(tree[*id].visits) / total))
            .collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
        ranked
    }
}

#[cfg(test)]
mod tests {
    use super::Mcts;
    use crate::{Bot, RandomBot, RankedBot};
    use examples::{HighCard, Move, TicTacToe};
    use turnbase::{Game, PlayerId};

    const P0: PlayerId = PlayerId::new(0);

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
            .unwrap();
            game.apply(&mut state, player, action);
        }
        state
    }

    #[test]
    fn mcts_does_not_lose_to_random() {
        let game = TicTacToe;
        for seed in 0..6 {
            let mut x = Mcts::new(2000, seed);
            let mut o = RandomBot::new(seed + 100);
            let end = run_match(&mut x, &mut o);
            assert!(
                game.reward(&end, P0) >= 0.0,
                "MCTS X lost to random O (seed {seed})"
            );
        }
    }

    #[test]
    fn mcts_takes_an_immediate_win() {
        // X at 0,1 with 2 open -> winning move; O at 3,4.
        let game = TicTacToe;
        let mut state = game.new_initial_state(0);
        for (seat, cell) in [(0u32, 0u8), (1, 3), (0, 1), (1, 4)] {
            game.apply(&mut state, PlayerId::new(seat), Move(cell));
        }
        let mut mcts = Mcts::new(3000, 1);
        assert_eq!(mcts.choose(&game, &state, P0), Some(Move(2)));
    }

    #[test]
    fn rank_is_a_probability_distribution() {
        let game = TicTacToe;
        let state = game.new_initial_state(0);
        let mut mcts = Mcts::new(1500, 7);
        let ranked = mcts.rank(&game, &state, P0);
        assert_eq!(ranked.len(), game.legal_actions(&state, P0).len());
        let sum: f64 = ranked.iter().map(|(_, p)| p).sum();
        assert!((sum - 1.0).abs() < 1e-9, "visit shares sum to 1");
        assert!(
            ranked.windows(2).all(|w| w[0].1 >= w[1].1),
            "sorted best-first"
        );
    }

    #[test]
    fn high_card_is_evaluated_as_fair() {
        // Only chance acts; each seat is equally likely to draw higher, so the
        // expected reward is ~0. This exercises descending through chance nodes.
        let game = HighCard::default();
        let state = game.new_initial_state(0);
        let mut mcts = Mcts::new(20_000, 3);
        let value = mcts.evaluate(&game, &state, P0);
        assert!(value.abs() < 0.1, "high card should be ~fair, got {value}");
    }
}
