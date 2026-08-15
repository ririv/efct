use efct_model::{EffectFormula, EffectSet, EffectTerm};
use efct_protocol::SourceSpan;

use crate::hir::{Expression, Statement, WithItem};
use crate::types::Type;

use super::signatures::qualified_name;
use super::typing::merge_fallthrough_locals;
use super::{ControlFlow, ControlFlowSet, FunctionAnalyzer, StatementAnalysis};

impl FunctionAnalyzer<'_> {
    pub(super) fn with_statement(
        &mut self,
        items: &[WithItem],
        body: &[Statement],
        _span: SourceSpan,
    ) -> Option<StatementAnalysis> {
        let mut caught = EffectSet::new();
        let mut manager_effects = EffectFormula::new();
        let mut manager_possible_effects = EffectFormula::new();
        let mut exception_types = Vec::new();

        for item in items {
            let (context, target) = match item {
                WithItem::Unbound { context } => (context, None),
                WithItem::Bound { context, target } => (context, Some(target)),
            };
            let Expression::Call {
                callee, arguments, ..
            } = context
            else {
                self.error(
                    "P1401",
                    context.span(),
                    "Only contextlib.suppress is supported as a context manager",
                );
                return None;
            };
            if !self.is_contextlib_suppress(callee) {
                self.error(
                    "P1401",
                    context.span(),
                    "Only contextlib.suppress is supported as a context manager",
                );
                return None;
            }
            if arguments.is_empty() {
                self.error(
                    "P1104",
                    context.span(),
                    "contextlib.suppress requires at least one registered exception type",
                );
                return None;
            }
            for argument in arguments {
                let outer_handlers = self.handled_effects.clone();
                self.handled_effects.extend(caught.clone());
                let analyzed = self.analyze_expression(argument);
                self.handled_effects = outer_handlers;
                let analyzed = analyzed?;
                let is_never = analyzed.value.is_never();
                let value_type = analyzed.value.value_type;
                let argument_effects = analyzed.value.effects;
                let argument_possible_effects = analyzed.possible_effects;
                if is_never {
                    let suppressed_reachable = argument_possible_effects
                        .iter()
                        .any(|term| matches!(term, EffectTerm::Concrete(effect) if caught.contains(effect)));
                    manager_effects.extend(argument_effects.without_handled(&caught));
                    manager_possible_effects
                        .extend(argument_possible_effects.without_handled(&caught));
                    let mut flows = ControlFlowSet::empty();
                    if suppressed_reachable {
                        flows.insert(ControlFlow::FallsThrough);
                    }
                    if manager_possible_effects
                        .partial_behavior_and_variables()
                        .iter()
                        .next()
                        .is_some()
                    {
                        flows.insert(ControlFlow::ExceptionExit);
                    }
                    return Some(StatementAnalysis::from_flow_set(
                        flows,
                        manager_effects,
                        manager_possible_effects,
                    ));
                }
                manager_effects.extend(argument_effects.without_handled(&caught));
                manager_possible_effects.extend(argument_possible_effects.without_handled(&caught));
                if qualified_name(argument).is_none() {
                    self.error(
                        "P1104",
                        argument.span(),
                        "contextlib.suppress arguments must be registered exception type names",
                    );
                    return None;
                }
                let Type::Exception(exception) = value_type else {
                    self.error(
                        "P1104",
                        argument.span(),
                        "contextlib.suppress arguments must be registered exception type names",
                    );
                    return None;
                };
                if exception_types.iter().any(|existing| {
                    self.exceptions.is_subtype(&exception, existing)
                        || self.exceptions.is_subtype(existing, &exception)
                }) {
                    self.error(
                        "P1104",
                        argument.span(),
                        "contextlib.suppress exception types must not overlap",
                    );
                    return None;
                }
                caught.extend(self.exceptions.caught_effects(&exception));
                exception_types.push(exception);
            }
            if let Some(target) = target {
                self.assign(target, &Type::None, context.span())?;
            }
        }

        let initial = self.locals.clone();
        let outer_handlers = self.handled_effects.clone();
        self.handled_effects.extend(caught.clone());
        let body_result = self.statements(body);
        self.handled_effects = outer_handlers;
        let body_result = body_result?;
        let body_locals = self.locals.clone();

        let suppressed_reachable = self.known_effects.is_none()
            || body_result.possible_effects.iter().any(|term| match term {
                EffectTerm::Concrete(effect) => caught.contains(effect),
                EffectTerm::Variable(_) => true,
            });
        let mut effects = manager_effects;
        effects.extend(body_result.direct_effects.without_handled(&caught));
        let mut possible_effects = manager_possible_effects;
        possible_effects.extend(body_result.possible_effects.without_handled(&caught));
        let mut flows = body_result.flows.without_exception_exit();
        if body_result.flows.contains_exception_exit()
            && possible_effects
                .partial_behavior_and_variables()
                .iter()
                .next()
                .is_some()
        {
            flows.insert(ControlFlow::ExceptionExit);
        }
        let mut branches = Vec::new();
        if body_result.flows.may_fall_through() {
            branches.push((body_result.flows.clone(), body_locals));
        }
        if suppressed_reachable {
            flows.insert(ControlFlow::FallsThrough);
            branches.push((
                ControlFlowSet::single(ControlFlow::FallsThrough),
                initial.clone(),
            ));
        }
        self.locals = merge_fallthrough_locals(&initial, &branches);
        Some(StatementAnalysis::from_flow_set(
            flows,
            effects,
            possible_effects,
        ))
    }

    fn is_contextlib_suppress(&self, callee: &Expression) -> bool {
        let Some(name) = qualified_name(callee) else {
            return false;
        };
        let root = name.split_once('.').map_or(name.as_str(), |(root, _)| root);
        if self.locals.contains_key(root)
            || self.constants.contains_key(root)
            || self.signatures.contains_key(root)
            || self.records.contains_key(root)
        {
            return false;
        }
        if crate::api_model::is_contextlib_suppress(&name) {
            return true;
        }
        if name.contains('.') {
            crate::api_model::resolve_attribute(&name, self.api_imports)
                .is_some_and(|resolved| crate::api_model::is_contextlib_suppress(&resolved))
        } else {
            crate::api_model::resolve_name(&name, self.api_imports)
                .is_some_and(|resolved| crate::api_model::is_contextlib_suppress(&resolved))
        }
    }
}
