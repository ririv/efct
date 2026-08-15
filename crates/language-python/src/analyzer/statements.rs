use efct_engine::{CallEdge, CallEffectPropagation, FunctionEffects};
use efct_model::{
    Diagnostic, Effect, EffectFormula, EffectSet, EffectTerm, ExceptionId, PartialBehavior,
};
use efct_protocol::SourceSpan;

use crate::exceptions::resolve_builtin_exception;
use crate::hir::{
    ExceptionHandler, ExceptionHandlerBinding, ExceptionHandlers, Expression, RaiseCause, Statement,
};
use crate::types::{LocalSource, Type};

use super::signatures::{qualified_name, resolve_type_with_effects};
use super::typing::{
    StaticBoolean, function_binds_name, is_assignable, merge_fallthrough_locals, same_binding_type,
    static_boolean, type_mismatch,
};
use super::{
    BareRaiseOutcome, ControlFlow, ControlFlowSet, FunctionAnalyzer, RethrowContext,
    StatementAnalysis, TypedExpression,
};

impl FunctionAnalyzer<'_> {
    pub(super) fn analyze(&mut self) -> Option<FunctionEffects> {
        for (parameter, parameter_type) in self
            .function
            .parameters
            .iter()
            .zip(&self.signature.parameters)
        {
            self.locals
                .insert(parameter.name.clone(), parameter_type.clone());
        }
        let analysis = self.statements(&self.function.body)?;
        if self.validate_returns
            && self.signature.returns != Type::None
            && analysis.flows.may_fall_through()
        {
            self.diagnostics.push(Diagnostic::error(
                "P1105",
                self.filename.to_owned(),
                Some(self.function.span),
                Some(self.function.name.clone()),
                "A non-None function has a path that does not return a value",
            ));
            return None;
        }
        Some(FunctionEffects {
            filename: self.filename.to_owned(),
            direct: analysis.direct_effects,
            direct_origins: std::mem::take(&mut self.direct_origins),
            calls: std::mem::take(&mut self.calls),
        })
    }

    pub(super) fn statements(&mut self, statements: &[Statement]) -> Option<StatementAnalysis> {
        let mut effects = EffectFormula::new();
        let mut possible_effects = EffectFormula::new();
        let mut flows = super::ControlFlowSet::single(ControlFlow::FallsThrough);
        for statement in statements {
            if !flows.may_fall_through() {
                break;
            }
            let result = self.statement(statement)?;
            effects.extend(result.direct_effects);
            possible_effects.extend(result.possible_effects);
            flows = flows.sequence(&result.flows);
        }
        Some(StatementAnalysis::from_flow_set(
            flows,
            effects,
            possible_effects,
        ))
    }

    fn statement(&mut self, statement: &Statement) -> Option<StatementAnalysis> {
        match statement {
            Statement::ModuleImport { module, span } => {
                let previous_calls = self.calls.clone();
                self.calls.insert(CallEdge::Invoke {
                    target: module.clone(),
                    span: *span,
                    propagation: CallEffectPropagation::AllExcept(self.handled_effects.clone()),
                    bindings: std::collections::BTreeMap::new(),
                });
                let mut possible_effects = EffectFormula::new();
                self.extend_possible_call_effects(&mut possible_effects, &previous_calls);
                Some(StatementAnalysis::from_parts(
                    ControlFlow::FallsThrough,
                    EffectFormula::new(),
                    possible_effects,
                ))
            }
            Statement::Return { value, span } => {
                let result = match value {
                    Some(value) => self.analyze_expression(value)?,
                    None => super::ExpressionAnalysis {
                        value: TypedExpression::pure(Type::None),
                        possible_effects: EffectFormula::new(),
                    },
                };
                if result.value.value_type.contains_local_mutable() {
                    self.error(
                        "P1202",
                        *span,
                        "A local mutable value cannot escape through a return value",
                    );
                    return None;
                }
                if !is_assignable(&self.signature.returns, &result.value.value_type) {
                    self.diagnostics.push(type_mismatch(
                        self.filename,
                        Some(&self.function.name),
                        *span,
                        &self.signature.returns,
                        &result.value.value_type,
                    ));
                    return None;
                }
                let flow = if result.value.is_never() {
                    ControlFlow::ExceptionExit
                } else {
                    ControlFlow::FunctionExit
                };
                Some(StatementAnalysis::from_parts(
                    flow,
                    result.value.effects,
                    result.possible_effects,
                ))
            }
            Statement::Assign {
                target,
                value,
                span,
            } => {
                if let Expression::List { elements, .. } = value {
                    return self.local_list_assignment(target, elements, *span);
                }
                let result = self.analyze_expression(value)?;
                if result.value.is_never() {
                    return Some(StatementAnalysis::from_parts(
                        ControlFlow::ExceptionExit,
                        result.value.effects,
                        result.possible_effects,
                    ));
                }
                if matches!(result.value.value_type, Type::External(_))
                    && self.function.declaration.is_pure()
                    && result.value.effects.iter().next().is_none()
                {
                    self.error(
                        "P1201",
                        *span,
                        "A local value in a pure function cannot use an external object type",
                    );
                    return None;
                }
                self.assign(target, &result.value.value_type, *span)?;
                Some(StatementAnalysis::from_parts(
                    ControlFlow::FallsThrough,
                    result.value.effects,
                    result.possible_effects,
                ))
            }
            Statement::AnnotatedAssignment {
                target,
                annotation,
                value,
                span,
            } => {
                let declared = resolve_type_with_effects(
                    annotation,
                    self.filename,
                    Some(&self.function.name),
                    self.records,
                    self.type_imports,
                    &self.signature.effect_parameters,
                    self.diagnostics,
                )?;
                let mut effects = EffectFormula::new();
                let mut possible_effects = EffectFormula::new();
                if let Some(value) = value {
                    let result = self.analyze_expression(value)?;
                    let result_never = result.value.is_never();
                    effects.extend(result.value.effects);
                    possible_effects.extend(result.possible_effects);
                    if result_never {
                        return Some(StatementAnalysis::from_parts(
                            ControlFlow::ExceptionExit,
                            effects,
                            possible_effects,
                        ));
                    }
                    if !is_assignable(&declared, &result.value.value_type) {
                        self.diagnostics.push(type_mismatch(
                            self.filename,
                            Some(&self.function.name),
                            value.span(),
                            &declared,
                            &result.value.value_type,
                        ));
                        return None;
                    }
                }
                self.assign(target, &declared, *span)?;
                Some(StatementAnalysis::from_parts(
                    ControlFlow::FallsThrough,
                    effects,
                    possible_effects,
                ))
            }
            Statement::AugmentedAssignment {
                target,
                operator,
                value,
                span,
            } => {
                let Expression::Name { identifier, .. } = target else {
                    self.error(
                        "P1401",
                        *span,
                        "An augmented assignment target must be a local name",
                    );
                    return None;
                };
                let Some(left_type) = self.locals.get(identifier).cloned() else {
                    self.error(
                        "P1104",
                        *span,
                        format!("Local variable {identifier} is not defined yet"),
                    );
                    return None;
                };
                let right = self.analyze_expression(value)?;
                if right.value.is_never() {
                    return Some(StatementAnalysis::from_parts(
                        ControlFlow::ExceptionExit,
                        right.value.effects,
                        right.possible_effects,
                    ));
                }
                let result_type =
                    self.binary_type(*operator, &left_type, &right.value.value_type, *span)?;
                if result_type != left_type {
                    self.error(
                        "P1104",
                        *span,
                        "Augmented assignment cannot change a local variable type",
                    );
                    return None;
                }
                let mut effects = right.value.effects;
                let mut possible_effects = right.possible_effects;
                self.record_binary_partiality(&mut effects, *operator, value, *span);
                possible_effects.extend(effects.clone());
                Some(StatementAnalysis::from_parts(
                    ControlFlow::FallsThrough,
                    effects,
                    possible_effects,
                ))
            }
            Statement::Expression { value, .. } => {
                let result = self.analyze_expression(value)?;
                if result.value.is_never() {
                    return Some(StatementAnalysis::from_parts(
                        ControlFlow::ExceptionExit,
                        result.value.effects,
                        result.possible_effects,
                    ));
                }
                if matches!(result.value.value_type, Type::External(_))
                    && self.function.declaration.is_pure()
                    && result.value.effects.iter().next().is_none()
                {
                    self.error(
                        "P1201",
                        value.span(),
                        "A pure function cannot construct an external object type",
                    );
                    return None;
                }
                Some(StatementAnalysis::from_parts(
                    ControlFlow::FallsThrough,
                    result.value.effects,
                    result.possible_effects,
                ))
            }
            Statement::If {
                condition,
                body,
                otherwise,
                span,
            } => self.if_statement(condition, body, otherwise, *span),
            Statement::For {
                target,
                iterable,
                body,
                otherwise,
                span,
            } => self.for_statement(target, iterable, body, otherwise, *span),
            Statement::While {
                condition,
                body,
                otherwise,
                span,
            } => self.while_statement(condition, body, otherwise, *span),
            Statement::Match {
                subject,
                cases,
                span,
            } => self.match_statement(subject, cases, *span),
            Statement::Try {
                body,
                handlers,
                otherwise,
                finalizer,
                span,
            } => self.try_statement(body, handlers, otherwise, finalizer, *span),
            Statement::With { items, body, span } => self.with_statement(items, body, *span),
            Statement::Raise {
                exception,
                cause,
                span,
            } => {
                let Some(exception) = exception else {
                    if !matches!(cause, RaiseCause::Implicit) {
                        self.error("P1401", *span, "A bare raise cannot declare a cause");
                        return None;
                    }
                    return self.bare_raise(*span);
                };
                let result = self.analyze_expression(exception)?;
                if result.value.is_never() {
                    return Some(StatementAnalysis::from_parts(
                        ControlFlow::ExceptionExit,
                        result.value.effects,
                        result.possible_effects,
                    ));
                }
                let raised = match result.value.value_type {
                    Type::Exception(exception) => {
                        std::collections::BTreeSet::from([PartialBehavior::Raise(exception)])
                    }
                    Type::ExceptionGroup(exceptions) => exceptions
                        .into_iter()
                        .map(PartialBehavior::RaiseGroup)
                        .collect(),
                    Type::CaughtException(partials) => partials,
                    _ => {
                        self.error(
                            "P1104",
                            *span,
                            "The raise operand must be a registered exception or ExceptionGroup",
                        );
                        return None;
                    }
                };
                let mut effects = result.value.effects;
                let mut possible_effects = result.possible_effects;
                if let RaiseCause::Explicit(cause) = cause {
                    let result = self.analyze_expression(cause)?;
                    let cause_type = result.value.value_type;
                    effects.extend(result.value.effects);
                    possible_effects.extend(result.possible_effects);
                    if cause_type == Type::Never {
                        return Some(StatementAnalysis::from_parts(
                            ControlFlow::ExceptionExit,
                            effects,
                            possible_effects,
                        ));
                    }
                    if !matches!(
                        cause_type,
                        Type::None
                            | Type::Exception(_)
                            | Type::ExceptionGroup(_)
                            | Type::CaughtException(_)
                    ) {
                        self.error(
                            "P1104",
                            *span,
                            "The raise cause must be a registered exception or None",
                        );
                        return None;
                    }
                }
                for partial in raised {
                    self.record_effect(
                        &mut effects,
                        Effect::Partial(partial.clone()),
                        *span,
                        partial_operation("Raise", &partial),
                    );
                }
                possible_effects.extend(effects.clone());
                Some(StatementAnalysis::from_parts(
                    ControlFlow::ExceptionExit,
                    effects,
                    possible_effects,
                ))
            }
            Statement::Assert {
                condition,
                message,
                span,
            } => self.assert_statement(condition, message.as_ref(), *span),
            Statement::Break(span) => {
                if self.loop_depth == 0 {
                    self.error("P1401", *span, "break may only appear inside a loop");
                    return None;
                }
                Some(StatementAnalysis::from_direct(
                    ControlFlow::Break,
                    EffectFormula::new(),
                ))
            }
            Statement::Continue(span) => {
                if self.loop_depth == 0 {
                    self.error("P1401", *span, "continue may only appear inside a loop");
                    return None;
                }
                Some(StatementAnalysis::from_direct(
                    ControlFlow::Continue,
                    EffectFormula::new(),
                ))
            }
            Statement::Pass(_) => Some(StatementAnalysis::from_direct(
                ControlFlow::FallsThrough,
                EffectFormula::new(),
            )),
        }
    }

    fn bare_raise(&mut self, span: SourceSpan) -> Option<StatementAnalysis> {
        let caught = match self.rethrow_context.clone() {
            RethrowContext::Handler { caught } => caught,
            RethrowContext::OutsideHandler => {
                self.error(
                    "P1401",
                    span,
                    "A bare raise may only appear inside an exception handler",
                );
                return None;
            }
            RethrowContext::Finally { outcomes } => {
                let mut effects = EffectFormula::new();
                for outcome in outcomes {
                    match outcome {
                        BareRaiseOutcome::ReRaise(EffectTerm::Concrete(Effect::Partial(
                            partial,
                        ))) => {
                            self.record_effect(
                                &mut effects,
                                Effect::Partial(partial.clone()),
                                span,
                                partial_operation("Re-raise", &partial),
                            );
                        }
                        BareRaiseOutcome::ReRaise(EffectTerm::Variable(variable)) => {
                            effects.insert_variable(variable);
                        }
                        BareRaiseOutcome::MissingCurrentException(exception) => {
                            self.record_effect(
                                &mut effects,
                                Effect::Partial(PartialBehavior::Raise(exception.clone())),
                                span,
                                format!("Raise {exception} because no exception is active"),
                            );
                        }
                        BareRaiseOutcome::ReRaise(EffectTerm::Concrete(Effect::External(_))) => {
                            unreachable!(
                                "finally rethrow outcomes contain only partial behavior and variables"
                            )
                        }
                    }
                }
                return Some(StatementAnalysis::from_direct(
                    ControlFlow::Rethrow,
                    effects,
                ));
            }
        };
        let mut effects = EffectFormula::new();
        for partial in caught {
            self.record_effect(
                &mut effects,
                Effect::Partial(partial.clone()),
                span,
                partial_operation("Re-raise", &partial),
            );
        }
        Some(StatementAnalysis::from_direct(
            ControlFlow::ExceptionExit,
            effects,
        ))
    }

    fn assert_statement(
        &mut self,
        condition: &Expression,
        message: Option<&Expression>,
        span: SourceSpan,
    ) -> Option<StatementAnalysis> {
        let truth = static_boolean(condition);
        let condition = self.analyze_expression(condition)?;
        if condition.value.is_never() {
            return Some(StatementAnalysis::from_parts(
                ControlFlow::ExceptionExit,
                condition.value.effects,
                condition.possible_effects,
            ));
        }
        if condition.value.value_type != Type::Bool {
            self.error("P1104", span, "An assert condition must be an exact bool");
            return None;
        }

        let mut effects = condition.value.effects;
        let mut possible_effects = condition.possible_effects;
        if truth == StaticBoolean::True {
            if let Some(message) = message {
                let previous_calls = self.calls.clone();
                let previous_origins = self.direct_origins.clone();
                let result = self.analyze_expression(message);
                self.calls = previous_calls;
                self.direct_origins = previous_origins;
                result?;
            }
            return Some(StatementAnalysis::from_parts(
                ControlFlow::FallsThrough,
                effects,
                possible_effects,
            ));
        }

        if let Some(message) = message {
            let result = self.analyze_expression(message)?;
            let message_never = result.value.is_never();
            effects.extend(result.value.effects);
            possible_effects.extend(result.possible_effects);
            if message_never {
                let flows = if truth == StaticBoolean::False {
                    ControlFlowSet::single(ControlFlow::ExceptionExit)
                } else {
                    let mut flows = ControlFlowSet::single(ControlFlow::FallsThrough);
                    flows.insert(ControlFlow::ExceptionExit);
                    flows
                };
                return Some(StatementAnalysis::from_flow_set(
                    flows,
                    effects,
                    possible_effects,
                ));
            }
            if !result.value.value_type.is_data_value() {
                self.error(
                    "P1202",
                    message.span(),
                    "An assert message must be a supported immutable data value",
                );
                return None;
            }
        }

        let assertion_error = resolve_builtin_exception("AssertionError")
            .expect("AssertionError must be present in the closed builtin hierarchy");
        self.record_effect(
            &mut effects,
            Effect::Partial(PartialBehavior::Raise(assertion_error.clone())),
            span,
            "Assert condition",
        );
        possible_effects.extend(effects.clone());
        Some(StatementAnalysis::from_parts(
            if truth == StaticBoolean::False {
                ControlFlow::ExceptionExit
            } else {
                ControlFlow::FallsThrough
            },
            effects,
            possible_effects,
        ))
    }

    pub(super) fn assign(
        &mut self,
        target: &Expression,
        value_type: &Type,
        span: SourceSpan,
    ) -> Option<()> {
        let Expression::Name { identifier, .. } = target else {
            self.error("P1401", span, "An assignment target must be a local name");
            return None;
        };
        if self.constants.contains_key(identifier) || self.signatures.contains_key(identifier) {
            self.error(
                "P1005",
                span,
                format!("Module binding {identifier} cannot be shadowed"),
            );
            return None;
        }
        let assigned_type = match value_type {
            Type::LocalList { element, source } => Type::LocalList {
                element: element.clone(),
                source: source.borrowed_by(identifier.clone()),
            },
            other => other.clone(),
        };
        if let Some(existing) = self.locals.get(identifier) {
            if !same_binding_type(existing, &assigned_type) {
                self.diagnostics.push(type_mismatch(
                    self.filename,
                    Some(&self.function.name),
                    span,
                    existing,
                    &assigned_type,
                ));
                return None;
            }
        }
        self.locals.insert(identifier.clone(), assigned_type);
        Some(())
    }

    fn local_list_assignment(
        &mut self,
        target: &Expression,
        elements: &[Expression],
        span: SourceSpan,
    ) -> Option<StatementAnalysis> {
        let Expression::Name { identifier, .. } = target else {
            self.error(
                "P1401",
                span,
                "A local list must be bound directly to a local name",
            );
            return None;
        };
        if self.constants.contains_key(identifier) || self.signatures.contains_key(identifier) {
            self.error(
                "P1005",
                span,
                format!("Module binding {identifier} cannot be shadowed"),
            );
            return None;
        }
        let Some(first) = elements.first() else {
            self.error(
                "P1104",
                span,
                "The element type of an empty list cannot be inferred",
            );
            return None;
        };
        let first = self.analyze_expression(first)?;
        let mut effects = first.value.effects;
        let mut possible_effects = first.possible_effects;
        if first.value.value_type == Type::Never {
            return Some(StatementAnalysis::from_parts(
                ControlFlow::ExceptionExit,
                effects,
                possible_effects,
            ));
        }
        if !first.value.value_type.is_data_value() {
            self.error(
                "P1104",
                span,
                "Local list elements must be supported pure values",
            );
            return None;
        }
        let element_type = first.value.value_type;
        for element in &elements[1..] {
            let result = self.analyze_expression(element)?;
            effects.extend(result.value.effects);
            possible_effects.extend(result.possible_effects);
            if result.value.value_type == Type::Never {
                return Some(StatementAnalysis::from_parts(
                    ControlFlow::ExceptionExit,
                    effects,
                    possible_effects,
                ));
            }
            if result.value.value_type != element_type {
                self.error(
                    "P1104",
                    element.span(),
                    "All local list elements must have exactly the same type",
                );
                return None;
            }
        }
        let value_type = Type::LocalList {
            element: Box::new(element_type),
            source: LocalSource::Created {
                binding: identifier.clone(),
            },
        };
        if let Some(existing) = self.locals.get(identifier)
            && !same_binding_type(existing, &value_type)
        {
            self.diagnostics.push(type_mismatch(
                self.filename,
                Some(&self.function.name),
                span,
                existing,
                &value_type,
            ));
            return None;
        }
        self.locals.insert(identifier.clone(), value_type);
        Some(StatementAnalysis::from_parts(
            ControlFlow::FallsThrough,
            effects,
            possible_effects,
        ))
    }

    fn if_statement(
        &mut self,
        condition: &Expression,
        body: &[Statement],
        otherwise: &[Statement],
        span: SourceSpan,
    ) -> Option<StatementAnalysis> {
        let truth = static_boolean(condition);
        let condition = self.analyze_expression(condition)?;
        if condition.value.is_never() {
            return Some(StatementAnalysis::from_parts(
                ControlFlow::ExceptionExit,
                condition.value.effects,
                condition.possible_effects,
            ));
        }
        if condition.value.value_type != Type::Bool {
            self.error("P1104", span, "An if condition must be an exact bool");
            return None;
        }
        let initial = self.locals.clone();
        let calls_before_body = self.calls.clone();
        let origins_before_body = self.direct_origins.clone();
        let body_result = self.statements(body)?;
        let body_locals = self.locals.clone();
        let calls_after_body = self.calls.clone();
        let origins_after_body = self.direct_origins.clone();
        if truth == StaticBoolean::False {
            self.calls = calls_before_body;
            self.direct_origins = origins_before_body;
        }
        self.locals = initial.clone();
        let otherwise_result = self.statements(otherwise)?;
        let otherwise_locals = self.locals.clone();
        if truth == StaticBoolean::True {
            self.calls = calls_after_body;
            self.direct_origins = origins_after_body;
        }
        self.locals = match truth {
            StaticBoolean::True if body_result.flows.may_fall_through() => body_locals,
            StaticBoolean::False if otherwise_result.flows.may_fall_through() => otherwise_locals,
            StaticBoolean::True | StaticBoolean::False => initial.clone(),
            StaticBoolean::Unknown => merge_fallthrough_locals(
                &initial,
                &[
                    (body_result.flows.clone(), body_locals),
                    (otherwise_result.flows.clone(), otherwise_locals),
                ],
            ),
        };

        let mut effects = condition.value.effects;
        let mut possible_effects = condition.possible_effects;
        let flows = match truth {
            StaticBoolean::True => {
                effects.extend(body_result.direct_effects);
                possible_effects.extend(body_result.possible_effects);
                body_result.flows
            }
            StaticBoolean::False => {
                effects.extend(otherwise_result.direct_effects);
                possible_effects.extend(otherwise_result.possible_effects);
                otherwise_result.flows
            }
            StaticBoolean::Unknown => {
                effects.extend(body_result.direct_effects);
                effects.extend(otherwise_result.direct_effects);
                possible_effects.extend(body_result.possible_effects);
                possible_effects.extend(otherwise_result.possible_effects);
                let mut flows = body_result.flows;
                flows.union(&otherwise_result.flows);
                flows
            }
        };
        Some(StatementAnalysis::from_flow_set(
            flows,
            effects,
            possible_effects,
        ))
    }

    fn for_statement(
        &mut self,
        target: &Expression,
        iterable: &Expression,
        body: &[Statement],
        otherwise: &[Statement],
        span: SourceSpan,
    ) -> Option<StatementAnalysis> {
        let iterable_result = self.analyze_expression(iterable)?;
        if iterable_result.value.is_never() {
            return Some(StatementAnalysis::from_parts(
                ControlFlow::ExceptionExit,
                iterable_result.value.effects,
                iterable_result.possible_effects,
            ));
        }
        let element_type = match &iterable_result.value.value_type {
            Type::TupleVariadic(element) => element.as_ref().clone(),
            Type::TupleFixed(elements) if !elements.is_empty() => {
                let first = elements[0].clone();
                if elements.iter().all(|element| element == &first) {
                    first
                } else {
                    self.error(
                        "P1104",
                        span,
                        "All iterated elements of a fixed tuple must have the same type",
                    );
                    return None;
                }
            }
            Type::TupleFixed(_) => {
                self.error(
                    "P1104",
                    span,
                    "A loop variable type cannot be inferred from an empty tuple",
                );
                return None;
            }
            Type::Range => Type::Int,
            other => {
                self.error(
                    "P1104",
                    span,
                    format!("Type {other} cannot be iterated in the MVP"),
                );
                return None;
            }
        };
        let initial = self.locals.clone();
        self.assign(target, &element_type, span)?;
        self.loop_depth += 1;
        let body_result = self.statements(body);
        self.loop_depth -= 1;
        let body_result = body_result?;
        self.locals = initial.clone();
        self.loop_depth += 1;
        let otherwise_result = self.statements(otherwise);
        self.loop_depth -= 1;
        let otherwise_result = otherwise_result?;
        self.locals = initial;
        let mut effects = iterable_result.value.effects;
        effects.extend(body_result.direct_effects);
        effects.extend(otherwise_result.direct_effects);
        let mut possible_effects = iterable_result.possible_effects;
        possible_effects.extend(body_result.possible_effects);
        possible_effects.extend(otherwise_result.possible_effects);
        Some(StatementAnalysis::from_parts(
            ControlFlow::FallsThrough,
            effects,
            possible_effects,
        ))
    }

    fn while_statement(
        &mut self,
        condition: &Expression,
        body: &[Statement],
        otherwise: &[Statement],
        span: SourceSpan,
    ) -> Option<StatementAnalysis> {
        let truth = static_boolean(condition);
        let condition = self.analyze_expression(condition)?;
        if condition.value.is_never() {
            return Some(StatementAnalysis::from_parts(
                ControlFlow::ExceptionExit,
                condition.value.effects,
                condition.possible_effects,
            ));
        }
        if condition.value.value_type != Type::Bool {
            self.error("P1104", span, "A while condition must be an exact bool");
            return None;
        }
        let initial = self.locals.clone();
        let calls_before_body = self.calls.clone();
        let origins_before_body = self.direct_origins.clone();
        self.loop_depth += 1;
        let body_result = self.statements(body);
        self.loop_depth -= 1;
        let body_result = body_result?;
        let calls_after_body = self.calls.clone();
        let origins_after_body = self.direct_origins.clone();
        if truth == StaticBoolean::False {
            self.calls = calls_before_body;
            self.direct_origins = origins_before_body;
        }
        self.locals = initial.clone();
        self.loop_depth += 1;
        let otherwise_result = self.statements(otherwise);
        self.loop_depth -= 1;
        let otherwise_result = otherwise_result?;
        if truth == StaticBoolean::True {
            self.calls = calls_after_body;
            self.direct_origins = origins_after_body;
        }
        self.locals = initial;
        let mut effects = condition.value.effects;
        let mut possible_effects = condition.possible_effects;
        if truth != StaticBoolean::False {
            effects.extend(body_result.direct_effects.clone());
            possible_effects.extend(body_result.possible_effects.clone());
        }
        if truth != StaticBoolean::True {
            effects.extend(otherwise_result.direct_effects);
            possible_effects.extend(otherwise_result.possible_effects);
        }

        let mut flows = ControlFlowSet::empty();
        if truth != StaticBoolean::True {
            flows.union(&otherwise_result.flows);
        }
        if truth != StaticBoolean::False {
            for flow in &body_result.flows.0 {
                match flow {
                    ControlFlow::FallsThrough | ControlFlow::Continue => {
                        flows.insert(ControlFlow::Diverges);
                    }
                    ControlFlow::Break => {
                        flows.insert(ControlFlow::FallsThrough);
                    }
                    other => {
                        flows.insert(*other);
                    }
                }
            }
            if body_result.flows.may_continue_loop() {
                self.record_effect(
                    &mut effects,
                    Effect::Partial(PartialBehavior::Diverge),
                    span,
                    "Repeat while loop",
                );
                possible_effects.insert(Effect::Partial(PartialBehavior::Diverge));
            }
        }

        Some(StatementAnalysis::from_flow_set(
            flows,
            effects,
            possible_effects,
        ))
    }

    fn try_statement(
        &mut self,
        body: &[Statement],
        handler_set: &ExceptionHandlers,
        otherwise: &[Statement],
        finalizer: &[Statement],
        span: SourceSpan,
    ) -> Option<StatementAnalysis> {
        let handlers = handler_set.as_slice();
        let group_handlers = matches!(handler_set, ExceptionHandlers::Group(_));
        if handlers.is_empty() && finalizer.is_empty() {
            self.error(
                "P1401",
                span,
                "A try statement must contain an exception handler or finally block",
            );
            return None;
        }

        let calls_before_try = self.calls.clone();

        let mut caught = EffectSet::new();
        let mut handler_catches = Vec::new();
        for handler in handlers {
            let exceptions = self.exception_handler_types(handler)?;
            if group_handlers
                && exceptions
                    .iter()
                    .any(|exception| self.exceptions.is_exception_group(exception))
            {
                self.error(
                    "P1401",
                    handler.span,
                    "An except* handler cannot match ExceptionGroup directly; match its leaf exception types",
                );
                return None;
            }
            let declared_catches: EffectSet = exceptions
                .iter()
                .flat_map(|exception| {
                    let mut effects = self.exceptions.caught_effects(exception);
                    if group_handlers {
                        effects.extend(self.exceptions.caught_group_leaf_effects(exception));
                    }
                    effects
                })
                .collect();
            let effective_catches: EffectSet =
                declared_catches.difference(&caught).cloned().collect();
            if effective_catches.is_empty() {
                self.error(
                    "P1401",
                    handler.span,
                    "The exception handler type is covered by an earlier handler and is unreachable",
                );
                return None;
            }
            caught.extend(effective_catches.clone());
            handler_catches.push(effective_catches);
        }

        let initial = self.locals.clone();
        let outer_handlers = self.handled_effects.clone();
        self.handled_effects.extend(caught.clone());
        let body_result = self.statements(body);
        self.handled_effects = outer_handlers;
        let body_result = body_result?;
        let body_locals = self.locals.clone();

        let mut effects = body_result.direct_effects.without_handled(&caught);
        let mut possible_effects = body_result.possible_effects.without_handled(&caught);
        let mut body_flows = body_result.flows.without_exception_exit();
        if body_result.flows.contains_exception_exit()
            && possible_effects
                .partial_behavior_and_variables()
                .iter()
                .next()
                .is_some()
        {
            body_flows.insert(ControlFlow::ExceptionExit);
        }
        let otherwise_reachable = body_result.flows.may_fall_through();
        self.locals = body_locals.clone();
        let previous_calls = self.calls.clone();
        let previous_origins = self.direct_origins.clone();
        let otherwise_result = self.statements(otherwise)?;
        let otherwise_locals = self.locals.clone();
        let mut flows = if otherwise_reachable {
            body_flows.sequence(&otherwise_result.flows)
        } else {
            body_flows
        };
        let mut branches = if otherwise_reachable {
            effects.extend(otherwise_result.direct_effects);
            possible_effects.extend(otherwise_result.possible_effects);
            vec![(otherwise_result.flows, otherwise_locals)]
        } else {
            self.calls = previous_calls;
            self.direct_origins = previous_origins;
            Vec::new()
        };
        for (handler, handler_catches) in handlers.iter().zip(&handler_catches) {
            let reachable = self.known_effects.is_none()
                || body_result.possible_effects.iter().any(|term| match term {
                    EffectTerm::Concrete(effect) => handler_catches.contains(effect),
                    EffectTerm::Variable(_) => true,
                });
            let caught_by_handler: std::collections::BTreeSet<PartialBehavior> = body_result
                .possible_effects
                .iter()
                .filter_map(|term| match term {
                    EffectTerm::Concrete(effect @ Effect::Partial(partial))
                        if handler_catches.contains(effect) =>
                    {
                        Some(if group_handlers {
                            match partial {
                                PartialBehavior::Raise(exception)
                                | PartialBehavior::RaiseGroup(exception) => {
                                    PartialBehavior::RaiseGroup(exception.clone())
                                }
                                PartialBehavior::Diverge => {
                                    unreachable!("exception handlers cannot catch divergence")
                                }
                            }
                        } else {
                            partial.clone()
                        })
                    }
                    EffectTerm::Concrete(_) | EffectTerm::Variable(_) => None,
                })
                .collect();
            self.locals = initial.clone();
            let previous_calls = self.calls.clone();
            let previous_origins = self.direct_origins.clone();
            self.bind_exception_handler(handler, caught_by_handler.clone())?;
            let outer_rethrow_context = std::mem::replace(
                &mut self.rethrow_context,
                RethrowContext::Handler {
                    caught: caught_by_handler.clone(),
                },
            );
            let result = self.statements(&handler.body);
            if let ExceptionHandlerBinding::Bound(binding) = &handler.binding {
                self.locals.remove(binding);
            }
            self.rethrow_context = outer_rethrow_context;
            let result = result?;
            if group_handlers
                && result
                    .possible_effects
                    .partial_behavior_and_variables()
                    .iter()
                    .any(|term| match term {
                        EffectTerm::Concrete(Effect::Partial(PartialBehavior::Diverge)) => false,
                        EffectTerm::Concrete(Effect::Partial(partial)) => {
                            !caught_by_handler.contains(partial)
                        }
                        EffectTerm::Variable(_) => true,
                        EffectTerm::Concrete(Effect::External(_)) => false,
                    })
            {
                self.error(
                    "P1201",
                    handler.span,
                    "An except* handler may only handle normally or re-raise its matched subgroup; raising new partial behavior is not supported",
                );
                return None;
            }
            if reachable {
                effects.extend(result.direct_effects);
                possible_effects.extend(result.possible_effects);
                flows.union(&result.flows);
                branches.push((result.flows, self.locals.clone()));
            } else {
                self.calls = previous_calls;
                self.direct_origins = previous_origins;
            }
        }

        let normal_locals = merge_fallthrough_locals(&initial, &branches);
        if finalizer.is_empty() {
            self.locals = normal_locals;
            return Some(StatementAnalysis::from_flow_set(
                flows,
                effects,
                possible_effects,
            ));
        }

        let calls_before_finalizer = self.calls.clone();
        let origins_before_finalizer = self.direct_origins.clone();
        self.locals = initial.clone();
        let mut rethrow_outcomes: std::collections::BTreeSet<_> = possible_effects
            .partial_behavior_and_variables()
            .into_iter()
            .filter(|term| {
                !matches!(
                    term,
                    EffectTerm::Concrete(Effect::Partial(PartialBehavior::Diverge))
                )
            })
            .map(BareRaiseOutcome::ReRaise)
            .collect();
        if flows.has_non_exception_path() {
            rethrow_outcomes.extend(self.rethrow_outcomes_without_pending_exception());
        }
        let outer_rethrow_context = std::mem::replace(
            &mut self.rethrow_context,
            RethrowContext::Finally {
                outcomes: rethrow_outcomes,
            },
        );
        let finalizer_result = self.statements(finalizer);
        self.rethrow_context = outer_rethrow_context;
        let finalizer_result = finalizer_result?;
        let finalizer_locals = self.locals.clone();
        if !flows.reaches_finally() {
            self.calls = calls_before_finalizer;
            self.direct_origins = origins_before_finalizer;
            self.locals = initial;
            return Some(StatementAnalysis::from_flow_set(
                flows,
                effects,
                possible_effects,
            ));
        }
        let finalizer_falls_through = finalizer_result.flows.may_fall_through();
        let finalizer_rethrows = finalizer_result.flows.contains_rethrow();
        if !finalizer_falls_through && !finalizer_rethrows {
            if self.validate_returns
                && (effects.contains_variable() || possible_effects.contains_variable())
            {
                self.error(
                    "P1201",
                    span,
                    "A non-fallthrough finally cannot override an unresolved effect variable",
                );
                return None;
            }
            effects = effects.without_concrete_exceptional_behavior();
            possible_effects = possible_effects.without_concrete_exceptional_behavior();
            self.suppress_partial_call_behavior(&calls_before_try, &calls_before_finalizer);
        }
        effects.extend(finalizer_result.direct_effects);
        possible_effects.extend(finalizer_result.possible_effects);
        let combined_flows = flows.followed_by_finally(&finalizer_result.flows);
        self.locals = if flows.may_fall_through() && finalizer_falls_through {
            self.merge_finalizer_locals(normal_locals, finalizer_locals, span)?
        } else {
            initial
        };
        Some(StatementAnalysis::from_flow_set(
            combined_flows,
            effects,
            possible_effects,
        ))
    }

    fn rethrow_outcomes_without_pending_exception(
        &self,
    ) -> std::collections::BTreeSet<BareRaiseOutcome> {
        match &self.rethrow_context {
            RethrowContext::OutsideHandler => {
                let runtime_error = resolve_builtin_exception("RuntimeError")
                    .expect("RuntimeError must be present in the closed builtin hierarchy");
                std::collections::BTreeSet::from([BareRaiseOutcome::MissingCurrentException(
                    runtime_error,
                )])
            }
            RethrowContext::Handler { caught } => caught
                .iter()
                .cloned()
                .map(|partial| {
                    BareRaiseOutcome::ReRaise(EffectTerm::Concrete(Effect::Partial(partial)))
                })
                .collect(),
            RethrowContext::Finally { outcomes } => outcomes.clone(),
        }
    }

    fn suppress_partial_call_behavior(
        &mut self,
        calls_before_try: &std::collections::BTreeSet<CallEdge>,
        calls_before_finalizer: &std::collections::BTreeSet<CallEdge>,
    ) {
        let protected_calls: Vec<_> = calls_before_finalizer
            .difference(calls_before_try)
            .cloned()
            .collect();
        for call in protected_calls {
            let CallEdge::Invoke {
                target,
                span,
                bindings,
                ..
            } = &call
            else {
                continue;
            };
            self.calls.remove(&call);
            self.calls.insert(CallEdge::Invoke {
                target: target.clone(),
                span: *span,
                propagation: CallEffectPropagation::SuppressConcreteExceptionalBehavior,
                bindings: bindings.clone(),
            });
        }
    }

    fn merge_finalizer_locals(
        &mut self,
        normal: std::collections::BTreeMap<String, Type>,
        finalizer: std::collections::BTreeMap<String, Type>,
        span: SourceSpan,
    ) -> Option<std::collections::BTreeMap<String, Type>> {
        let mut merged = normal;
        for (name, finalizer_type) in finalizer {
            if let Some(normal_type) = merged.get(&name)
                && !same_binding_type(normal_type, &finalizer_type)
            {
                self.error(
                    "P1104",
                    span,
                    format!(
                        "Finally assigns local {name} with a type incompatible with the normal path"
                    ),
                );
                return None;
            }
            merged.insert(name, finalizer_type);
        }
        Some(merged)
    }

    fn exception_handler_types(&mut self, handler: &ExceptionHandler) -> Option<Vec<ExceptionId>> {
        let (first, remaining) = handler.selector.parts();
        let mut exceptions = Vec::with_capacity(1 + remaining.len());
        for expression in std::iter::once(first).chain(remaining) {
            let exception = self.exception_handler_type(expression, handler.span)?;
            for previous in &exceptions {
                if previous == &exception {
                    self.error(
                        "P1401",
                        handler.span,
                        format!("Exception handler type {exception} appears more than once"),
                    );
                    return None;
                }
                if self.exceptions.is_subtype(&exception, previous)
                    || self.exceptions.is_subtype(previous, &exception)
                {
                    self.error(
                        "P1401",
                        handler.span,
                        format!(
                            "Exception handler types {previous} and {exception} overlap by inheritance"
                        ),
                    );
                    return None;
                }
            }
            exceptions.push(exception);
        }
        Some(exceptions)
    }

    fn exception_handler_type(
        &mut self,
        expression: &Expression,
        span: SourceSpan,
    ) -> Option<ExceptionId> {
        let Some(name) = qualified_name(expression) else {
            self.error(
                "P1104",
                span,
                "An exception handler must use a registered exception type name",
            );
            return None;
        };
        if !self.constants.contains_key(&name)
            && !self.signatures.contains_key(&name)
            && !self.records.contains_key(&name)
            && !self.externals.contains_key(&name)
            && !function_binds_name(self.function, &name)
            && let Some(exception) = self.exceptions.resolve(&name)
        {
            return Some(exception);
        }
        self.error(
            "P1104",
            span,
            format!("Exception handler type {name} is unregistered or shadowed"),
        );
        None
    }

    fn bind_exception_handler(
        &mut self,
        handler: &ExceptionHandler,
        caught: std::collections::BTreeSet<PartialBehavior>,
    ) -> Option<()> {
        let ExceptionHandlerBinding::Bound(binding) = &handler.binding else {
            return Some(());
        };
        if self.constants.contains_key(binding) || self.signatures.contains_key(binding) {
            self.error(
                "P1005",
                handler.span,
                format!("Module binding {binding} cannot be shadowed"),
            );
            return None;
        }
        self.locals
            .insert(binding.clone(), Type::CaughtException(caught));
        Some(())
    }
}

fn partial_operation(action: &str, partial: &PartialBehavior) -> String {
    match partial {
        PartialBehavior::Raise(exception) => format!("{action} {exception}"),
        PartialBehavior::RaiseGroup(exception) => {
            format!("{action} exception group leaf {exception}")
        }
        PartialBehavior::Diverge => format!("{action} divergence"),
    }
}
