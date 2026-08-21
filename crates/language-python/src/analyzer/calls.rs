use efct_engine::{CallEdge, CallEffectPropagation};
use efct_model::{Effect, EffectFormula, ExternalEffect, PartialBehavior};
use efct_protocol::SourceSpan;

use crate::exceptions::resolve_builtin_exception;
use crate::external::{TrustLevel, policy_rejects};
use crate::hir::Expression;
use crate::types::Type;

use super::signatures::qualified_name;
use super::typing::{
    EvaluatedArguments, StaticInteger, StaticMapKeys, bind_call_parameters,
    homogeneous_map_entries, homogeneous_tuple_element, is_assignable, static_integer,
    static_map_keys, substitute_type_effects,
};
use super::{FunctionAnalyzer, TypedExpression};

enum ArgumentEvaluation {
    Complete(EvaluatedArguments),
    Never(EffectFormula),
}

impl FunctionAnalyzer<'_> {
    pub(super) fn call(
        &mut self,
        callee: &Expression,
        arguments: &[Expression],
        span: SourceSpan,
    ) -> Option<TypedExpression> {
        if let Expression::Attribute { value, name, .. } = callee {
            if let Some(qualified) = qualified_name(callee)
                && matches!(
                    qualified.as_str(),
                    "efct.Ok" | "efct.Err" | "efct.FrozenMap"
                )
            {
                let evaluated = match self.evaluate_arguments(arguments)? {
                    ArgumentEvaluation::Complete(evaluated) => evaluated,
                    ArgumentEvaluation::Never(effects) => {
                        return Some(TypedExpression::never(effects));
                    }
                };
                return self.builtin_call(&qualified, evaluated, arguments, span);
            }
            if let Some(qualified) = self.resolve_api_attribute(callee) {
                let evaluated = match self.evaluate_arguments(arguments)? {
                    ArgumentEvaluation::Complete(evaluated) => evaluated,
                    ArgumentEvaluation::Never(effects) => {
                        return Some(TypedExpression::never(effects));
                    }
                };
                return self.api_call(&qualified, evaluated, arguments, span);
            }
            let receiver = self.expression(value)?;
            if receiver.is_never() {
                return Some(receiver);
            }
            let mut evaluated = match self.evaluate_arguments(arguments)? {
                ArgumentEvaluation::Complete(evaluated) => evaluated,
                ArgumentEvaluation::Never(mut effects) => {
                    effects.extend(receiver.effects);
                    return Some(TypedExpression::never(effects));
                }
            };
            evaluated.effects.extend(receiver.effects);
            if let Type::LocalList { element, .. } = &receiver.value_type
                && name == "append"
            {
                if evaluated.types.len() != 1
                    || !is_assignable(element, &evaluated.types[0])
                    || evaluated.types[0].contains_local_mutable()
                {
                    self.error(
                        "P1104",
                        span,
                        "The local list append argument has the wrong type",
                    );
                    return None;
                }
                return Some(TypedExpression {
                    value_type: Type::None,
                    effects: evaluated.effects,
                });
            }
            if receiver.value_type == Type::Str
                && matches!(name.as_str(), "strip" | "lower")
                && evaluated.types.is_empty()
            {
                return Some(TypedExpression {
                    value_type: Type::Str,
                    effects: evaluated.effects,
                });
            }
            self.error(
                "P1004",
                span,
                format!("Method {}.{name} is not registered", receiver.value_type),
            );
            return None;
        }
        let Expression::Name { identifier, .. } = callee else {
            let callee_result = self.expression(callee)?;
            if callee_result.is_never() {
                return Some(callee_result);
            }
            self.error(
                "P1004",
                span,
                "A call target must be a static name or a registered method",
            );
            return None;
        };
        if let Some(Type::PureCallable {
            parameters,
            returns,
        }) = self.locals.get(identifier).cloned()
        {
            let evaluated = match self.evaluate_arguments(arguments)? {
                ArgumentEvaluation::Complete(evaluated) => evaluated,
                ArgumentEvaluation::Never(effects) => {
                    return Some(TypedExpression::never(effects));
                }
            };
            if evaluated.contains_local_mutable() {
                self.error(
                    "P1202",
                    span,
                    "A local mutable value cannot escape as a call argument",
                );
                return None;
            }
            if parameters.len() != evaluated.types.len()
                || !parameters
                    .iter()
                    .zip(&evaluated.types)
                    .all(|(expected, actual)| is_assignable(expected, actual))
            {
                self.error(
                    "P1104",
                    span,
                    format!("Arguments passed to PureCallable parameter {identifier} have the wrong types"),
                );
                return None;
            }
            return Some(TypedExpression {
                value_type: *returns,
                effects: evaluated.effects,
            });
        }
        if let Some(Type::EffectCallable {
            parameters,
            returns,
            effects: callback_effects,
        }) = self.locals.get(identifier).cloned()
        {
            let evaluated = match self.evaluate_arguments(arguments)? {
                ArgumentEvaluation::Complete(evaluated) => evaluated,
                ArgumentEvaluation::Never(effects) => {
                    return Some(TypedExpression::never(effects));
                }
            };
            if evaluated.contains_local_mutable() {
                self.error(
                    "P1202",
                    span,
                    "A local mutable value cannot escape as a call argument",
                );
                return None;
            }
            if parameters.len() != evaluated.types.len()
                || !parameters
                    .iter()
                    .zip(&evaluated.types)
                    .all(|(expected, actual)| is_assignable(expected, actual))
            {
                self.error(
                    "P1104",
                    span,
                    format!("Arguments passed to EffectCallable parameter {identifier} have the wrong types"),
                );
                return None;
            }
            let mut effects = evaluated.effects;
            self.record_formula(
                &mut effects,
                &callback_effects,
                span,
                format!("Invoke callback {identifier}"),
            );
            return Some(TypedExpression {
                value_type: *returns,
                effects,
            });
        }
        let non_callable_value =
            self.locals.contains_key(identifier) || self.constants.contains_key(identifier);
        let registered_target = self.records.contains_key(identifier)
            || self.signatures.contains_key(identifier)
            || self.externals.contains_key(identifier)
            || crate::api_model::resolve_name(identifier, self.api_imports).is_some()
            || is_builtin_call_target(identifier)
            || self.exceptions.resolve(identifier).is_some();
        let mut evaluated = match self.evaluate_arguments(arguments)? {
            ArgumentEvaluation::Complete(evaluated) => evaluated,
            ArgumentEvaluation::Never(effects) => {
                if !non_callable_value && !registered_target {
                    self.error(
                        "P1004",
                        span,
                        format!("Call target {identifier} is not registered"),
                    );
                    return None;
                }
                return Some(TypedExpression::never(effects));
            }
        };
        if non_callable_value {
            self.error(
                "P1004",
                span,
                format!("Name {identifier} is shadowed by a non-callable value"),
            );
            return None;
        }
        if identifier == "range" && arguments.len() == 3 {
            match static_integer(&arguments[2]) {
                Some(StaticInteger::NonZero) => {}
                Some(StaticInteger::Zero) => {
                    self.error(
                        "P1104",
                        arguments[2].span(),
                        "The range step cannot be zero",
                    );
                    return None;
                }
                None => {
                    self.error(
                        "P1104",
                        arguments[2].span(),
                        "The MVP requires the range step to be a static non-zero integer",
                    );
                    return None;
                }
            }
        }
        if !matches!(identifier.as_str(), "len" | "sum") && evaluated.contains_local_mutable() {
            self.error(
                "P1202",
                span,
                "A local mutable value cannot escape as a call argument",
            );
            return None;
        }
        if let Some(Type::Record { name, fields }) = self.records.get(identifier) {
            if fields.len() != evaluated.types.len()
                || !fields
                    .iter()
                    .zip(&evaluated.types)
                    .all(|((_, expected), actual)| is_assignable(expected, actual))
            {
                self.error(
                    "P1104",
                    span,
                    format!("Constructor arguments for pure record {name} have the wrong types"),
                );
                return None;
            }
            return Some(TypedExpression {
                value_type: self.records[identifier].clone(),
                effects: evaluated.effects,
            });
        }
        if identifier == "frozenset" && evaluated.types.len() == 1 {
            let element = homogeneous_tuple_element(&evaluated.types[0]);
            let Some(element) = element else {
                self.error(
                    "P1104",
                    span,
                    "frozenset requires a non-empty homogeneous tuple",
                );
                return None;
            };
            return Some(TypedExpression {
                value_type: Type::FrozenSet(Box::new(element)),
                effects: evaluated.effects,
            });
        }
        if let Some(signature) = self.signatures.get(identifier) {
            if signature
                .parameters
                .iter()
                .zip(&evaluated.types)
                .any(|(expected, actual)| {
                    matches!(expected, Type::PureCallable { .. })
                        && matches!(actual, Type::EffectCallable { .. })
                })
            {
                self.error(
                    "P1201",
                    span,
                    "The argument does not satisfy the explicit empty effect and partial contract required by PureCallable",
                );
                return None;
            }
            let Some(bindings) = bind_call_parameters(&signature.parameters, &evaluated.types)
            else {
                self.error(
                    "P1104",
                    span,
                    format!("Arguments passed to function {identifier} have the wrong types"),
                );
                return None;
            };
            self.calls.insert(CallEdge::Invoke {
                target: identifier.clone(),
                span,
                propagation: CallEffectPropagation::AllExcept(self.handled_effects.clone()),
                bindings: bindings.clone(),
            });
            return Some(TypedExpression {
                value_type: substitute_type_effects(&signature.returns, &bindings),
                effects: evaluated.effects,
            });
        }
        if let Some(external) = self.externals.get(identifier).cloned() {
            if external.parameters.len() != evaluated.types.len()
                || !external
                    .parameters
                    .iter()
                    .zip(&evaluated.types)
                    .all(|(expected, actual)| is_assignable(expected, actual))
            {
                self.error(
                    "P1104",
                    span,
                    format!(
                        "Arguments passed to external symbol {identifier} have the wrong types"
                    ),
                );
                return None;
            }
            if policy_rejects(self.policy, &external.trust) {
                let (code, message) = match &external.trust {
                    TrustLevel::Audited(evidence) => (
                        "P1302",
                        format!(
                            "The current policy rejects audited symbol {identifier}; boundary ID: {evidence}"
                        ),
                    ),
                    TrustLevel::Unsafe(reason) => (
                        "P1303",
                        format!("The current policy rejects unsafe symbol {identifier}: {reason}"),
                    ),
                };
                self.error(code, span, message);
                return None;
            }
            let mut external_effects = EffectFormula::new();
            external_effects.extend(external.effects.clone());
            self.record_formula(
                &mut evaluated.effects,
                &external_effects,
                span,
                format!("Call external symbol {identifier}"),
            );
            if matches!(external.trust, TrustLevel::Unsafe(_)) {
                self.record_effect(
                    &mut evaluated.effects,
                    Effect::External(ExternalEffect::Unsafe),
                    span,
                    format!("Call unsafe external symbol {identifier}"),
                );
            }
            return Some(TypedExpression {
                value_type: external.returns.clone(),
                effects: evaluated.effects,
            });
        }
        if let Some(qualified) = crate::api_model::resolve_name(identifier, self.api_imports) {
            return self.api_call(&qualified, evaluated, arguments, span);
        }
        self.builtin_call(identifier, evaluated, arguments, span)
    }

    fn resolve_api_attribute(&self, callee: &Expression) -> Option<String> {
        let lexical = qualified_name(callee)?;
        let root = lexical.split_once('.')?.0;
        if self.locals.contains_key(root)
            || self.constants.contains_key(root)
            || self.signatures.contains_key(root)
            || self.records.contains_key(root)
        {
            return None;
        }
        crate::api_model::resolve_attribute(&lexical, self.api_imports)
    }

    fn api_call(
        &mut self,
        name: &str,
        mut arguments: EvaluatedArguments,
        expressions: &[Expression],
        span: SourceSpan,
    ) -> Option<TypedExpression> {
        if crate::api_model::find(name).is_none() {
            self.error(
                "P1004",
                span,
                format!("Python API operation {name} is not modeled"),
            );
            return None;
        }
        let Some(operation) = crate::api_model::find_matching(name, &arguments.types) else {
            self.error(
                "P1104",
                span,
                format!("Arguments passed to Python API operation {name} have the wrong types"),
            );
            return None;
        };
        let effects = match operation.resolve_effects(expressions) {
            Ok(effects) => effects,
            Err(error) => {
                self.file_mode_error(error, span);
                return None;
            }
        };
        for effect in effects {
            self.record_effect(
                &mut arguments.effects,
                effect.clone(),
                span,
                format!("Call {name}"),
            );
        }
        Some(TypedExpression {
            value_type: operation.returns.to_type(),
            effects: arguments.effects,
        })
    }

    fn builtin_call(
        &mut self,
        name: &str,
        mut arguments: EvaluatedArguments,
        expressions: &[Expression],
        span: SourceSpan,
    ) -> Option<TypedExpression> {
        let value_type = match name {
            "len" if arguments.types.len() == 1 => match &arguments.types[0] {
                Type::TupleFixed(_)
                | Type::TupleVariadic(_)
                | Type::LocalList { .. }
                | Type::Str
                | Type::Bytes => Type::Int,
                other => {
                    self.error("P1104", span, format!("len does not accept type {other}"));
                    return None;
                }
            },
            "sum"
                if matches!(
                    arguments.types.as_slice(),
                    [Type::TupleVariadic(element)] if element.as_ref() == &Type::Int
                ) || matches!(
                    arguments.types.as_slice(),
                    [Type::TupleFixed(elements)] if elements.iter().all(|element| element == &Type::Int)
                ) || matches!(
                    arguments.types.as_slice(),
                    [Type::LocalList { element, .. }] if element.as_ref() == &Type::Int
                ) =>
            {
                Type::Int
            }
            "range"
                if (1..=3).contains(&arguments.types.len())
                    && arguments.types.iter().all(|value| value == &Type::Int) =>
            {
                Type::Range
            }
            "str" if arguments.types.len() == 1 => match &arguments.types[0] {
                Type::Exception(_) | Type::ExceptionGroup(_) | Type::CaughtException(_) => {
                    Type::Str
                }
                other => {
                    self.error("P1104", span, format!("str does not accept type {other}"));
                    return None;
                }
            },
            "print" => {
                if !arguments.types.iter().all(Type::is_data_value) {
                    self.error(
                        "P1104",
                        span,
                        "print arguments must be supported pure values",
                    );
                    return None;
                }
                for effect in crate::api_model::console_effects() {
                    self.record_effect(&mut arguments.effects, effect, span, "Call builtins.print");
                }
                Type::None
            }
            "open" => {
                if !matches!(
                    arguments.types.as_slice(),
                    [Type::Str] | [Type::Str, Type::Str]
                ) {
                    self.error("P1104", span, "open arguments have the wrong types");
                    return None;
                }
                let effects = match crate::api_model::file_open_effects(expressions.get(1)) {
                    Ok(effects) => effects,
                    Err(error) => {
                        self.file_mode_error(error, span);
                        return None;
                    }
                };
                for effect in effects {
                    self.record_effect(&mut arguments.effects, effect, span, "Call builtins.open");
                }
                Type::External("builtins.FileHandle".to_owned())
            }
            "ExceptionGroup" => {
                let [Type::Str, Type::TupleFixed(children)] = arguments.types.as_slice() else {
                    self.error(
                        "P1104",
                        span,
                        "ExceptionGroup requires a str message and a non-empty tuple of registered exceptions",
                    );
                    return None;
                };
                if children.is_empty() {
                    self.error(
                        "P1104",
                        span,
                        "ExceptionGroup requires a str message and a non-empty tuple of registered exceptions",
                    );
                    return None;
                }
                let mut leaves = std::collections::BTreeSet::new();
                for child in children {
                    match child {
                        Type::Exception(exception) => {
                            if self.exceptions.is_exception_group(exception) {
                                self.error(
                                    "P1104",
                                    span,
                                    "ExceptionGroup children must be exception instances, not the ExceptionGroup class",
                                );
                                return None;
                            }
                            leaves.insert(exception.clone());
                        }
                        Type::ExceptionGroup(exceptions) => {
                            leaves.extend(exceptions.clone());
                        }
                        Type::CaughtException(partials) => {
                            leaves.extend(partials.iter().filter_map(|partial| match partial {
                                PartialBehavior::Raise(exception)
                                | PartialBehavior::RaiseGroup(exception) => Some(exception.clone()),
                                PartialBehavior::Throw | PartialBehavior::Diverge => None,
                            }));
                        }
                        _ => {
                            self.error(
                                "P1104",
                                span,
                                "ExceptionGroup children must be registered exception instances or ExceptionGroup values",
                            );
                            return None;
                        }
                    }
                }
                Type::ExceptionGroup(leaves)
            }
            name if let Some(exception) = self.exceptions.resolve(name) => {
                if !self
                    .exceptions
                    .constructor_accepts(&exception, &arguments.types)
                {
                    self.error(
                        "P1104",
                        span,
                        format!(
                            "Exception constructor {exception} requires zero arguments or one str argument"
                        ),
                    );
                    return None;
                }
                Type::Exception(exception)
            }
            "efct.Ok" if arguments.types.len() == 1 => {
                Type::Ok(Box::new(arguments.types[0].clone()))
            }
            "efct.Err" if arguments.types.len() == 1 => {
                Type::Err(Box::new(arguments.types[0].clone()))
            }
            "efct.FrozenMap" if arguments.types.len() == 1 => {
                let Some((key, value)) = homogeneous_map_entries(&arguments.types[0]) else {
                    self.error(
                        "P1104",
                        span,
                        "FrozenMap requires a non-empty tuple of consistently typed key-value pairs",
                    );
                    return None;
                };
                let map_type = Type::FrozenMap(Box::new(key), Box::new(value));
                let entry_count = match &arguments.types[0] {
                    Type::TupleFixed(entries) => entries.len(),
                    _ => unreachable!("homogeneous map entries require a fixed tuple"),
                };
                let keys = if entry_count <= 1 {
                    StaticMapKeys::Unique
                } else {
                    expressions
                        .first()
                        .map_or(StaticMapKeys::Unknown, static_map_keys)
                };
                match keys {
                    StaticMapKeys::Unique => map_type,
                    StaticMapKeys::Duplicate => {
                        self.record_frozen_map_duplicate(&mut arguments.effects, span);
                        Type::Never
                    }
                    StaticMapKeys::Unknown => {
                        self.record_frozen_map_duplicate(&mut arguments.effects, span);
                        map_type
                    }
                }
            }
            _ => {
                self.error(
                    "P1004",
                    span,
                    format!("Call target {name} is not registered"),
                );
                return None;
            }
        };
        Some(TypedExpression {
            value_type,
            effects: arguments.effects,
        })
    }

    fn record_frozen_map_duplicate(&mut self, effects: &mut EffectFormula, span: SourceSpan) {
        self.record_effect(
            effects,
            Effect::Partial(PartialBehavior::Raise(
                resolve_builtin_exception("ValueError")
                    .expect("ValueError is a registered builtin exception"),
            )),
            span,
            "Construct FrozenMap",
        );
    }

    fn file_mode_error(&mut self, error: crate::api_model::FileModeError, span: SourceSpan) {
        let message = match error {
            crate::api_model::FileModeError::Dynamic => {
                "The file mode must be a static string literal"
            }
            crate::api_model::FileModeError::Unsupported => {
                "The file mode is not supported by the Python API model"
            }
        };
        self.error("P1104", span, message);
    }

    fn evaluate_arguments(&mut self, arguments: &[Expression]) -> Option<ArgumentEvaluation> {
        let mut types = Vec::with_capacity(arguments.len());
        let mut effects = EffectFormula::new();
        for argument in arguments {
            let result = self.expression(argument)?;
            let result_never = result.is_never();
            effects.extend(result.effects);
            if result_never {
                return Some(ArgumentEvaluation::Never(effects));
            }
            types.push(result.value_type);
        }
        Some(ArgumentEvaluation::Complete(EvaluatedArguments {
            types,
            effects,
        }))
    }
}

fn is_builtin_call_target(name: &str) -> bool {
    matches!(
        name,
        "len"
            | "sum"
            | "range"
            | "str"
            | "print"
            | "open"
            | "frozenset"
            | "efct.Ok"
            | "efct.Err"
            | "efct.FrozenMap"
    )
}
