#![forbid(unsafe_code)]

use std::collections::HashSet;

const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

/// Deterministic 64-bit identifier for a UI widget or layout node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WidgetId(pub u64);

fn fnv1a_bytes(mut hash: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Scoped FNV-1a identifier stack with per-frame interactive collision detection.
pub struct IdStack {
    seeds: Vec<u64>,
    seen_interactive: HashSet<WidgetId>,
    collisions: Vec<WidgetId>,
}

impl Default for IdStack {
    fn default() -> Self {
        Self::new()
    }
}

impl IdStack {
    /// Creates a new identifier stack rooted at the FNV-1a offset basis.
    pub fn new() -> Self {
        Self {
            seeds: vec![FNV_OFFSET_BASIS],
            seen_interactive: HashSet::new(),
            collisions: Vec::new(),
        }
    }

    /// Clears per-frame state and restores the root seed while retaining capacity.
    pub fn clear_frame(&mut self) {
        self.seeds.clear();
        self.seeds.push(FNV_OFFSET_BASIS);
        self.seen_interactive.clear();
        self.collisions.clear();
    }

    fn current_seed(&self) -> u64 {
        *self.seeds.last().unwrap_or(&FNV_OFFSET_BASIS)
    }

    /// Pushes a string scope onto the ID stack.
    pub fn push_id(&mut self, label: &str) {
        let next = fnv1a_bytes(self.current_seed(), label.as_bytes());
        let separated = fnv1a_bytes(next, &[0xff]);
        self.seeds.push(separated);
    }

    /// Pushes a numeric scope (such as a loop index) onto the ID stack.
    pub fn push_u64(&mut self, value: u64) {
        let next = fnv1a_bytes(self.current_seed(), &value.to_le_bytes());
        let separated = fnv1a_bytes(next, &[0xfe]);
        self.seeds.push(separated);
    }

    /// Pops the most recent scope without popping the root seed.
    pub fn pop_id(&mut self) {
        if self.seeds.len() > 1 {
            self.seeds.pop();
        }
    }

    /// Derives a deterministic `WidgetId` from the current scope and `label`.
    pub fn id(&self, label: &str) -> WidgetId {
        WidgetId(fnv1a_bytes(self.current_seed(), label.as_bytes()))
    }

    /// Registers an interactive `WidgetId` for the current frame.
    ///
    /// Returns `false` and records the ID in `collisions()` if already seen this frame.
    pub fn register_interactive(&mut self, id: WidgetId) -> bool {
        if self.seen_interactive.insert(id) {
            true
        } else {
            self.collisions.push(id);
            false
        }
    }

    /// Returns duplicate interactive IDs detected during the current frame.
    pub fn collisions(&self) -> &[WidgetId] {
        &self.collisions
    }
}

#[cfg(test)]
mod tests {
    use super::IdStack;

    #[test]
    fn scoped_determinism_and_collision_detection() {
        let mut ids = IdStack::new();
        let a1 = ids.id("save");
        ids.push_id("panel_a");
        let b1 = ids.id("save");
        ids.pop_id();

        ids.push_id("panel_b");
        let b2 = ids.id("save");
        ids.pop_id();

        assert_ne!(a1, b1);
        assert_ne!(b1, b2);

        ids.clear_frame();
        ids.push_id("panel_a");
        let b1_repeat = ids.id("save");
        ids.pop_id();
        assert_eq!(b1, b1_repeat);

        assert!(ids.register_interactive(b1));
        assert!(ids.register_interactive(b2));
        assert!(!ids.register_interactive(b1));
        assert_eq!(ids.collisions(), &[b1]);
    }
}
