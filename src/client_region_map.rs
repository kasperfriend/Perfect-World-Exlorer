//! Client-like active-box hash iteration (elementclient 0x41b23b, 0x41da70,
//! 0x413170, 0x41eb7c). Scene window admission order is implemented by
//! client_region_stream; this module preserves insertion/refcount/hash order.
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
    pub fn select(&self, classify: impl FnMut(u32) -> Option<bool>) -> Option<u32> {
        let (local, global) = self.select_pair(classify);
        local.or(global)
    }
    /// Keep the default separately: local fog/colors do not override its light direction.
    pub fn select_pair(
        &self,
        mut classify: impl FnMut(u32) -> Option<bool>,
    ) -> (Option<u32>, Option<u32>) {
        let mut local = None;
        let mut global = None;
        for &id in self.buckets.iter().flatten() {
            match classify(id) {
                Some(false) => local = Some(id),
                Some(true) => global = Some(id),
                None => {}
            }
        }
        (local, global)
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
        assert_eq!(m.select_pair(|id| Some(id == 6)), (Some(1), Some(6)));
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
    fn overlapping_default_boxes_keep_the_last_native_bucket_match() {
        let mut m = ActiveBoxMap::default();
        // 5 and 58 collide in bucket 5. Head insertion visits 58 then 5;
        // bucket 6 is visited afterward, so default 6 must win the tie.
        for id in [5, 58, 6] {
            m.load(id);
        }
        assert_eq!(m.select_pair(|_| Some(true)), (None, Some(6)));
    }

    #[test]
    fn supplied_world_local_overlap_uses_native_bucket_precedence() {
        // Surveyed 17x17 window at the intersection of local boxes 37906 and
        // 38176 contains seven unique areas. In the native 53-bucket table,
        // 37906 lands in bucket 11 and 38176 in bucket 16, so the latter is
        // the last matching local box regardless of their within-cell order.
        let mut m = ActiveBoxMap::default();
        for id in [35755, 37447, 37904, 37906, 38176, 39053, 39986] {
            m.load(id);
        }
        assert_eq!(
            m.select_pair(|id| match id {
                37906 | 38176 => Some(false),
                37904 => Some(true),
                _ => None,
            }),
            (Some(38176), Some(37904))
        );
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
