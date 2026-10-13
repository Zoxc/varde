//! A component's Jacobian, kept sparse.

use faer::Mat;

use super::system::{Component, System};

/// A component's Jacobian, a row per equation, each row's entries by
/// the variable's position in the component.
pub(super) struct Jacobian {
    starts: Vec<usize>,
    entries: Vec<(usize, f64)>,
}

/// A [`Jacobian`]'s sparsity pattern: where each row's entries start, and
/// each entry's column.
pub(super) struct Pattern {
    starts: Vec<usize>,
    columns: Vec<usize>,
}

impl Jacobian {
    /// At the system's values, each variable's column multiplied by its
    /// entry of `scaling`, if given.
    pub(super) fn new(system: &System, component: &Component, scaling: Option<&[f64]>) -> Self {
        let mut starts = Vec::with_capacity(component.equations.len() + 1);
        let mut entries = Vec::new();
        for &index in &component.equations {
            starts.push(entries.len());
            system.equations[index]
                .residual
                .gradient(&system.values, |var, derivative| {
                    let column = component.column(var);
                    let scale = scaling.map_or(1.0, |scaling| scaling[column]);
                    entries.push((column, derivative * scale));
                });
        }
        starts.push(entries.len());
        Jacobian { starts, entries }
    }

    /// Whether every entry is a number.
    pub(super) fn is_finite(&self) -> bool {
        self.entries.iter().all(|(_, value)| value.is_finite())
    }

    pub(super) fn row(&self, row: usize) -> &[(usize, f64)] {
        &self.entries[self.starts[row]..self.starts[row + 1]]
    }

    /// Each entry of `row` as its index among all the entries, which
    /// [`Jacobian::value`] takes, and its column.
    pub(super) fn indexed_row(&self, row: usize) -> impl Iterator<Item = (usize, usize)> + '_ {
        (self.starts[row]..self.starts[row + 1]).map(|index| (index, self.entries[index].0))
    }

    /// The value of the entry at `index` among all the entries.
    pub(super) fn value(&self, index: usize) -> f64 {
        self.entries[index].1
    }

    pub(super) fn pattern(&self) -> Pattern {
        Pattern {
            starts: self.starts.clone(),
            columns: self.entries.iter().map(|&(column, _)| column).collect(),
        }
    }

    /// Whether the Jacobian has `pattern`.
    pub(super) fn has_pattern(&self, pattern: &Pattern) -> bool {
        self.starts == pattern.starts
            && pattern
                .columns
                .iter()
                .zip(&self.entries)
                .all(|(&column, &(other, _))| column == other)
    }

    pub(super) fn rows(&self) -> usize {
        self.starts.len() - 1
    }

    /// As a dense matrix of `columns` columns, entries that aren't
    /// numbers taken as zero.
    pub(super) fn dense(&self, columns: usize) -> Mat<f64> {
        let mut matrix = Mat::zeros(self.rows(), columns);
        for row in 0..self.rows() {
            for &(column, value) in self.row(row) {
                if value.is_finite() {
                    matrix[(row, column)] += value;
                }
            }
        }
        matrix
    }
}
