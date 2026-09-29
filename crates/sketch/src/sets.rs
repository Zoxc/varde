//! Disjoint sets of indices, for what the solver's equations tie together
//! and what's connected among a profile's pieces.

/// Disjoint sets of the indices below a count, joined by union-find, each
/// named by its lowest index.
pub(crate) struct Sets(Vec<usize>);

impl Sets {
    /// Each index below `count` in a set of its own.
    pub(crate) fn new(count: usize) -> Sets {
        Sets((0..count).collect())
    }

    /// The lowest index in `i`'s set.
    pub(crate) fn root(&mut self, mut i: usize) -> usize {
        while self.0[i] != i {
            self.0[i] = self.0[self.0[i]];
            i = self.0[i];
        }
        i
    }

    /// Makes the sets of `a` and `b` one.
    pub(crate) fn join(&mut self, a: usize, b: usize) {
        let (a, b) = (self.root(a), self.root(b));
        self.0[a.max(b)] = a.min(b);
    }
}
