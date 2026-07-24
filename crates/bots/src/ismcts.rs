//! Information-set Monte Carlo tree search (single-observer ISMCTS).

use std::f64::consts::SQRT_2;

use turnbase::{Determinize, Game, PlayerId, Prng, sample_chance};

use crate::{Bot, RankedBot};

/// Single-observer information-set MCTS for hidden-information games.
///
/// Standard [`Mcts`](crate::Mcts) cheats at hidden information: it searches the
/// one true state, so it sees opponents' secret cards. ISMCTS instead searches
/// from a player's *information set*. Each simulation asks the game for a fresh
/// determinization ([`Determinize::determinize`]) — a full world consistent with
/// what the searcher can see, but with the hidden parts resampled — and runs one
/// UCT iteration in that world. Averaging over many sampled worlds yields a move
/// that is good on expectation without ever peeking.
///
/// A single tree is shared across determinizations. Because different worlds
/// offer different legal moves, selection uses an *availability* count (how many
/// simulations a move was legal for) in the UCB denominator, and only moves
/// legal in the current world are considered. Values are backed up as a vector,
/// one entry per seat, and each node selects to maximize the mover's own entry
/// (`max^n`), so this handles three- and four-player games, not just two-player
/// zero-sum. Rollouts are uniform-random. All randomness comes from an internal
/// seeded generator, so a run is reproducible.
pub struct Ismcts {
    iterations: u32,
    exploration: f64,
    rng: Prng,
}

impl Ismcts {
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

    /// Estimates `player`'s value of `state` as the mean rollout reward over
    /// sampled worlds, searching from `player`'s information set.
    pub fn evaluate<G>(&mut self, game: &G, state: &G::State, player: PlayerId) -> f64
    where
        G: Determinize,
        G::State: Clone,
        G::Action: Clone,
    {
        let tree = self.run(game, state, player);
        tree[0].mean(player.index() as usize)
    }

    fn run<G>(
        &mut self,
        game: &G,
        root_state: &G::State,
        observer: PlayerId,
    ) -> Vec<Node<G::Action>>
    where
        G: Determinize,
        G::State: Clone,
        G::Action: Clone,
    {
        let players = game.num_players();
        let mut nodes = vec![Node::new(players)];
        for _ in 0..self.iterations {
            let mut world = game.determinize(root_state, observer, &mut self.rng);
            let mut path = vec![0usize];
            // Nodes whose availability counts to bump: (node, its legal children).
            let mut available: Vec<Vec<usize>> = Vec::new();
            let mut current = 0usize;

            loop {
                if game.is_terminal(&world) {
                    break;
                }
                let Some(actor) = game.active_players(&world).iter().next() else {
                    break;
                };
                if actor.is_chance() {
                    let Some(action) = sample_chance(game, &world, &mut self.rng) else {
                        break;
                    };
                    game.apply(&mut world, PlayerId::CHANCE, action.clone());
                    current = child_for(&mut nodes, current, &action, players);
                    path.push(current);
                    continue;
                }

                let legal = game.legal_actions(&world, actor);
                if legal.is_empty() {
                    break;
                }
                let untried: Vec<G::Action> = legal
                    .iter()
                    .filter(|a| !nodes[current].children.iter().any(|(c, _)| c == *a))
                    .cloned()
                    .collect();

                if !untried.is_empty() {
                    // Index is strictly below len, so the cast cannot truncate.
                    #[allow(clippy::cast_possible_truncation)]
                    let pick = self.rng.below(untried.len() as u64) as usize;
                    let action = untried[pick].clone();
                    game.apply(&mut world, actor, action.clone());
                    nodes.push(Node::new(players));
                    let child = nodes.len() - 1;
                    nodes[current].children.push((action, child));
                    available.push(legal_children(&nodes[current], &legal));
                    path.push(child);
                    break;
                }

                let seat = actor.index() as usize;
                let legal_ids = legal_children(&nodes[current], &legal);
                let (action, child) = self.select(&nodes, current, &legal_ids, seat);
                available.push(legal_ids);
                game.apply(&mut world, actor, action);
                current = child;
                path.push(current);
            }

            let rewards = self.rollout(game, world, players);
            for &id in &path {
                nodes[id].visits += 1;
                for (seat, reward) in rewards.iter().enumerate() {
                    nodes[id].value[seat] += reward;
                }
            }
            for ids in &available {
                for &id in ids {
                    nodes[id].avails += 1;
                }
            }
        }
        nodes
    }

    /// UCB1 over the currently-legal children, from `seat`'s perspective. The
    /// exploration term divides by the child's availability count, not the
    /// parent's visits, because a move may be legal in only some worlds.
    fn select<A: Clone>(
        &self,
        nodes: &[Node<A>],
        parent: usize,
        legal_ids: &[usize],
        seat: usize,
    ) -> (A, usize) {
        let mut best = None;
        let mut best_score = f64::NEG_INFINITY;
        for &id in legal_ids {
            let child = &nodes[id];
            let exploit = child.mean(seat);
            let explore = self.exploration
                * (f64::from(child.avails.max(1)).ln() / f64::from(child.visits.max(1))).sqrt();
            let score = exploit + explore;
            if score > best_score {
                best_score = score;
                best = Some(id);
            }
        }
        let id = best.expect("a fully expanded node has legal children");
        let action = nodes[parent]
            .children
            .iter()
            .find(|(_, cid)| *cid == id)
            .map(|(a, _)| a.clone())
            .expect("selected child belongs to the parent");
        (action, id)
    }

    fn rollout<G>(&mut self, game: &G, mut state: G::State, players: usize) -> Vec<f64>
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
        (0..players)
            .map(|seat| game.reward(&state, PlayerId::new(u32::try_from(seat).unwrap())))
            .collect()
    }
}

/// A shared-tree node. Its state is not stored; each simulation replays from the
/// root in a freshly sampled world. Values are one running sum per seat.
struct Node<A> {
    visits: u32,
    avails: u32,
    value: Vec<f64>,
    children: Vec<(A, usize)>,
}

impl<A> Node<A> {
    fn new(players: usize) -> Self {
        Self {
            visits: 0,
            avails: 0,
            value: vec![0.0; players],
            children: Vec::new(),
        }
    }

    fn mean(&self, seat: usize) -> f64 {
        if self.visits == 0 {
            0.0
        } else {
            self.value[seat] / f64::from(self.visits)
        }
    }
}

/// The child node ids whose action is legal in the current world.
fn legal_children<A: PartialEq>(node: &Node<A>, legal: &[A]) -> Vec<usize> {
    node.children
        .iter()
        .filter(|(action, _)| legal.contains(action))
        .map(|(_, id)| *id)
        .collect()
}

/// Finds or creates the chance child reached by `action`.
fn child_for<A: PartialEq + Clone>(
    nodes: &mut Vec<Node<A>>,
    parent: usize,
    action: &A,
    players: usize,
) -> usize {
    if let Some((_, id)) = nodes[parent].children.iter().find(|(a, _)| a == action) {
        return *id;
    }
    nodes.push(Node::new(players));
    let id = nodes.len() - 1;
    nodes[parent].children.push((action.clone(), id));
    id
}

impl<G> Bot<G> for Ismcts
where
    G: Determinize,
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

impl<G> RankedBot<G> for Ismcts
where
    G: Determinize,
    G::State: Clone,
    G::Action: Clone,
{
    /// Ranks the searcher's moves by visit share (the ISMCTS policy). Scores are
    /// the fraction of simulations spent on each move and sum to 1.
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
    use super::Ismcts;
    use crate::{Bot, RandomBot};
    use coup::{Coup, CoupState};
    use tic_tac_toe::{Move, TicTacToe};
    use turnbase::{Game, PlayerId};

    const P0: PlayerId = PlayerId::new(0);

    /// Drives a two-seat match: seat 0 uses `a`, seat 1 uses `b`.
    fn play<G, A, B>(game: &G, mut state: G::State, a: &mut A, b: &mut B) -> G::State
    where
        G: Game,
        A: Bot<G>,
        B: Bot<G>,
    {
        let mut steps = 0;
        while !game.is_terminal(&state) {
            let player = game.active_players(&state).iter().next().unwrap();
            let action = if player.index() == 0 {
                a.choose(game, &state, player)
            } else {
                b.choose(game, &state, player)
            }
            .expect("an active player has a move");
            game.apply(&mut state, player, action);
            steps += 1;
            assert!(steps < 20_000, "match did not terminate");
        }
        state
    }

    #[test]
    fn ismcts_reduces_to_mcts_on_perfect_information() {
        // With determinize = clone, ISMCTS is ordinary UCT and must not lose to
        // random play at tic-tac-toe.
        let game = TicTacToe;
        for seed in 0..6 {
            let mut x = Ismcts::new(2000, seed);
            let mut o = RandomBot::new(seed + 100);
            let end = play(&game, game.new_initial_state(0), &mut x, &mut o);
            assert!(
                game.reward(&end, P0) >= 0.0,
                "ISMCTS X lost to random O (seed {seed})"
            );
        }
    }

    #[test]
    fn ismcts_takes_an_immediate_win() {
        let game = TicTacToe;
        let mut state = game.new_initial_state(0);
        for (seat, cell) in [(0u32, 0u8), (1, 3), (0, 1), (1, 4)] {
            game.apply(&mut state, PlayerId::new(seat), Move(cell));
        }
        let mut bot = Ismcts::new(3000, 1);
        assert_eq!(bot.choose(&game, &state, P0), Some(Move(2)));
    }

    #[test]
    fn ismcts_beats_random_at_coup() {
        // Hidden-information play with no cheating: ISMCTS searches sampled
        // worlds. Over many two-player matches it should beat a random opponent
        // comfortably. Seat 0 is ISMCTS; alternate who moves first by seed.
        let game = Coup::new(2);
        let mut wins = 0;
        let matches = 30;
        for seed in 0..matches {
            let start = game.new_initial_state(seed);
            let end: CoupState = if seed % 2 == 0 {
                let mut a = Ismcts::new(400, seed);
                let mut b = RandomBot::new(seed ^ 0x55);
                play(&game, start, &mut a, &mut b)
            } else {
                // Odd seeds: still measure seat 0 = ISMCTS, fresh generators.
                let mut a = Ismcts::new(400, seed ^ 0xAA);
                let mut b = RandomBot::new(seed);
                play(&game, start, &mut a, &mut b)
            };
            if game.reward(&end, P0) > 0.0 {
                wins += 1;
            }
        }
        assert!(
            wins * 2 > matches,
            "ISMCTS won only {wins}/{matches} vs random"
        );
    }
}
