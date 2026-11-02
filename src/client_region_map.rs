//! Client-like active-box hash iteration (elementclient 0x41b23b, 0x41da70,
//! 0x413170, 0x41eb7c). This is research infrastructure: block streaming and
//! its asynchronous insertion order have not yet been recovered completely.
use std::collections::HashMap;
const PRIMES: [usize; 12] = [
    53, 97, 193, 389, 769, 1543, 3079, 6151, 12289, 24593, 49157, 98317,
];
#[derive(Debug)]
pub struct ActiveBoxMap {
    buckets: Vec<Vec<u32>>,
    refs: HashMap<u32, u32>,
}
impl Default for ActiveBoxMap {
    fn default() -> Self {
        Self {
            buckets: vec![Vec::new(); 53],
            refs: HashMap::new(),
        }
    }
}
impl ActiveBoxMap {
    /// One reference per loaded block. Repeated references do not change
    /// hash position; the client increments the existing box's refcount.
    pub fn load(&mut self, id: u32) {
        if let Some(r) = self.refs.get_mut(&id) {
            *r += 1;
            return;
        }
        let n = self.refs.len() + 1;
        if n > self.buckets.len() {
            let cap = *PRIMES
                .iter()
                .find(|p| **p > n)
                .expect("active box count exceeds recovered table");
            let mut fresh = vec![Vec::new(); cap];
            // The native rehash consumes old chains from head to tail and
            // prepends to each destination, reversing colliding chains.
            for bucket in &self.buckets {
                for &old in bucket {
                    fresh[old as usize % cap].insert(0, old);
                }
            }
            self.buckets = fresh;
        }
        self.refs.insert(id, 1);
        let bucket = id as usize % self.buckets.len();
        self.buckets[bucket].insert(0, id);
    }
    pub fn unload(&mut self, id: u32) -> bool {
        let Some(count) = self.refs.get_mut(&id) else {
            return false;
        };
        *count -= 1;
        if *count == 0 {
            self.refs.remove(&id);
            let b = id as usize % self.buckets.len();
            self.buckets[b].retain(|x| *x != id);
        }
        true
    }
    /// Buckets ascend, chain walks head to tail. Client retains the last
    /// containing local box; if none, it retains the last containing global.
    pub fn select(&self, mut classify: impl FnMut(u32) -> Option<bool>) -> Option<u32> {
        let mut local = None;
        let mut global = None;
        for &id in self.buckets.iter().flatten() {
            match classify(id) {
                Some(false) => local = Some(id),
                Some(true) => global = Some(id),
                None => {}
            }
        }
        local.or(global)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn head_insertion_and_local_priority() {
        let mut m = ActiveBoxMap::default();
        m.load(1);
        m.load(54);
        m.load(6); // bucket 1: 54 then 1; bucket 6: 6
        assert_eq!(m.select(|id| Some(id == 6)), Some(1));
        assert_eq!(
            m.select(|id| if id == 6 { Some(true) } else { None }),
            Some(6)
        );
        m.load(1);
        assert!(m.unload(1));
        assert_eq!(m.select(|_| Some(false)), Some(6));
        assert!(m.unload(1));
        assert!(!m.unload(1));
    }
    #[test]
    fn resize_rehash_reverses_collisions() {
        let mut m = ActiveBoxMap::default();
        for id in 0..54 {
            m.load(id * 53);
        }
        assert_eq!(m.buckets.len(), 97);
        assert_eq!(m.refs.len(), 54);
        for i in 0..54 {
            assert!(m.unload(i * 53));
        }
        assert!(m.select(|_| Some(false)).is_none());
    }
}
