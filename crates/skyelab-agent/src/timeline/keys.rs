//! A thread's timeline rows, each with a key of its own. The list is spliced (an interrupted note goes in above
//! later rows), so a row's index can change under it; element ids and per-row state (markdown cache, selection, an
//! open fold) are keyed by the row key instead, and stay with their row.

use super::Item;
use std::sync::atomic::{AtomicU64, Ordering};

/// Never reused, even by a list built again from the store, so old element state can't attach to a new row.
static NEXT: AtomicU64 = AtomicU64::new(1);

/// The rows as a slice (`Deref`), so reading and editing a row in place work as on a `Vec`; only the calls that add a
/// row are here, and each gives the new row a key.
#[derive(Debug, Clone, Default)]
pub struct Items {
    items: Vec<Item>,
    keys: Vec<u64>,
}

impl Items {
    pub fn push(&mut self, item: Item) {
        self.items.push(item);
        self.keys.push(NEXT.fetch_add(1, Ordering::Relaxed));
    }

    pub fn insert(&mut self, at: usize, item: Item) {
        self.items.insert(at, item);
        self.keys.insert(at, NEXT.fetch_add(1, Ordering::Relaxed));
    }

    /// Removes rows `range`, with their keys.
    pub fn remove(&mut self, range: std::ops::Range<usize>) {
        self.items.drain(range.clone());
        self.keys.drain(range);
    }

    /// Row `ix`'s key; 0 (no row has it) past the end.
    pub fn key(&self, ix: usize) -> u64 {
        self.keys.get(ix).copied().unwrap_or(0)
    }

    /// The row whose key is `key`, where it is now.
    pub fn position(&self, key: u64) -> Option<usize> {
        self.keys.iter().rposition(|k| *k == key)
    }
}

impl std::ops::Deref for Items {
    type Target = [Item];
    fn deref(&self) -> &[Item] {
        &self.items
    }
}

impl std::ops::DerefMut for Items {
    fn deref_mut(&mut self) -> &mut [Item] {
        &mut self.items
    }
}

// Tests build lists directly.
impl PartialEq<Vec<Item>> for Items {
    fn eq(&self, other: &Vec<Item>) -> bool {
        self.items == *other
    }
}

// Tests build lists directly.
impl FromIterator<Item> for Items {
    fn from_iter<I: IntoIterator<Item = Item>>(iter: I) -> Self {
        let mut items = Items::default();
        iter.into_iter().for_each(|i| items.push(i));
        items
    }
}

impl<'a> IntoIterator for &'a Items {
    type Item = &'a Item;
    type IntoIter = std::slice::Iter<'a, Item>;
    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

// Tests build lists directly.
impl From<Vec<Item>> for Items {
    fn from(items: Vec<Item>) -> Self {
        items.into_iter().collect()
    }
}
