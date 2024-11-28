use itertools::Itertools;
use stwo_prover::{
    constraint_framework::{assert_constraints, logup::LookupElements},
    core::{
        backend::simd::SimdBackend,
        channel::Blake2sChannel,
        fields::m31::BaseField,
        fri::FriConfig,
        pcs::{CommitmentSchemeProver, PcsConfig, TreeVec},
        poly::{
            circle::{CanonicCoset, CircleEvaluation, PolyOps},
            BitReversedOrder,
        },
        vcs::blake2_merkle::Blake2sMerkleChannel,
    },
};

use crate::machine2::{
    trace::{eval::TraceEval, Traces},
    traits::MachineChip,
};

pub(crate) fn test_params(
    log_size: u32,
) -> (
    PcsConfig,
    stwo_prover::core::poly::twiddles::TwiddleTree<SimdBackend>,
) {
    let config = PcsConfig {
        pow_bits: 10,
        fri_config: FriConfig::new(5, 4, 64), // should I change this?
    };
    let twiddles = SimdBackend::precompute_twiddles(
        // The + 1 is taken from the stwo examples. I don't know why it's needed.
        CanonicCoset::new(log_size + config.fri_config.log_blowup_factor + 1)
            .circle_domain()
            .half_coset,
    );
    (config, twiddles)
}

/// Filled out traces, mainly for testing
pub(crate) struct CommittedTraces<'a> {
    pub(crate) commitment_scheme: CommitmentSchemeProver<'a, SimdBackend, Blake2sMerkleChannel>,
    pub(crate) prover_channel: Blake2sChannel,
    pub(crate) lookup_elements: LookupElements<12>,
    pub(crate) preprocessed_trace: Traces,
    pub(crate) interaction_trace: Vec<CircleEvaluation<SimdBackend, BaseField, BitReversedOrder>>,
}

/// Testing utility for filling in traces
pub(crate) fn commit_traces<'a, C: MachineChip>(
    config: PcsConfig,
    twiddles: &'a stwo_prover::core::poly::twiddles::TwiddleTree<SimdBackend>,
    traces: &Traces,
    custom_preprocessed: Option<Traces>,
) -> CommittedTraces<'a> {
    let mut commitment_scheme =
        CommitmentSchemeProver::<_, Blake2sMerkleChannel>::new(config, twiddles);
    let mut prover_channel = Blake2sChannel::default();
    // Preprocessed trace
    let preprocessed_trace =
        custom_preprocessed.unwrap_or_else(|| Traces::new_preprocessed_trace(traces.log_size()));
    let mut tree_builder = commitment_scheme.tree_builder();
    let _preprocessed_trace_location =
        tree_builder.extend_evals(preprocessed_trace.circle_evaluation());
    tree_builder.commit(&mut prover_channel);

    // Original trace
    let mut tree_builder = commitment_scheme.tree_builder();
    let _main_trace_location = tree_builder.extend_evals(traces.circle_evaluation());
    tree_builder.commit(&mut prover_channel);
    let lookup_elements = LookupElements::draw(&mut prover_channel);

    // Interaction Trace
    let interaction_trace =
        C::fill_interaction_trace(traces, &preprocessed_trace, &lookup_elements);
    let mut tree_builder = commitment_scheme.tree_builder();
    let _interaction_trace_location = tree_builder.extend_evals(interaction_trace.clone());
    tree_builder.commit(&mut prover_channel);
    CommittedTraces {
        commitment_scheme,
        prover_channel,
        lookup_elements,
        preprocessed_trace,
        interaction_trace,
    }
}

/// Assuming traces are filled, assert constraints
pub(crate) fn assert_chip<C: MachineChip>(traces: Traces, custom_preprocessed: Option<Traces>) {
    let (config, twiddles) = test_params(traces.log_size());

    let CommittedTraces {
        commitment_scheme: _,
        prover_channel: _,
        lookup_elements,
        preprocessed_trace,
        interaction_trace,
    } = commit_traces::<C>(config, &twiddles, &traces, custom_preprocessed);

    let trace_evals = TreeVec::new(vec![
        preprocessed_trace.circle_evaluation(),
        traces.circle_evaluation(),
        interaction_trace
            .iter()
            .map(|col| col.to_cpu())
            .collect_vec(),
    ]);
    let trace_polys = trace_evals.map(|trace| {
        trace
            .into_iter()
            .map(|c| c.interpolate())
            .collect::<Vec<_>>()
    });

    // Now check the constraints to make sure they're satisfied
    assert_constraints(
        &trace_polys,
        CanonicCoset::new(traces.log_size()),
        |mut eval| {
            let trace_eval = TraceEval::new(&mut eval);
            C::add_constraints(&mut eval, &trace_eval, &lookup_elements);
        },
    );
}
