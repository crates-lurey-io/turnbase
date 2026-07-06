//! Sampling committed chance outcomes.

use crate::{Game, Prng};

/// Samples one outcome from `game.chance_outcomes(state)`, weighted by
/// probability, using `rng`. Returns `None` if there are no outcomes.
///
/// `rng` is the chance sampler's generator, owned by the driver or search
/// loop, deliberately separate from any per-state generator a game uses for
/// implicit rolls inside `apply`. Given the same generator position and the
/// same outcome list, the draw is reproducible, which is what makes replay and
/// undo/redo of a dealt card land on the same result.
///
/// The returned action is one of the outcomes; the caller applies it with
/// [`PlayerId::CHANCE`](crate::PlayerId::CHANCE) to commit it to state. Use it
/// in a driver or rollout loop whenever the chance pseudo-player is active.
///
/// # Example
/// ```
/// use turnbase::{ActivePlayers, Game, PlayerId, Prng, sample_chance};
///
/// // One chance node that reveals a card 0, 1, or 2 (uniform by default).
/// struct Reveal;
/// impl Game for Reveal {
///     type State = ();
///     type Action = u8;
///     type View = ();
///     fn new_initial_state(&self, _seed: u64) {}
///     fn num_players(&self) -> usize { 0 }
///     fn active_players(&self, _s: &()) -> ActivePlayers { ActivePlayers::one(PlayerId::CHANCE) }
///     fn legal_actions(&self, _s: &(), p: PlayerId) -> Vec<u8> {
///         if p.is_chance() { vec![0, 1, 2] } else { vec![] }
///     }
///     fn apply(&self, _s: &mut (), _p: PlayerId, _a: u8) {}
///     fn is_terminal(&self, _s: &()) -> bool { false }
///     fn reward(&self, _s: &(), _p: PlayerId) -> f64 { 0.0 }
///     fn view(&self, _s: &(), _v: Option<PlayerId>) {}
/// }
///
/// let mut sampler = Prng::new(42);
/// let mut state = ();
/// if Reveal.active_players(&state).contains(PlayerId::CHANCE) {
///     let card = sample_chance(&Reveal, &state, &mut sampler).unwrap();
///     Reveal.apply(&mut state, PlayerId::CHANCE, card); // commit the draw
///     assert!(card <= 2);
/// }
/// ```
pub fn sample_chance<G: Game>(game: &G, state: &G::State, rng: &mut Prng) -> Option<G::Action> {
    let outcomes = game.chance_outcomes(state);
    if outcomes.is_empty() {
        return None;
    }
    let total: f64 = outcomes.iter().map(|(_, weight)| *weight).sum();

    // A uniform point in [0, total). next_u64 / 2^64 lands in [0, 1); the loss
    // of the low bits when widening to f64 is irrelevant for outcome selection.
    #[allow(clippy::cast_precision_loss)]
    let unit = rng.next_u64() as f64 / (u64::MAX as f64 + 1.0);
    let mut remaining = unit * total;

    let mut chosen = None;
    for (action, weight) in outcomes {
        chosen = Some(action);
        remaining -= weight;
        if remaining < 0.0 {
            break;
        }
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::sample_chance;
    use crate::{ActivePlayers, Game, PlayerId, Prng};

    /// A single chance node offering three weighted outcomes.
    struct Weighted;

    impl Game for Weighted {
        type State = ();
        type Action = u8;
        type View = ();

        fn new_initial_state(&self, _seed: u64) -> Self::State {}
        fn num_players(&self) -> usize {
            0
        }
        fn active_players(&self, _state: &Self::State) -> ActivePlayers {
            ActivePlayers::one(PlayerId::CHANCE)
        }
        fn legal_actions(&self, _state: &Self::State, player: PlayerId) -> Vec<Self::Action> {
            if player.is_chance() {
                vec![0, 1, 2]
            } else {
                Vec::new()
            }
        }
        fn chance_outcomes(&self, _state: &Self::State) -> Vec<(Self::Action, f64)> {
            vec![(0, 0.1), (1, 0.3), (2, 0.6)]
        }
        fn apply(&self, _state: &mut Self::State, _player: PlayerId, _action: Self::Action) {}
        fn is_terminal(&self, _state: &Self::State) -> bool {
            false
        }
        fn reward(&self, _state: &Self::State, _player: PlayerId) -> f64 {
            0.0
        }
        fn view(&self, _state: &Self::State, _viewer: Option<PlayerId>) -> Self::View {}
    }

    #[test]
    fn only_returns_offered_outcomes() {
        let mut rng = Prng::new(1);
        for _ in 0..1000 {
            let outcome = sample_chance(&Weighted, &(), &mut rng).unwrap();
            assert!(outcome <= 2);
        }
    }

    #[test]
    fn empty_distribution_yields_none() {
        struct NoChance;
        impl Game for NoChance {
            type State = ();
            type Action = u8;
            type View = ();
            fn new_initial_state(&self, _seed: u64) -> Self::State {}
            fn num_players(&self) -> usize {
                0
            }
            fn active_players(&self, _s: &Self::State) -> ActivePlayers {
                ActivePlayers::none()
            }
            fn legal_actions(&self, _s: &Self::State, _p: PlayerId) -> Vec<Self::Action> {
                Vec::new()
            }
            fn apply(&self, _s: &mut Self::State, _p: PlayerId, _a: Self::Action) {}
            fn is_terminal(&self, _s: &Self::State) -> bool {
                true
            }
            fn reward(&self, _s: &Self::State, _p: PlayerId) -> f64 {
                0.0
            }
            fn view(&self, _s: &Self::State, _v: Option<PlayerId>) -> Self::View {}
        }
        let mut rng = Prng::new(1);
        assert!(sample_chance(&NoChance, &(), &mut rng).is_none());
    }

    #[test]
    fn empirical_frequencies_track_the_weights() {
        let mut rng = Prng::new(12345);
        let mut counts = [0u32; 3];
        let trials = 100_000;
        for _ in 0..trials {
            let outcome = sample_chance(&Weighted, &(), &mut rng).unwrap();
            counts[outcome as usize] += 1;
        }
        let freq = counts.map(|c| f64::from(c) / f64::from(trials));
        // Expected 0.1 / 0.3 / 0.6; generous tolerance for sampling noise.
        assert!((freq[0] - 0.1).abs() < 0.01, "freq {freq:?}");
        assert!((freq[1] - 0.3).abs() < 0.01, "freq {freq:?}");
        assert!((freq[2] - 0.6).abs() < 0.01, "freq {freq:?}");
    }

    #[test]
    fn same_seed_reproduces_the_draw() {
        let mut a = Prng::new(99);
        let mut b = Prng::new(99);
        for _ in 0..100 {
            assert_eq!(
                sample_chance(&Weighted, &(), &mut a),
                sample_chance(&Weighted, &(), &mut b)
            );
        }
    }
}
