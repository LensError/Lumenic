#![forbid(unsafe_code)]

/// Bounded generic undo/redo history stack.
#[derive(Clone, Debug)]
pub struct History<T: Clone> {
    undo_stack: Vec<T>,
    redo_stack: Vec<T>,
    max_entries: usize,
}

impl<T: Clone> Default for History<T> {
    fn default() -> Self {
        Self::new(128)
    }
}

impl<T: Clone> History<T> {
    /// Creates an empty undo/redo stack keeping up to `max_entries` states.
    pub fn new(max_entries: usize) -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            max_entries: max_entries.max(1),
        }
    }

    /// Pushes a snapshot onto the undo stack and clears any pending redo branch.
    pub fn record(&mut self, state: T) {
        self.redo_stack.clear();
        if self.undo_stack.len() >= self.max_entries {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(state);
    }

    /// Undoes to the most recent prior state, pushing `current` onto the redo stack.
    pub fn undo(&mut self, current: T) -> Option<T> {
        let prev = self.undo_stack.pop()?;
        if self.redo_stack.len() >= self.max_entries {
            self.redo_stack.remove(0);
        }
        self.redo_stack.push(current);
        Some(prev)
    }

    /// Re-applies the most recently undone state, pushing `current` onto the undo stack.
    pub fn redo(&mut self, current: T) -> Option<T> {
        let next = self.redo_stack.pop()?;
        if self.undo_stack.len() >= self.max_entries {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(current);
        Some(next)
    }

    /// Returns `true` if at least one undo step is available.
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Returns `true` if at least one redo step is available.
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    /// Clears both undo and redo stacks.
    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::History;

    #[test]
    fn bounded_undo_redo_and_branch_clear() {
        let mut h = History::new(2);
        h.record("a".to_string());
        h.record("b".to_string());
        h.record("c".to_string());

        assert!(h.can_undo());
        let s1 = h.undo("d".to_string()).expect("undo 1");
        assert_eq!(s1, "c");
        let s2 = h.undo(s1).expect("undo 2");
        assert_eq!(s2, "b");
        assert!(h.undo(s2.clone()).is_none());

        let r1 = h.redo(s2).expect("redo 1");
        assert_eq!(r1, "c");
        assert!(h.can_redo());

        h.record("e".to_string());
        assert!(!h.can_redo());
    }
}
