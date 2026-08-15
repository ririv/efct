use std::collections::{BTreeMap, BTreeSet, VecDeque};

use efct_model::{
    Effect, EffectFormula, EffectSet, EffectTerm, EffectTraceFrame, EffectVariable, SourceSpan,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DirectEffectOrigin {
    pub span: SourceSpan,
    pub operation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct WellFoundedSelfCall {
    pub function: String,
    pub span: SourceSpan,
}

#[derive(Debug, Clone)]
pub struct FunctionEffects {
    pub filename: String,
    pub direct: EffectFormula,
    pub direct_origins: BTreeMap<EffectTerm, BTreeSet<DirectEffectOrigin>>,
    pub calls: BTreeSet<CallEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum CallEffectPropagation {
    AllExcept(EffectSet),
    SuppressConcreteExceptionalBehavior,
}

impl CallEffectPropagation {
    fn apply(&self, effects: &EffectFormula) -> EffectFormula {
        match self {
            Self::AllExcept(handled) => effects.without_handled(handled),
            Self::SuppressConcreteExceptionalBehavior => {
                effects.without_concrete_exceptional_behavior()
            }
        }
    }

    fn permits(&self, effect: &Effect) -> bool {
        match self {
            Self::AllExcept(handled) => !handled.contains(effect),
            Self::SuppressConcreteExceptionalBehavior => matches!(
                effect,
                Effect::External(_) | Effect::Partial(efct_model::PartialBehavior::Diverge)
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum CallEdge {
    Invoke {
        target: String,
        span: SourceSpan,
        propagation: CallEffectPropagation,
        bindings: BTreeMap<EffectVariable, EffectFormula>,
    },
    Reference {
        target: String,
        span: SourceSpan,
    },
}

impl CallEdge {
    #[must_use]
    pub fn target(&self) -> &str {
        match self {
            Self::Invoke { target, .. } | Self::Reference { target, .. } => target,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectTrace {
    pub labels: Vec<String>,
    pub frames: Vec<EffectTraceFrame>,
}

pub fn infer_effect_fixed_point(
    functions: &BTreeMap<String, FunctionEffects>,
    valid: &BTreeSet<String>,
) -> BTreeMap<String, EffectFormula> {
    let mut actual: BTreeMap<String, EffectFormula> = valid
        .iter()
        .map(|name| (name.clone(), functions[name].direct.clone()))
        .collect();
    loop {
        let mut changed = false;
        for name in valid {
            let mut inferred = functions[name].direct.clone();
            for call in &functions[name].calls {
                if let CallEdge::Invoke {
                    target,
                    propagation,
                    bindings,
                    ..
                } = call
                    && let Some(effects) = actual.get(target)
                {
                    inferred.extend(propagation.apply(&effects.substitute(bindings)));
                }
            }
            if actual.get(name) != Some(&inferred) {
                actual.insert(name.clone(), inferred);
                changed = true;
            }
        }
        if !changed {
            return actual;
        }
    }
}

pub fn record_recursive_divergence(
    functions: &mut BTreeMap<String, FunctionEffects>,
    valid: &BTreeSet<String>,
    well_founded_self_calls: &BTreeSet<WellFoundedSelfCall>,
) {
    let origins: Vec<_> = valid
        .iter()
        .filter_map(|name| {
            let function = functions.get(name)?;
            function.calls.iter().find_map(|call| {
                let CallEdge::Invoke { target, span, .. } = call else {
                    return None;
                };
                if target == name
                    && well_founded_self_calls.contains(&WellFoundedSelfCall {
                        function: name.clone(),
                        span: *span,
                    })
                {
                    return None;
                }
                if !valid.contains(target)
                    || (target != name && !invocation_path_exists(target, name, functions, valid))
                {
                    return None;
                }
                Some((name.clone(), target.clone(), *span))
            })
        })
        .collect();
    let divergence = Effect::Partial(efct_model::PartialBehavior::Diverge);
    for (name, target, span) in origins {
        let function = functions
            .get_mut(&name)
            .expect("recursive origin belongs to a valid function");
        function.direct.insert(divergence.clone());
        function
            .direct_origins
            .entry(EffectTerm::Concrete(divergence.clone()))
            .or_default()
            .insert(DirectEffectOrigin {
                span,
                operation: format!("Recursive call to {target}"),
            });
    }
}

fn invocation_path_exists(
    start: &str,
    goal: &str,
    functions: &BTreeMap<String, FunctionEffects>,
    valid: &BTreeSet<String>,
) -> bool {
    let mut pending = vec![start];
    let mut visited = BTreeSet::new();
    while let Some(name) = pending.pop() {
        if name == goal {
            return true;
        }
        if !visited.insert(name) {
            continue;
        }
        let Some(function) = functions.get(name) else {
            continue;
        };
        for call in &function.calls {
            if let CallEdge::Invoke { target, .. } = call
                && valid.contains(target)
            {
                pending.push(target);
            }
        }
    }
    false
}

pub fn effect_trace(
    origin: &str,
    target: &Effect,
    functions: &BTreeMap<String, FunctionEffects>,
) -> Vec<String> {
    effect_trace_details(origin, target, functions).labels
}

pub fn effect_trace_details(
    origin: &str,
    target: &Effect,
    functions: &BTreeMap<String, FunctionEffects>,
) -> EffectTrace {
    let mut queue = VecDeque::from([(
        origin.to_owned(),
        vec![origin.to_owned()],
        Vec::<EffectTraceFrame>::new(),
    )]);
    let mut visited = BTreeSet::new();
    while let Some((name, path, frames)) = queue.pop_front() {
        if !visited.insert(name.clone()) {
            continue;
        }
        let Some(function) = functions.get(&name) else {
            continue;
        };
        if function.direct.contains_effect(target) {
            let term = EffectTerm::Concrete(target.clone());
            let mut result_frames = frames;
            if let Some(origin) = function
                .direct_origins
                .get(&term)
                .and_then(|origins| origins.first())
            {
                result_frames.push(EffectTraceFrame {
                    function: name.clone(),
                    filename: function.filename.clone(),
                    span: origin.span,
                    operation: origin.operation.clone(),
                });
            }
            let mut labels = path;
            labels.push(target.to_string());
            return EffectTrace {
                labels,
                frames: result_frames,
            };
        }
        for call in &function.calls {
            let CallEdge::Invoke {
                target: callee,
                span,
                propagation,
                ..
            } = call
            else {
                continue;
            };
            if !propagation.permits(target) {
                continue;
            }
            let mut next_path = path.clone();
            next_path.push(callee.clone());
            let mut next_frames = frames.clone();
            next_frames.push(EffectTraceFrame {
                function: name.clone(),
                filename: function.filename.clone(),
                span: *span,
                operation: format!("Call {callee}"),
            });
            queue.push_back((callee.clone(), next_path, next_frames));
        }
    }
    EffectTrace {
        labels: vec![origin.to_owned(), target.to_string()],
        frames: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use efct_model::{ExceptionId, ExternalEffect, PartialBehavior};

    fn test_span() -> SourceSpan {
        SourceSpan {
            start_line: 1,
            start_utf8_byte: 0,
            end_line: 1,
            end_utf8_byte: 1,
        }
    }

    fn console() -> Effect {
        Effect::External(ExternalEffect::Console)
    }

    fn raise_value_error() -> Effect {
        Effect::Partial(PartialBehavior::Raise(
            ExceptionId::parse("builtins.ValueError").unwrap(),
        ))
    }

    fn diverge() -> Effect {
        Effect::Partial(PartialBehavior::Diverge)
    }

    #[test]
    fn effect_fixed_point_terminates_for_recursive_call_graphs() {
        let functions = BTreeMap::from([
            (
                "left".to_owned(),
                FunctionEffects {
                    filename: "fixture.py".to_owned(),
                    direct: EffectFormula::new(),
                    direct_origins: BTreeMap::new(),
                    calls: BTreeSet::from([CallEdge::Invoke {
                        target: "right".to_owned(),
                        span: test_span(),
                        propagation: CallEffectPropagation::AllExcept(EffectSet::new()),
                        bindings: BTreeMap::new(),
                    }]),
                },
            ),
            (
                "right".to_owned(),
                FunctionEffects {
                    filename: "fixture.py".to_owned(),
                    direct: EffectFormula::from([console()]),
                    direct_origins: BTreeMap::from([(
                        EffectTerm::Concrete(console()),
                        BTreeSet::from([DirectEffectOrigin {
                            span: test_span(),
                            operation: "Call builtins.print".to_owned(),
                        }]),
                    )]),
                    calls: BTreeSet::from([CallEdge::Invoke {
                        target: "left".to_owned(),
                        span: test_span(),
                        propagation: CallEffectPropagation::AllExcept(EffectSet::new()),
                        bindings: BTreeMap::new(),
                    }]),
                },
            ),
        ]);
        let valid = BTreeSet::from(["left".to_owned(), "right".to_owned()]);

        let inferred = infer_effect_fixed_point(&functions, &valid);

        assert!(inferred["left"].contains_effect(&console()));
        assert_eq!(
            effect_trace("left", &console(), &functions),
            ["left", "right", "console"]
        );
        let details = effect_trace_details("left", &console(), &functions);
        assert_eq!(details.frames.len(), 2);
        assert_eq!(details.frames[0].operation, "Call right");
        assert_eq!(details.frames[1].operation, "Call builtins.print");
    }

    #[test]
    fn recursive_invocation_cycles_receive_divergence_origins() {
        let mut functions = BTreeMap::from([
            (
                "left".to_owned(),
                FunctionEffects {
                    filename: "fixture.py".to_owned(),
                    direct: EffectFormula::new(),
                    direct_origins: BTreeMap::new(),
                    calls: BTreeSet::from([CallEdge::Invoke {
                        target: "right".to_owned(),
                        span: test_span(),
                        propagation: CallEffectPropagation::AllExcept(EffectSet::new()),
                        bindings: BTreeMap::new(),
                    }]),
                },
            ),
            (
                "right".to_owned(),
                FunctionEffects {
                    filename: "fixture.py".to_owned(),
                    direct: EffectFormula::new(),
                    direct_origins: BTreeMap::new(),
                    calls: BTreeSet::from([CallEdge::Invoke {
                        target: "left".to_owned(),
                        span: test_span(),
                        propagation: CallEffectPropagation::AllExcept(EffectSet::new()),
                        bindings: BTreeMap::new(),
                    }]),
                },
            ),
        ]);
        let valid = BTreeSet::from(["left".to_owned(), "right".to_owned()]);

        record_recursive_divergence(&mut functions, &valid, &BTreeSet::new());

        for function in functions.values() {
            assert!(function.direct.contains_effect(&diverge()));
            assert!(
                function
                    .direct_origins
                    .contains_key(&EffectTerm::Concrete(diverge()))
            );
        }
    }

    #[test]
    fn function_references_do_not_form_recursive_invocation_cycles() {
        let mut functions = BTreeMap::from([(
            "callback".to_owned(),
            FunctionEffects {
                filename: "fixture.py".to_owned(),
                direct: EffectFormula::new(),
                direct_origins: BTreeMap::new(),
                calls: BTreeSet::from([CallEdge::Reference {
                    target: "callback".to_owned(),
                    span: test_span(),
                }]),
            },
        )]);
        let valid = BTreeSet::from(["callback".to_owned()]);

        record_recursive_divergence(&mut functions, &valid, &BTreeSet::new());

        assert!(!functions["callback"].direct.contains_effect(&diverge()));
    }

    #[test]
    fn proven_well_founded_self_call_does_not_diverge() {
        let span = test_span();
        let mut functions = BTreeMap::from([(
            "countdown".to_owned(),
            FunctionEffects {
                filename: "fixture.py".to_owned(),
                direct: EffectFormula::new(),
                direct_origins: BTreeMap::new(),
                calls: BTreeSet::from([CallEdge::Invoke {
                    target: "countdown".to_owned(),
                    span,
                    propagation: CallEffectPropagation::AllExcept(EffectSet::new()),
                    bindings: BTreeMap::new(),
                }]),
            },
        )]);
        let valid = BTreeSet::from(["countdown".to_owned()]);
        let proven = BTreeSet::from([WellFoundedSelfCall {
            function: "countdown".to_owned(),
            span,
        }]);

        record_recursive_divergence(&mut functions, &valid, &proven);

        assert!(!functions["countdown"].direct.contains_effect(&diverge()));
    }

    #[test]
    fn an_unproven_self_call_keeps_recursive_divergence() {
        let proven_span = test_span();
        let unproven_span = SourceSpan {
            start_line: 2,
            start_utf8_byte: 0,
            end_line: 2,
            end_utf8_byte: 1,
        };
        let mut functions = BTreeMap::from([(
            "countdown".to_owned(),
            FunctionEffects {
                filename: "fixture.py".to_owned(),
                direct: EffectFormula::new(),
                direct_origins: BTreeMap::new(),
                calls: BTreeSet::from([
                    CallEdge::Invoke {
                        target: "countdown".to_owned(),
                        span: proven_span,
                        propagation: CallEffectPropagation::AllExcept(EffectSet::new()),
                        bindings: BTreeMap::new(),
                    },
                    CallEdge::Invoke {
                        target: "countdown".to_owned(),
                        span: unproven_span,
                        propagation: CallEffectPropagation::AllExcept(EffectSet::new()),
                        bindings: BTreeMap::new(),
                    },
                ]),
            },
        )]);
        let valid = BTreeSet::from(["countdown".to_owned()]);
        let proven = BTreeSet::from([WellFoundedSelfCall {
            function: "countdown".to_owned(),
            span: proven_span,
        }]);

        record_recursive_divergence(&mut functions, &valid, &proven);

        assert!(functions["countdown"].direct.contains_effect(&diverge()));
        assert_eq!(
            functions["countdown"].direct_origins[&EffectTerm::Concrete(diverge())]
                .iter()
                .next()
                .expect("unproven self call records an origin")
                .span,
            unproven_span
        );
    }

    #[test]
    fn call_edges_only_remove_explicitly_handled_effects() {
        let raised = raise_value_error();
        let functions = BTreeMap::from([
            (
                "recover".to_owned(),
                FunctionEffects {
                    filename: "fixture.py".to_owned(),
                    direct: EffectFormula::new(),
                    direct_origins: BTreeMap::new(),
                    calls: BTreeSet::from([CallEdge::Invoke {
                        target: "fail".to_owned(),
                        span: test_span(),
                        propagation: CallEffectPropagation::AllExcept(EffectSet::from([
                            raised.clone()
                        ])),
                        bindings: BTreeMap::new(),
                    }]),
                },
            ),
            (
                "fail".to_owned(),
                FunctionEffects {
                    filename: "fixture.py".to_owned(),
                    direct: EffectFormula::from([console(), raised.clone()]),
                    direct_origins: BTreeMap::new(),
                    calls: BTreeSet::new(),
                },
            ),
        ]);
        let valid = BTreeSet::from(["recover".to_owned(), "fail".to_owned()]);

        let inferred = infer_effect_fixed_point(&functions, &valid);

        assert_eq!(inferred["recover"], EffectFormula::from([console()]));
        assert_eq!(
            effect_trace("recover", &console(), &functions),
            ["recover", "fail", "console"]
        );
        assert_eq!(
            effect_trace("recover", &raised, &functions),
            ["recover", "raise:builtins.ValueError"]
        );
    }

    #[test]
    fn call_edges_can_suppress_partial_behavior() {
        let raised = raise_value_error();
        let functions = BTreeMap::from([
            (
                "cleanup".to_owned(),
                FunctionEffects {
                    filename: "fixture.py".to_owned(),
                    direct: EffectFormula::new(),
                    direct_origins: BTreeMap::new(),
                    calls: BTreeSet::from([CallEdge::Invoke {
                        target: "operation".to_owned(),
                        span: test_span(),
                        propagation: CallEffectPropagation::SuppressConcreteExceptionalBehavior,
                        bindings: BTreeMap::new(),
                    }]),
                },
            ),
            (
                "operation".to_owned(),
                FunctionEffects {
                    filename: "fixture.py".to_owned(),
                    direct: EffectFormula::from([console(), raised.clone()]),
                    direct_origins: BTreeMap::new(),
                    calls: BTreeSet::new(),
                },
            ),
        ]);
        let valid = BTreeSet::from(["cleanup".to_owned(), "operation".to_owned()]);

        let inferred = infer_effect_fixed_point(&functions, &valid);

        assert_eq!(inferred["cleanup"], EffectFormula::from([console()]));
        assert_eq!(
            effect_trace("cleanup", &console(), &functions),
            ["cleanup", "operation", "console"]
        );
        assert_eq!(
            effect_trace("cleanup", &raised, &functions),
            ["cleanup", "raise:builtins.ValueError"]
        );
    }

    #[test]
    fn exceptional_suppression_preserves_divergence() {
        let functions = BTreeMap::from([
            (
                "cleanup".to_owned(),
                FunctionEffects {
                    filename: "fixture.py".to_owned(),
                    direct: EffectFormula::new(),
                    direct_origins: BTreeMap::new(),
                    calls: BTreeSet::from([CallEdge::Invoke {
                        target: "operation".to_owned(),
                        span: test_span(),
                        propagation: CallEffectPropagation::SuppressConcreteExceptionalBehavior,
                        bindings: BTreeMap::new(),
                    }]),
                },
            ),
            (
                "operation".to_owned(),
                FunctionEffects {
                    filename: "fixture.py".to_owned(),
                    direct: EffectFormula::from([diverge(), raise_value_error()]),
                    direct_origins: BTreeMap::new(),
                    calls: BTreeSet::new(),
                },
            ),
        ]);
        let valid = BTreeSet::from(["cleanup".to_owned(), "operation".to_owned()]);

        let inferred = infer_effect_fixed_point(&functions, &valid);

        assert_eq!(inferred["cleanup"], EffectFormula::from([diverge()]));
    }

    #[test]
    fn partial_suppression_preserves_unresolved_effect_variable() {
        let variable = EffectVariable::new("operation", "E");
        let mut variable_effects = EffectFormula::new();
        variable_effects.insert_variable(variable.clone());
        let functions = BTreeMap::from([
            (
                "cleanup".to_owned(),
                FunctionEffects {
                    filename: "fixture.py".to_owned(),
                    direct: EffectFormula::new(),
                    direct_origins: BTreeMap::new(),
                    calls: BTreeSet::from([CallEdge::Invoke {
                        target: "operation".to_owned(),
                        span: test_span(),
                        propagation: CallEffectPropagation::SuppressConcreteExceptionalBehavior,
                        bindings: BTreeMap::new(),
                    }]),
                },
            ),
            (
                "operation".to_owned(),
                FunctionEffects {
                    filename: "fixture.py".to_owned(),
                    direct: variable_effects.clone(),
                    direct_origins: BTreeMap::new(),
                    calls: BTreeSet::new(),
                },
            ),
        ]);
        let valid = BTreeSet::from(["cleanup".to_owned(), "operation".to_owned()]);

        let inferred = infer_effect_fixed_point(&functions, &valid);

        assert_eq!(inferred["cleanup"], variable_effects);
    }

    #[test]
    fn call_edges_instantiate_effect_variables() {
        let variable = EffectVariable::new("apply", "E");
        let mut apply_effects = EffectFormula::new();
        apply_effects.insert_variable(variable.clone());
        let functions = BTreeMap::from([
            (
                "apply".to_owned(),
                FunctionEffects {
                    filename: "fixture.py".to_owned(),
                    direct: apply_effects,
                    direct_origins: BTreeMap::new(),
                    calls: BTreeSet::new(),
                },
            ),
            (
                "run".to_owned(),
                FunctionEffects {
                    filename: "fixture.py".to_owned(),
                    direct: EffectFormula::new(),
                    direct_origins: BTreeMap::new(),
                    calls: BTreeSet::from([CallEdge::Invoke {
                        target: "apply".to_owned(),
                        span: test_span(),
                        propagation: CallEffectPropagation::AllExcept(EffectSet::new()),
                        bindings: BTreeMap::from([(variable, EffectFormula::from([console()]))]),
                    }]),
                },
            ),
        ]);
        let valid = BTreeSet::from(["apply".to_owned(), "run".to_owned()]);

        let inferred = infer_effect_fixed_point(&functions, &valid);

        assert!(inferred["run"].contains_effect(&console()));
    }
}
