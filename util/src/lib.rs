use std::cell::Cell;

/// Simple monotonic id generator for in-process identifiers.
#[derive(Debug)]
pub struct IdGenerator {
    next: Cell<u64>,
}

impl IdGenerator {
    /// Creates a new generator starting at the provided value.
    pub fn new(start: u64) -> Self {
        Self {
            next: Cell::new(start),
        }
    }

    /// Returns the next id in sequence.
    pub fn next(&self) -> u64 {
        let id = self.next.get();
        self.next.set(id + 1);
        id
    }
}

impl Default for IdGenerator {
    fn default() -> Self {
        Self::new(1)
    }
}
