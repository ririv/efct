use efct_protocol::SourceSpan;

use crate::hir::{Expression, MatchCase, Pattern};
use crate::types::Type;

use super::signatures::qualified_name;
use super::typing::{function_binds_name, merge_fallthrough_locals};
use super::{ControlFlow, FunctionAnalyzer, StatementAnalysis};

impl FunctionAnalyzer<'_> {
    pub(super) fn match_statement(
        &mut self,
        subject: &Expression,
        cases: &[MatchCase],
        span: SourceSpan,
    ) -> Option<StatementAnalysis> {
        let subject_result = self.analyze_expression(subject)?;
        if subject_result.value.is_never() {
            return Some(StatementAnalysis::from_parts(
                ControlFlow::ExceptionExit,
                subject_result.value.effects,
                subject_result.possible_effects,
            ));
        }
        let Type::Result(value_type, error_type) = &subject_result.value.value_type else {
            self.error(
                "P1104",
                span,
                format!(
                    "A supported match subject must be Result, not {}",
                    subject_result.value.value_type
                ),
            );
            return None;
        };
        if function_binds_name(self.function, "efct") {
            self.error(
                "P1005",
                span,
                "The efct module binding cannot be shadowed in a Result match",
            );
            return None;
        }

        let mut ok_seen = false;
        let mut err_seen = false;
        let mut classified = Vec::with_capacity(cases.len());
        for case in cases {
            let (variant, binding) = self.result_pattern(&case.pattern)?;
            let duplicate = match variant {
                ResultVariant::Ok => std::mem::replace(&mut ok_seen, true),
                ResultVariant::Err => std::mem::replace(&mut err_seen, true),
            };
            if duplicate {
                self.error(
                    "P1401",
                    case.span,
                    format!(
                        "The {} Result pattern is duplicated and unreachable",
                        variant.name()
                    ),
                );
                return None;
            }
            classified.push((case, variant, binding));
        }
        if !ok_seen || !err_seen {
            let missing = if !ok_seen { "Ok" } else { "Err" };
            self.error(
                "P1401",
                span,
                format!("Result match is not exhaustive; missing {missing}"),
            );
            return None;
        }

        let initial = self.locals.clone();
        let subject_binding = match subject {
            Expression::Name { identifier, .. } => Some(identifier.as_str()),
            _ => None,
        };
        let mut effects = subject_result.value.effects;
        let mut possible_effects = subject_result.possible_effects;
        let mut branches = Vec::with_capacity(classified.len());
        for (case, variant, binding) in classified {
            self.locals = initial.clone();
            let payload_type = match variant {
                ResultVariant::Ok => value_type.as_ref(),
                ResultVariant::Err => error_type.as_ref(),
            };
            if let Some(subject_binding) = subject_binding {
                self.locals.insert(
                    subject_binding.to_owned(),
                    match variant {
                        ResultVariant::Ok => Type::Ok(Box::new(payload_type.clone())),
                        ResultVariant::Err => Type::Err(Box::new(payload_type.clone())),
                    },
                );
            }
            if let Some((binding, binding_span)) = binding {
                self.assign_name(binding, payload_type, binding_span)?;
            }
            let result = self.statements(&case.body)?;
            effects.extend(result.direct_effects);
            possible_effects.extend(result.possible_effects);
            branches.push((result.flows, self.locals.clone()));
        }

        let mut branch_flows = branches.iter().map(|(flows, _)| flows);
        let mut flows = branch_flows
            .next()
            .cloned()
            .expect("an exhaustive Result match contains cases");
        for branch_flows in branch_flows {
            flows.union(branch_flows);
        }
        self.locals = merge_fallthrough_locals(&initial, &branches);
        if let Some(subject_binding) = subject_binding {
            self.locals
                .insert(subject_binding.to_owned(), subject_result.value.value_type);
        }
        Some(StatementAnalysis::from_flow_set(
            flows,
            effects,
            possible_effects,
        ))
    }

    fn result_pattern<'pattern>(
        &mut self,
        pattern: &'pattern Pattern,
    ) -> Option<(ResultVariant, Option<(&'pattern str, SourceSpan)>)> {
        let Pattern::Class {
            class,
            positional,
            span,
        } = pattern
        else {
            self.error(
                "P1401",
                pattern.span(),
                "A Result match must use explicit efct.Ok and efct.Err class patterns",
            );
            return None;
        };
        let Some(class_name) = qualified_name(class) else {
            self.error(
                "P1401",
                *span,
                "A Result pattern class must be a static qualified name",
            );
            return None;
        };
        let variant = match class_name.as_str() {
            "efct.Ok" => ResultVariant::Ok,
            "efct.Err" => ResultVariant::Err,
            _ => {
                self.error(
                    "P1401",
                    *span,
                    format!("Class pattern {class_name} is not supported in a Result match"),
                );
                return None;
            }
        };
        let binding = match positional.as_slice() {
            [] => None,
            [Pattern::Capture { name, span }] => Some((name.as_str(), *span)),
            [Pattern::Wildcard { .. }] => None,
            [_] => {
                self.error(
                    "P1401",
                    *span,
                    "A Result payload pattern must be a name or wildcard",
                );
                return None;
            }
            _ => {
                self.error(
                    "P1401",
                    *span,
                    "A Result variant pattern accepts at most one positional payload",
                );
                return None;
            }
        };
        Some((variant, binding))
    }

    fn assign_name(&mut self, name: &str, value_type: &Type, span: SourceSpan) -> Option<()> {
        let target = Expression::Name {
            identifier: name.to_owned(),
            span,
        };
        self.assign(&target, value_type, span)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResultVariant {
    Ok,
    Err,
}

impl ResultVariant {
    const fn name(self) -> &'static str {
        match self {
            Self::Ok => "Ok",
            Self::Err => "Err",
        }
    }
}
