//! An ordered pile of items: a deck, hand, discard, or any zone of cards.

use crate::Prng;

/// An ordered pile of items with deterministic operations.
///
/// The "top" is the end of the pile: [`draw`](Pile::draw) takes from the top
/// and [`put`](Pile::put) adds to it. Shuffling is deterministic given a
/// [`Prng`], and the pile carries no generator of its own, so it snapshots and
/// replays with whatever state it lives in. This is an opt-in helper; a game's
/// `State` can hold piles or plain fields as it prefers.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Pile<T> {
    items: Vec<T>,
}

impl<T> Pile<T> {
    /// Creates an empty pile.
    #[must_use]
    pub const fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Creates a pile from `items`, with the last element on top.
    #[must_use]
    pub const fn from_items(items: Vec<T>) -> Self {
        Self { items }
    }

    /// Returns the number of items.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns true if the pile is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Removes and returns the top item, or `None` if empty.
    pub fn draw(&mut self) -> Option<T> {
        self.items.pop()
    }

    /// Removes up to `n` items from the top, returned top-first.
    pub fn draw_n(&mut self, n: usize) -> Vec<T> {
        let mut drawn = Vec::new();
        for _ in 0..n {
            match self.items.pop() {
                Some(item) => drawn.push(item),
                None => break,
            }
        }
        drawn
    }

    /// Adds an item to the top.
    pub fn put(&mut self, item: T) {
        self.items.push(item);
    }

    /// Adds an item to the bottom.
    pub fn put_bottom(&mut self, item: T) {
        self.items.insert(0, item);
    }

    /// Inserts an item at `index` (clamped to the length), preserving order.
    ///
    /// Use this to reverse a draw exactly, e.g. in a `Reversible` undo.
    pub fn insert(&mut self, index: usize, item: T) {
        self.items.insert(index.min(self.items.len()), item);
    }

    /// Removes and returns the item at `index`, or `None` if out of range.
    pub fn remove(&mut self, index: usize) -> Option<T> {
        if index < self.items.len() {
            Some(self.items.remove(index))
        } else {
            None
        }
    }

    /// Shuffles the pile in place with a deterministic Fisher-Yates using `rng`.
    pub fn shuffle(&mut self, rng: &mut Prng) {
        rng.shuffle(&mut self.items);
    }

    /// Returns the items, bottom to top.
    #[must_use]
    pub fn as_slice(&self) -> &[T] {
        &self.items
    }

    /// Iterates the items, bottom to top.
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.items.iter()
    }
}

impl<T: PartialEq> Pile<T> {
    /// Returns true if the pile contains `item`.
    #[must_use]
    pub fn contains(&self, item: &T) -> bool {
        self.items.contains(item)
    }

    /// Returns the index of the first item equal to `item`, or `None`.
    #[must_use]
    pub fn position(&self, item: &T) -> Option<usize> {
        self.items.iter().position(|candidate| candidate == item)
    }

    /// Removes and returns the first item equal to `item`, or `None`.
    pub fn remove_item(&mut self, item: &T) -> Option<T> {
        self.position(item).and_then(|index| self.remove(index))
    }
}

impl<T> FromIterator<T> for Pile<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self {
            items: iter.into_iter().collect(),
        }
    }
}

impl<'a, T> IntoIterator for &'a Pile<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::Pile;
    use crate::Prng;

    #[test]
    fn draw_takes_from_the_top() {
        let mut pile = Pile::from_items(vec![1, 2, 3]);
        assert_eq!(pile.draw(), Some(3));
        assert_eq!(pile.draw(), Some(2));
        assert_eq!(pile.len(), 1);
    }

    #[test]
    fn draw_n_stops_at_empty() {
        let mut pile = Pile::from_items(vec![1, 2]);
        assert_eq!(pile.draw_n(5), vec![2, 1]);
        assert!(pile.is_empty());
    }

    #[test]
    fn remove_then_insert_restores_order() {
        let mut pile = Pile::from_items(vec!['a', 'b', 'c', 'd']);
        let removed = pile.remove(1).unwrap();
        assert_eq!(removed, 'b');
        pile.insert(1, removed);
        assert_eq!(pile.as_slice(), &['a', 'b', 'c', 'd']);
    }

    #[test]
    fn shuffle_is_a_deterministic_permutation() {
        let base: Vec<u32> = (0..50).collect();
        let mut a = Pile::from_items(base.clone());
        let mut b = Pile::from_items(base.clone());
        a.shuffle(&mut Prng::new(7));
        b.shuffle(&mut Prng::new(7));
        assert_eq!(a, b, "same seed shuffles identically");

        let mut sorted: Vec<u32> = a.iter().copied().collect();
        sorted.sort_unstable();
        assert_eq!(sorted, base, "shuffle is a permutation");
    }

    #[test]
    fn contains_and_remove_item_use_equality() {
        let mut pile = Pile::from_items(vec![10, 20, 30]);
        assert!(pile.contains(&20));
        assert_eq!(pile.position(&30), Some(2));
        assert_eq!(pile.remove_item(&20), Some(20));
        assert!(!pile.contains(&20));
    }
}
