use eval::TraceEval;
use itertools::Itertools as _;
use num_traits::{One as _, Zero};
use stwo_prover::{
    constraint_framework::{assert_constraints, AssertEvaluator},
    core::{
        backend::{
            simd::{column::BaseColumn, m31::LOG_N_LANES},
            Backend, CpuBackend,
        },
        fields::m31::BaseField,
        pcs::TreeVec,
        poly::{
            circle::{CanonicCoset, CircleEvaluation},
            BitReversedOrder,
        },
        ColumnVec,
    },
};

use crate::machine2::column::PreprocessedColumn;

use super::column::Column;

pub mod eval;
pub mod program;
pub mod utils;

pub use program::{ProgramStep, Word, WordWithEffectiveBits};

use utils::{bit_reverse, coset_order_to_circle_domain_order};

pub struct Traces {
    cols: Vec<Vec<BaseField>>,
    log_size: u32,
}

impl Traces {
    /// Returns [`Column::TOTAL_COLUMNS_NUM`] zeroed columns, each one `2.pow(log_size)` in length.
    pub(crate) fn new(log_size: u32) -> Self {
        assert!(log_size >= LOG_N_LANES);
        Self {
            cols: vec![vec![BaseField::zero(); 1 << log_size]; Column::COLUMNS_NUM],
            log_size,
        }
    }

    /// Returns [`Column::COLUMNS_NUM`] columns, each one `2.pow(log_size)` in length, filled with preprocessed trace content.
    pub(crate) fn new_preprocessed_trace(log_size: u32) -> Self {
        assert!(log_size >= LOG_N_LANES);
        assert!(
            log_size >= 8,
            "log_size must be at least 8, to accomodate 256-element lookup tables"
        );
        let mut cols =
            vec![vec![BaseField::zero(); 1 << log_size]; PreprocessedColumn::COLUMNS_NUM];
        cols[PreprocessedColumn::IsFirst.offset()][0] = BaseField::one();
        for row_idx in 0..256 {
            cols[PreprocessedColumn::Range256.offset()][row_idx] = BaseField::from(row_idx);
        }
        Self { cols, log_size }
    }

    /// Returns inner representation of columns.
    pub fn into_inner(self) -> Vec<Vec<BaseField>> {
        self.cols
    }

    /// Returns the log_size of columns.
    pub fn log_size(&self) -> u32 {
        self.log_size
    }

    /// Returns a copy of `N` raw columns in range `[offset..offset + N]` at `row`, where
    /// `N` is assumed to be equal `Column::size` of a `col`.
    #[doc(hidden)]
    pub fn column<const N: usize>(&self, row: usize, col: Column) -> [BaseField; N] {
        assert_eq!(col.size(), N, "column size mismatch");

        let offset = col.offset();
        let mut iter = self.cols[offset..].iter();
        std::array::from_fn(|_idx| iter.next().expect("invalid offset; must be unreachable")[row])
    }

    /// Returns mutable reference to `N` raw columns in range `[offset..offset + N]` at `row`,
    /// where `N` is assumed to be equal `Column::size` of a `col`.
    #[doc(hidden)]
    pub fn column_mut<const N: usize>(&mut self, row: usize, col: Column) -> [&mut BaseField; N] {
        assert_eq!(col.size(), N, "column size mismatch");

        let offset = col.offset();
        let mut iter = self.cols[offset..].iter_mut();
        std::array::from_fn(|_idx| {
            &mut iter.next().expect("invalid offset; must be unreachable")[row]
        })
    }

    /// Fills columns with values from a byte slice.
    pub fn fill_columns(&mut self, row: usize, value: &[u8], col: Column) {
        let n = value.len();
        assert_eq!(col.size(), n, "column size mismatch");
        for (i, b) in value.iter().enumerate() {
            self.cols[col.offset() + i][row] = BaseField::from(*b as u32);
        }
    }

    /// Fills columns with values from a byte slice, applying a selector.
    ///
    /// If the selector is true, fills the columns with zeros. Otherwise, fills with values from the byte slice.
    pub fn fill_effective_columns(
        &mut self,
        row: usize,
        value: &[u8],
        col: Column,
        selector: bool,
    ) {
        let n = value.len();
        assert_eq!(col.size(), n, "column size mismatch");
        for (i, b) in value.iter().enumerate() {
            self.cols[col.offset() + i][row] = if selector {
                BaseField::zero()
            } else {
                BaseField::from(*b as u32)
            };
        }
    }

    /// Returns a copy of `N` raw columns in range `[offset..offset + N]` in the bit-reversed BaseColumn format.
    ///
    /// This function allows SIMD-aware stwo libraries (for instance, logup) to read columns in the format they expect.
    pub fn get_base_column<const N: usize>(&self, col: Column) -> [BaseColumn; N] {
        assert_eq!(col.size(), N, "column size mismatch");
        self.cols[col.offset()..]
            .iter()
            .take(N)
            .map(|column_in_trace_order| {
                let mut tmp_col =
                    coset_order_to_circle_domain_order(column_in_trace_order.as_slice());
                bit_reverse(&mut tmp_col);
                BaseColumn::from_iter(tmp_col)
            })
            .collect_vec()
            .try_into()
            .expect("wrong size?")
    }

    /// Converts traces into circle domain evaluations, bit-reversing row indices
    /// according to circle domain ordering.
    pub fn circle_evaluation<B>(
        &self,
    ) -> ColumnVec<CircleEvaluation<B, BaseField, BitReversedOrder>>
    where
        B: Backend,
    {
        let domain = CanonicCoset::new(self.log_size).circle_domain();
        self.cols
            .iter()
            .map(|col| {
                let mut eval = coset_order_to_circle_domain_order(col.as_slice());
                bit_reverse(&mut eval);

                CircleEvaluation::<B, _, BitReversedOrder>::new(domain, eval.into_iter().collect())
            })
            .collect()
    }
    /// Asserts add_constraints_calls() in a main trace
    ///
    /// This function combines the trace with an empty preprocessed-trace and
    /// an empty interaction trace and then calls `add_constraints_calls()` on
    /// the combination. This is useful in test cases.
    pub fn assert_as_original_trace<F>(self, add_constraints_calls: F)
    where
        F: for<'a, 'b, 'c> Fn(&'a mut AssertEvaluator<'c>, &'b TraceEval<AssertEvaluator<'c>>),
    {
        let log_size = self.log_size;
        // Convert traces to the format expected by assert_constraints
        let traces: Vec<CircleEvaluation<CpuBackend, BaseField, BitReversedOrder>> =
            self.circle_evaluation();

        let preprocessed_trace = Traces::new_preprocessed_trace(log_size).circle_evaluation();

        let traces = TreeVec::new(vec![
            preprocessed_trace,
            traces,
            vec![], /* interaction trace */
        ]);
        let trace_polys = traces.map(|trace| {
            trace
                .into_iter()
                .map(|c| c.interpolate())
                .collect::<Vec<_>>()
        });

        // Now check the constraints to make sure they're satisfied
        assert_constraints(&trace_polys, CanonicCoset::new(log_size), |mut eval| {
            let trace_eval = TraceEval::new(&mut eval);
            add_constraints_calls(&mut eval, &trace_eval);
        });
    }
}
