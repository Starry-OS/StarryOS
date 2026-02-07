use alloc::vec::Vec;
use core::mem::replace;

/// A simple LRU Cache implementation based on a fixed-size array.
///
/// It maintains a fixed-capacity storage and uses an intrusive doubly-linked list
/// structure using array indices to track the usage order (from MRU to LRU).
#[derive(Debug, Clone)]
pub struct LruCache<V, const CAP: usize> {
    storage: Vec<CacheNode<V>>,
    mru_idx: u16,
    lru_idx: u16,
}

/// Internal node for the LRU cache linked list.
#[derive(Debug, Clone)]
struct CacheNode<V> {
    payload: V,
    prev: u16,
    next: u16,
}

impl<V, const CAP: usize> Default for LruCache<V, CAP> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V, const CAP: usize> LruCache<V, CAP> {
    /// Creates a new empty LRU cache.
    pub const fn new() -> Self {
        Self {
            storage: Vec::new(),
            mru_idx: 0,
            lru_idx: 0,
        }
    }

    /// Inserts a value into the cache.
    ///
    /// The inserted value becomes the most-recently-used (MRU) item.
    /// If the cache is full, the least-recently-used (LRU) item is removed and returned.
    pub fn put(&mut self, val: V) -> Option<V> {
        let node = CacheNode {
            payload: val,
            prev: 0,
            next: 0,
        };

        if self.storage.len() >= CAP {
            let idx = self.pop_lru();
            let old = replace(&mut self.storage[idx as usize], node);
            self.push_mru(idx);
            Some(old.payload)
        } else {
            let idx = self.storage.len() as u16;
            self.storage.push(node);
            self.push_mru(idx);
            None
        }
    }

    /// Accesses an item in the cache that matches the predicate.
    ///
    /// If an item is found, it is promoted to the most-recently-used (MRU) position,
    /// and the function returns `true`. Otherwise, it returns `false`.
    pub fn access<F>(&mut self, mut pred: F) -> bool
    where
        F: FnMut(&V) -> bool,
    {
        for i in 0..self.storage.len() {
            if pred(&self.storage[i].payload) {
                self.promote(i as u16);
                return true;
            }
        }
        false
    }

    /// Returns a reference to the most-recently-used (MRU) item.
    ///
    /// This does not change the cache state.
    pub fn peek_mru(&self) -> Option<&V> {
        self.storage.get(self.mru_idx as usize).map(|n| &n.payload)
    }

    /// Returns an iterator over the cache items, ordered from MRU to LRU.
    pub fn items(&self) -> LruIter<'_, V, CAP> {
        LruIter::<V, CAP> {
            cache: self,
            pos: self.mru_idx,
        }
    }

    /// Clears all items from the cache.
    pub fn flush(&mut self) {
        self.storage.clear();
        self.mru_idx = 0;
        self.lru_idx = 0;
    }

    fn promote(&mut self, idx: u16) {
        if idx != self.mru_idx {
            self.unlink(idx);
            self.push_mru(idx);
        }
    }

    fn unlink(&mut self, idx: u16) {
        let prev = self.storage[idx as usize].prev;
        let next = self.storage[idx as usize].next;

        if idx == self.mru_idx {
            self.mru_idx = next;
        } else {
            self.storage[prev as usize].next = next;
        }

        if idx == self.lru_idx {
            self.lru_idx = prev;
        } else {
            self.storage[next as usize].prev = prev;
        }
    }

    fn push_mru(&mut self, idx: u16) {
        if self.storage.len() == 1 {
            self.lru_idx = idx;
        } else {
            self.storage[idx as usize].next = self.mru_idx;
            self.storage[self.mru_idx as usize].prev = idx;
        }
        self.mru_idx = idx;
    }

    fn pop_lru(&mut self) -> u16 {
        let idx = self.lru_idx;

        if self.storage.len() <= 1 {
            // Single-element cache: after popping, there is no valid MRU/LRU.
            // Reset indices; the caller is expected to reuse `idx` as needed.
            self.mru_idx = 0;
            self.lru_idx = 0;
        } else {
            let prev = self.storage[idx as usize].prev;
            // The previous node becomes the new LRU (tail).
            self.lru_idx = prev;
            // Fully unlink the old LRU by clearing the new tail's `next`
            // pointer so it no longer points at the removed node.
            self.storage[prev as usize].next = prev;
        }
        idx
    }
}

/// Iterator over the `LruCache` items, from MRU to LRU.
pub struct LruIter<'a, V, const CAP: usize> {
    cache: &'a LruCache<V, CAP>,
    pos: u16,
}

impl<'a, V, const CAP: usize> Iterator for LruIter<'a, V, CAP> {
    type Item = &'a V;

    fn next(&mut self) -> Option<&'a V> {
        // Ensure CAP fits in u16, as indices are u16.
        const _: () = assert!(CAP <= u16::MAX as usize, "CAP must fit in u16");

        if self.cache.storage.is_empty() {
            return None;
        }
        if self.pos as usize >= self.cache.storage.len() {
            return None;
        }

        let node = &self.cache.storage[self.pos as usize];
        let val = &node.payload;

        let current_pos = self.pos;
        if current_pos == self.cache.lru_idx {
            self.pos = CAP as u16;
        } else {
            self.pos = node.next;
        }
        Some(val)
    }
}
