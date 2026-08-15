use std::collections::{BTreeMap, BTreeSet};

use efct_engine::{CallEdge, FunctionEffects, WellFoundedSelfCall};

use crate::hir::{Function, Module};

use self::control_flow::analyze_block;
use self::evidence::PathFacts;
use super::FunctionSignature;

mod control_flow;
mod evidence;

pub(super) fn prove_well_founded_self_calls(
    module: &Module,
    signatures: &BTreeMap<String, FunctionSignature>,
    bodies: &BTreeMap<String, FunctionEffects>,
) -> BTreeSet<WellFoundedSelfCall> {
    let mut proven = BTreeSet::new();
    for function in &module.functions {
        let Some(signature) = signatures.get(&function.name) else {
            continue;
        };
        let Some(effects) = bodies.get(&function.name) else {
            continue;
        };
        let actual_self_calls: BTreeSet<_> = effects
            .calls
            .iter()
            .filter_map(|call| match call {
                CallEdge::Invoke { target, span, .. } if target == &function.name => Some(*span),
                CallEdge::Invoke { .. } | CallEdge::Reference { .. } => None,
            })
            .collect();
        if actual_self_calls.is_empty() {
            continue;
        }

        let candidates = call_measure_candidates(function, signature);
        let mut common_measure: Option<BTreeSet<String>> = None;
        for span in &actual_self_calls {
            let Some(measures) = candidates.get(span) else {
                common_measure = Some(BTreeSet::new());
                break;
            };
            common_measure = Some(match common_measure {
                Some(common) => common.intersection(measures).cloned().collect(),
                None => measures.clone(),
            });
        }
        if common_measure.is_some_and(|measures| !measures.is_empty()) {
            proven.extend(
                actual_self_calls
                    .into_iter()
                    .map(|span| WellFoundedSelfCall {
                        function: function.name.clone(),
                        span,
                    }),
            );
        }
    }
    proven
}

fn call_measure_candidates(
    function: &Function,
    signature: &FunctionSignature,
) -> BTreeMap<efct_protocol::SourceSpan, BTreeSet<String>> {
    analyze_block(
        &function.body,
        PathFacts::new(function, signature),
        function,
    )
    .calls
    .into_iter()
    .map(|call| (call.span, call.measures))
    .collect()
}
