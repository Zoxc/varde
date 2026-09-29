//! The normal matrix `A Aᵀ` of a component's Jacobian `A`, kept sparse,
//! and solving with it: most equations share variables with only a few
//! others, so its sparse Cholesky factorization costs a fraction of a
//! dense one.

use faer::prelude::Solve;
use faer::sparse::linalg::solvers::{Llt, SymbolicLlt};
use faer::sparse::{SparseColMatRef, SymbolicSparseColMat};
use faer::{Mat, Side};

use super::Jacobian;

/// `A Aᵀ` for Jacobians of one sparsity pattern: its lower triangle,
/// column by column, with the pattern's symbolic factorization made once.
pub(super) struct Normal {
    pattern: SymbolicSparseColMat<usize>,
    symbolic: SymbolicLlt<usize>,
    /// Per column of the Jacobian (variable): the rows with an entry in
    /// it, each with the entry's index among the Jacobian's entries.
    by_column: Vec<Vec<(usize, usize)>>,
    /// The index of each diagonal entry among the values.
    diagonal: Vec<usize>,
    values: Vec<f64>,
    /// The Jacobian's pattern it was made for: where each row's entries
    /// start, and each entry's column.
    starts: Vec<usize>,
    columns: Vec<usize>,
}

impl Normal {
    /// The normal matrix of Jacobians shaped like `jacobian`, which has
    /// `columns` columns, filled from it. `None` if faer can't make the
    /// symbolic factorization (out of memory).
    pub fn new(jacobian: &Jacobian, columns: usize) -> Option<Self> {
        let rows = jacobian.rows();
        let mut by_column: Vec<Vec<(usize, usize)>> = vec![Vec::new(); columns];
        for row in 0..rows {
            for index in jacobian.starts[row]..jacobian.starts[row + 1] {
                by_column[jacobian.entries[index].0].push((row, index));
            }
        }
        let mut col_ptr = vec![0];
        let mut row_idx = Vec::new();
        let mut diagonal = Vec::with_capacity(rows);
        for row in 0..rows {
            let start = row_idx.len();
            row_idx.push(row);
            for &(column, _) in jacobian.row(row) {
                row_idx.extend(
                    by_column[column]
                        .iter()
                        .map(|&(other, _)| other)
                        .filter(|&other| other > row),
                );
            }
            row_idx[start..].sort_unstable();
            let mut end = start;
            for i in start..row_idx.len() {
                if i == start || row_idx[i] != row_idx[end - 1] {
                    row_idx[end] = row_idx[i];
                    end += 1;
                }
            }
            row_idx.truncate(end);
            diagonal.push(start);
            col_ptr.push(row_idx.len());
        }
        let pattern = SymbolicSparseColMat::new_checked(rows, rows, col_ptr, None, row_idx);
        let symbolic = SymbolicLlt::try_new(pattern.as_ref(), Side::Lower).ok()?;
        let values = vec![0.0; pattern.row_idx().len()];
        let mut normal = Normal {
            pattern,
            symbolic,
            by_column,
            diagonal,
            values,
            starts: jacobian.starts.clone(),
            columns: jacobian.entries.iter().map(|&(column, _)| column).collect(),
        };
        normal.fill(jacobian);
        Some(normal)
    }

    /// Whether `jacobian` has the pattern of the one it was made from,
    /// which [`Normal::fill`] takes. An equation reading a spline where a
    /// variable parameter is reads other points of it once the parameter
    /// passes a knot.
    pub fn fits(&self, jacobian: &Jacobian) -> bool {
        self.starts == jacobian.starts
            && self
                .columns
                .iter()
                .zip(&jacobian.entries)
                .all(|(&column, &(other, _))| column == other)
    }

    /// Recomputes the values from `jacobian`, shaped like the one it was
    /// made from.
    pub fn fill(&mut self, jacobian: &Jacobian) {
        self.values.fill(0.0);
        let col_ptr = self.pattern.col_ptr();
        let row_idx = self.pattern.row_idx();
        for row in 0..jacobian.rows() {
            let rows = &row_idx[col_ptr[row]..col_ptr[row + 1]];
            for &(column, a) in jacobian.row(row) {
                for &(other, index) in &self.by_column[column] {
                    if other >= row
                        && let Ok(position) = rows.binary_search(&other)
                    {
                        self.values[col_ptr[row] + position] += a * jacobian.entries[index].1;
                    }
                }
            }
        }
    }

    /// The largest diagonal entry.
    pub fn largest_diagonal(&self) -> f64 {
        self.diagonal
            .iter()
            .map(|&index| self.values[index])
            .fold(0.0, f64::max)
    }

    /// Solves `(A Aᵀ + λ I) u = rhs`. `None` if the matrix isn't positive
    /// definite as far as the factorization can tell.
    pub fn solve(&self, lambda: f64, rhs: &[f64]) -> Option<Vec<f64>> {
        let mut values = self.values.clone();
        for &index in &self.diagonal {
            values[index] += lambda;
        }
        let matrix = SparseColMatRef::new(self.pattern.as_ref(), &values);
        let llt = Llt::try_new_with_symbolic(self.symbolic.clone(), matrix, Side::Lower).ok()?;
        let mut u = Mat::from_fn(rhs.len(), 1, |i, _| rhs[i]);
        llt.solve_in_place(u.as_mut());
        Some((0..rhs.len()).map(|i| u[(i, 0)]).collect())
    }
}
