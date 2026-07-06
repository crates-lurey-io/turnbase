//! Tier-2 triggered effects: an ordered effect queue.
//!
//! A game whose moves resolve as a queue of effects, where applying one effect
//! can trigger more, implements [`EffectSystem`] and drives resolution with
//! [`resolve_effects`]. Effects are always appended and resolved in FIFO order,
//! never invoked inline. That is the Tier-2 boundary from `ARCHITECTURE.md`
//! (queued enum effects, deterministic order, no player responses); a future
//! Tier-3 (priority stack, state-based-action rechecking, response windows)
//! slots around this loop rather than replacing it.

use std::collections::VecDeque;

/// A game that resolves effects through a queue.
///
/// The move logic ([`Game::apply`](crate::Game::apply)) builds the initial
/// effects of a move and calls [`resolve_effects`]; the queue does the rest.
pub trait EffectSystem {
    /// The state effects mutate.
    type State;
    /// One atomic effect (a game-defined enum).
    type Effect;

    /// Applies one effect's direct consequence to state.
    fn apply(&self, state: &mut Self::State, effect: &Self::Effect);

    /// After `effect` is applied, performs state-based actions (deaths,
    /// removals) and returns the follow-up effects they trigger, in
    /// deterministic order. Returned effects are appended to the queue and
    /// resolved later, never inline.
    fn react(&self, state: &mut Self::State, effect: &Self::Effect) -> Vec<Self::Effect>;
}

/// Upper bound on effects resolved by one [`resolve_effects`] call, a guard
/// against a game whose triggers never terminate. A well-formed game resolves
/// in far fewer steps.
pub const MAX_EFFECTS: usize = 100_000;

/// Resolves `initial` and everything it triggers, applying each effect then
/// enqueuing its follow-ups, in FIFO order.
///
/// Returns the number of effects resolved. Stops at [`MAX_EFFECTS`] to bound a
/// non-terminating trigger loop (a game bug).
pub fn resolve_effects<S: EffectSystem>(
    system: &S,
    state: &mut S::State,
    initial: impl IntoIterator<Item = S::Effect>,
) -> usize {
    let mut queue: VecDeque<S::Effect> = initial.into_iter().collect();
    let mut resolved = 0;
    while let Some(effect) = queue.pop_front() {
        system.apply(state, &effect);
        for follow_up in system.react(state, &effect) {
            queue.push_back(follow_up);
        }
        resolved += 1;
        if resolved >= MAX_EFFECTS {
            break;
        }
    }
    resolved
}

#[cfg(test)]
mod tests {
    use super::{EffectSystem, MAX_EFFECTS, resolve_effects};

    /// Knocking domino `i` knocks `i + 1`: a one-at-a-time cascade.
    struct Dominoes;

    impl EffectSystem for Dominoes {
        type State = Vec<bool>;
        type Effect = usize;

        fn apply(&self, state: &mut Self::State, effect: &Self::Effect) {
            state[*effect] = true;
        }

        fn react(&self, state: &mut Self::State, effect: &Self::Effect) -> Vec<Self::Effect> {
            let next = *effect + 1;
            if next < state.len() && !state[next] {
                vec![next]
            } else {
                Vec::new()
            }
        }
    }

    #[test]
    fn cascade_resolves_in_order() {
        let mut state = vec![false; 5];
        let steps = resolve_effects(&Dominoes, &mut state, [0]);
        assert!(state.iter().all(|&knocked| knocked));
        assert_eq!(steps, 5);
    }

    #[test]
    fn nonterminating_triggers_are_capped() {
        struct Forever;
        impl EffectSystem for Forever {
            type State = u32;
            type Effect = ();
            fn apply(&self, state: &mut u32, _effect: &()) {
                *state += 1;
            }
            fn react(&self, _state: &mut u32, _effect: &()) -> Vec<()> {
                vec![()]
            }
        }
        let mut count = 0;
        let steps = resolve_effects(&Forever, &mut count, [()]);
        assert_eq!(steps, MAX_EFFECTS);
    }
}
