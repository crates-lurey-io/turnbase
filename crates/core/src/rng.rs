//! Deterministic, snapshot-able pseudo-random generator.

/// A small, deterministic pseudo-random generator that lives inside game state.
///
/// The whole reproducible position is a single `u64` ([`Prng::position`]), so
/// the generator is `Copy`, serializes with the state it lives in (O(1)
/// snapshot and resume), and a `Reversible` game's undo record can restore its
/// position exactly. Everything is integer-only with a fixed algorithm, so a
/// seed reproduces the same stream on every platform. Do not rely on the
/// concrete algorithm (currently PCG XSH-RR 64/32, single stream): it is an
/// implementation detail of this newtype and may change.
///
/// This is not cryptographically secure and must not be used for security.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Prng {
    state: u64,
}

const MULT: u64 = 6_364_136_223_846_793_005;
const INC: u64 = 1_442_695_040_888_963_407;

impl Prng {
    /// Creates a generator seeded from `seed`.
    ///
    /// Equal seeds produce identical streams; different seeds are extremely
    /// likely to diverge immediately.
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        let mut rng = Self {
            state: INC.wrapping_add(seed),
        };
        rng.step();
        rng
    }

    /// Returns the generator's current position in its stream.
    ///
    /// Capture this before a move and pass it to [`Prng::set_position`] to
    /// rewind, which is how a make/unmake (`Reversible`) undo record restores
    /// the random stream.
    #[must_use]
    pub const fn position(&self) -> u64 {
        self.state
    }

    /// Restores a position previously read from [`Prng::position`].
    pub const fn set_position(&mut self, position: u64) {
        self.state = position;
    }

    const fn step(&mut self) {
        self.state = self.state.wrapping_mul(MULT).wrapping_add(INC);
    }

    /// Returns the next 32-bit value and advances the stream by one step.
    // The two casts are the defining truncations of PCG XSH-RR: the output word
    // is the low 32 bits of the xorshift, and the rotation is the top 5 bits.
    #[allow(clippy::cast_possible_truncation)]
    pub const fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.step();
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// Returns the next 64-bit value (two steps of the stream).
    pub const fn next_u64(&mut self) -> u64 {
        let hi = self.next_u32() as u64;
        let lo = self.next_u32() as u64;
        (hi << 32) | lo
    }

    /// Returns a uniformly distributed value in `0..bound`.
    ///
    /// Uses rejection sampling to avoid modulo bias, so a single call consumes a
    /// variable number of steps. That is why a pre-move [`Prng::position`] must
    /// be snapshotted rather than reconstructed by counting draws.
    ///
    /// # Panics
    /// Panics if `bound` is zero.
    pub const fn below(&mut self, bound: u64) -> u64 {
        assert!(bound != 0, "below(0) has no valid output");
        // Reject the low `2^64 % bound` outputs so the rest divide evenly.
        let threshold = bound.wrapping_neg() % bound;
        loop {
            let value = self.next_u64();
            if value >= threshold {
                return value % bound;
            }
        }
    }

    /// Returns a uniformly distributed value in `low..high`.
    ///
    /// # Panics
    /// Panics if `low >= high`.
    pub const fn range(&mut self, low: u64, high: u64) -> u64 {
        assert!(low < high, "range requires low < high");
        low + self.below(high - low)
    }

    /// Returns a reference to a uniformly chosen element, or `None` if empty.
    // `below` returns a value strictly less than `len`, which is itself a
    // `usize`, so the cast back to `usize` cannot truncate.
    #[allow(clippy::cast_possible_truncation)]
    pub fn choose<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            None
        } else {
            let index = self.below(items.len() as u64) as usize;
            Some(&items[index])
        }
    }

    /// Shuffles `items` in place with an unbiased Fisher-Yates shuffle.
    // `j` is strictly less than `i + 1 <= len`, a `usize`, so the cast cannot
    // truncate.
    #[allow(clippy::cast_possible_truncation)]
    pub const fn shuffle<T>(&mut self, items: &mut [T]) {
        let mut i = items.len();
        while i > 1 {
            i -= 1;
            let j = self.below(i as u64 + 1) as usize;
            items.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Prng;

    #[test]
    fn same_seed_same_stream() {
        let mut a = Prng::new(42);
        let mut b = Prng::new(42);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn different_seeds_diverge() {
        let mut a = Prng::new(1);
        let mut b = Prng::new(2);
        assert_ne!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn position_round_trips_through_variable_draws() {
        let mut rng = Prng::new(7);
        let mark = rng.position();
        let first: Vec<u64> = (0..20).map(|_| rng.below(100)).collect();

        rng.set_position(mark);
        let second: Vec<u64> = (0..20).map(|_| rng.below(100)).collect();

        assert_eq!(first, second, "restoring position replays the same draws");
    }

    #[test]
    fn below_is_in_bounds() {
        let mut rng = Prng::new(9);
        for _ in 0..10_000 {
            assert!(rng.below(6) < 6);
        }
    }

    #[test]
    fn range_is_in_bounds() {
        let mut rng = Prng::new(9);
        for _ in 0..10_000 {
            let v = rng.range(10, 20);
            assert!((10..20).contains(&v));
        }
    }

    #[test]
    fn shuffle_is_a_permutation() {
        let mut rng = Prng::new(3);
        let mut data: Vec<u32> = (0..50).collect();
        rng.shuffle(&mut data);
        let mut sorted = data.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..50).collect::<Vec<_>>());
    }

    #[test]
    fn choose_empty_is_none() {
        let mut rng = Prng::new(1);
        assert!(rng.choose::<u8>(&[]).is_none());
    }
}
