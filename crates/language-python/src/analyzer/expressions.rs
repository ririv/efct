use efct_engine::CallEdge;
use efct_model::{Diagnostic, Effect, EffectFormula, EffectTerm, ExceptionId, PartialBehavior};
use efct_protocol::{
    BinaryOperator, BooleanOperator, ComparisonOperator, ConstantValue, SourceSpan, UnaryOperator,
};

use crate::exceptions::resolve_builtin_exception;
use crate::hir::{Expression, FunctionDeclaration};
use crate::types::Type;

use super::typing::{
    StaticInteger, StaticTupleIndex, function_binds_exception_name, function_binds_name,
    homogeneous_tuple_element, is_assignable, static_integer, static_tuple_index,
};
use super::{FunctionAnalyzer, RethrowContext, TypedExpression};

#[derive(Debug, Clone, PartialEq, Eq)]
enum BinaryOperationPartiality {
    Total,
    Raises {
        exception: ExceptionId,
        operation: &'static str,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SubscriptFailure {
    TupleIndex,
    MissingFrozenMapKey,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SubscriptOutcome {
    Total(Type),
    MayRaise {
        value_type: Type,
        failure: SubscriptFailure,
    },
    AlwaysRaises(SubscriptFailure),
}

impl FunctionAnalyzer<'_> {
    pub(super) fn expression(&mut self, expression: &Expression) -> Option<TypedExpression> {
        match expression {
            Expression::Name { identifier, span } => {
                if let Some(value_type) = self.locals.get(identifier) {
                    return Some(TypedExpression::pure(value_type.clone()));
                }
                if function_binds_exception_name(self.function, identifier) {
                    self.error(
                        "P1104",
                        *span,
                        format!(
                            "Exception binding {identifier} is only available inside its handler"
                        ),
                    );
                    return None;
                }
                if matches!(self.rethrow_context, RethrowContext::Finally { .. })
                    && function_binds_name(self.function, identifier)
                {
                    self.error(
                        "P1104",
                        *span,
                        format!(
                            "Local variable {identifier} is not available in finally because it was not defined before try"
                        ),
                    );
                    return None;
                }
                if let Some(value_type) = self.constants.get(identifier) {
                    return Some(TypedExpression::pure(value_type.clone()));
                }
                if let Some(signature) = self.signatures.get(identifier) {
                    self.calls.insert(CallEdge::Reference {
                        target: identifier.clone(),
                        span: *span,
                    });
                    return match &signature.declaration {
                        FunctionDeclaration::BoundedPure(partials) if partials.is_empty() => {
                            Some(TypedExpression::pure(Type::PureCallable {
                                parameters: signature.parameters.clone(),
                                returns: Box::new(signature.returns.clone()),
                            }))
                        }
                        FunctionDeclaration::BoundedPure(_)
                        | FunctionDeclaration::BoundedEffects(_) => {
                            if signature
                                .declared_effects
                                .iter()
                                .any(|term| matches!(term, EffectTerm::Variable(_)))
                            {
                                self.error(
                                    "P1201",
                                    *span,
                                    format!("Passing effect-generic function {identifier} as a callback is not supported yet"),
                                );
                                None
                            } else {
                                Some(TypedExpression::pure(Type::EffectCallable {
                                    parameters: signature.parameters.clone(),
                                    returns: Box::new(signature.returns.clone()),
                                    effects: signature.declared_effects.clone(),
                                }))
                            }
                        }
                        FunctionDeclaration::InferredPure
                        | FunctionDeclaration::InferredEffects => {
                            self.error(
                                "P1201",
                                *span,
                                format!("A function with inferred effects or partial behavior cannot be passed as a first-order callback: {identifier}"),
                            );
                            None
                        }
                    };
                }
                if let Some(exception) = self.exceptions.resolve(identifier) {
                    return Some(TypedExpression::pure(Type::Exception(exception)));
                }
                self.error(
                    "P1004",
                    *span,
                    format!("Value name {identifier} cannot be resolved"),
                );
                None
            }
            Expression::Constant { value, span } => match value {
                ConstantValue::None => Some(TypedExpression::pure(Type::None)),
                ConstantValue::Bool(_) => Some(TypedExpression::pure(Type::Bool)),
                ConstantValue::Int(_) => Some(TypedExpression::pure(Type::Int)),
                ConstantValue::Str(_) => Some(TypedExpression::pure(Type::Str)),
                ConstantValue::Bytes(_) => Some(TypedExpression::pure(Type::Bytes)),
                ConstantValue::Ellipsis | ConstantValue::Unsupported(_) => {
                    self.error(
                        "P1401",
                        *span,
                        "This constant cannot be used in a runtime expression",
                    );
                    None
                }
            },
            Expression::Tuple { elements, .. } => {
                let mut effects = EffectFormula::new();
                let mut types = Vec::with_capacity(elements.len());
                for element in elements {
                    let result = self.expression(element)?;
                    effects.extend(result.effects);
                    if result.value_type == Type::Never {
                        return Some(TypedExpression::never(effects));
                    }
                    if result.value_type.contains_local_mutable() {
                        self.error(
                            "P1202",
                            element.span(),
                            "A local mutable value cannot escape inside a container",
                        );
                        return None;
                    }
                    types.push(result.value_type);
                }
                Some(TypedExpression {
                    value_type: Type::TupleFixed(types),
                    effects,
                })
            }
            Expression::List { span, .. } => {
                self.error(
                    "P1401",
                    *span,
                    "A list literal must be bound directly to a local name",
                );
                None
            }
            Expression::Unary {
                operator,
                operand,
                span,
            } => {
                let operand = self.expression(operand)?;
                if operand.is_never() {
                    return Some(operand);
                }
                let value_type = match (operator, &operand.value_type) {
                    (UnaryOperator::Positive | UnaryOperator::Negative, Type::Int) => Type::Int,
                    (UnaryOperator::Not, Type::Bool) => Type::Bool,
                    _ => {
                        self.error(
                            "P1104",
                            *span,
                            format!(
                                "Type {} does not support this unary operation",
                                operand.value_type
                            ),
                        );
                        return None;
                    }
                };
                Some(TypedExpression {
                    value_type,
                    effects: operand.effects,
                })
            }
            Expression::Binary {
                operator,
                left,
                right,
                span,
            } => {
                let left = self.expression(left)?;
                if left.is_never() {
                    return Some(left);
                }
                let right_result = self.expression(right)?;
                let right_never = right_result.is_never();
                let mut effects = left.effects;
                effects.extend(right_result.effects);
                if right_never {
                    return Some(TypedExpression::never(effects));
                }
                let value_type =
                    self.binary_type(*operator, &left.value_type, &right_result.value_type, *span)?;
                self.record_binary_partiality(&mut effects, *operator, right, *span);
                Some(TypedExpression {
                    value_type,
                    effects,
                })
            }
            Expression::Boolean {
                operator,
                values,
                span,
            } => {
                if !matches!(operator, BooleanOperator::And | BooleanOperator::Or) {
                    self.error("P1401", *span, "Unknown boolean operator");
                    return None;
                }
                let mut effects = EffectFormula::new();
                for (index, value) in values.iter().enumerate() {
                    let result = self.expression(value)?;
                    let result_never = result.is_never();
                    effects.extend(result.effects);
                    if result_never {
                        return Some(if index == 0 {
                            TypedExpression::never(effects)
                        } else {
                            TypedExpression {
                                value_type: Type::Bool,
                                effects,
                            }
                        });
                    }
                    if result.value_type != Type::Bool {
                        self.error(
                            "P1104",
                            value.span(),
                            "Boolean operations require exact bool values",
                        );
                        return None;
                    }
                }
                Some(TypedExpression {
                    value_type: Type::Bool,
                    effects,
                })
            }
            Expression::Compare {
                left,
                operators,
                comparators,
                span,
            } => {
                if operators.len() != comparators.len() || operators.is_empty() {
                    self.error(
                        "P1401",
                        *span,
                        "The comparison expression protocol is invalid",
                    );
                    return None;
                }
                let left = self.expression(left)?;
                if left.is_never() {
                    return Some(left);
                }
                let mut effects = left.effects;
                let mut previous = left.value_type;
                for (index, (operator, comparator)) in operators.iter().zip(comparators).enumerate()
                {
                    let current = self.expression(comparator)?;
                    let current_never = current.is_never();
                    effects.extend(current.effects);
                    if current_never {
                        return Some(if index == 0 {
                            TypedExpression::never(effects)
                        } else {
                            TypedExpression {
                                value_type: Type::Bool,
                                effects,
                            }
                        });
                    }
                    if previous != current.value_type {
                        self.error(
                            "P1104",
                            comparator.span(),
                            "Comparison operand types are incompatible",
                        );
                        return None;
                    }
                    let supported = match operator {
                        ComparisonOperator::Equal | ComparisonOperator::NotEqual => {
                            previous.is_data_value()
                        }
                        ComparisonOperator::Less
                        | ComparisonOperator::LessEqual
                        | ComparisonOperator::Greater
                        | ComparisonOperator::GreaterEqual => {
                            matches!(&previous, Type::Int | Type::Str | Type::Bytes)
                        }
                        ComparisonOperator::Is | ComparisonOperator::IsNot => {
                            previous == Type::None
                        }
                        ComparisonOperator::In
                        | ComparisonOperator::NotIn
                        | ComparisonOperator::Unknown => false,
                    };
                    if !supported {
                        self.error(
                            "P1401",
                            comparator.span(),
                            "This comparison operator is not supported",
                        );
                        return None;
                    }
                    previous = current.value_type;
                }
                Some(TypedExpression {
                    value_type: Type::Bool,
                    effects,
                })
            }
            Expression::Conditional {
                condition,
                then_value,
                else_value,
                span,
            } => {
                let condition = self.expression(condition)?;
                if condition.is_never() {
                    return Some(condition);
                }
                if condition.value_type != Type::Bool {
                    self.error(
                        "P1104",
                        *span,
                        "A conditional expression requires an exact bool condition",
                    );
                    return None;
                }
                let then_value = self.expression(then_value)?;
                let else_value = self.expression(else_value)?;
                let value_type = match (&then_value.value_type, &else_value.value_type) {
                    (Type::Never, Type::Never) => Type::Never,
                    (Type::Never, value_type) | (value_type, Type::Never) => value_type.clone(),
                    (then_type, else_type) if then_type == else_type => then_type.clone(),
                    _ => {
                        self.error(
                            "P1104",
                            *span,
                            "Both branches of a conditional expression must have exactly the same type",
                        );
                        return None;
                    }
                };
                let mut effects = condition.effects;
                effects.extend(then_value.effects);
                effects.extend(else_value.effects);
                Some(TypedExpression {
                    value_type,
                    effects,
                })
            }
            Expression::Call {
                callee,
                arguments,
                span,
            } => self.call(callee, arguments, *span),
            Expression::Attribute { value, name, span } => {
                let receiver = self.expression(value)?;
                if receiver.is_never() {
                    return Some(receiver);
                }
                if let Type::Record { fields, .. } = &receiver.value_type
                    && let Some((_, field_type)) = fields.iter().find(|(field, _)| field == name)
                {
                    return Some(TypedExpression {
                        value_type: field_type.clone(),
                        effects: receiver.effects,
                    });
                }
                let variant_field = match (&receiver.value_type, name.as_str()) {
                    (Type::Ok(value_type), "value") | (Type::Err(value_type), "error") => {
                        Some(value_type.as_ref().clone())
                    }
                    _ => None,
                };
                if let Some(value_type) = variant_field {
                    return Some(TypedExpression {
                        value_type,
                        effects: receiver.effects,
                    });
                }
                self.error(
                    "P1004",
                    *span,
                    "An attribute may only be used as the target of a registered method call",
                );
                None
            }
            Expression::Subscript { value, slice, span } => {
                let value = self.expression(value)?;
                if value.is_never() {
                    return Some(value);
                }
                let index = self.expression(slice)?;
                let index_never = index.is_never();
                let mut effects = value.effects;
                effects.extend(index.effects);
                if index_never {
                    return Some(TypedExpression::never(effects));
                }
                let outcome = match &value.value_type {
                    Type::TupleFixed(elements) => {
                        if index.value_type != Type::Int {
                            self.error("P1104", slice.span(), "A tuple index must be an exact int");
                            return None;
                        }
                        match static_tuple_index(slice, elements.len()) {
                            Some(StaticTupleIndex::InBounds(index)) => {
                                SubscriptOutcome::Total(elements[index].clone())
                            }
                            Some(StaticTupleIndex::OutOfBounds) => {
                                SubscriptOutcome::AlwaysRaises(SubscriptFailure::TupleIndex)
                            }
                            None => match homogeneous_tuple_element(&value.value_type) {
                                Some(element) => SubscriptOutcome::MayRaise {
                                    value_type: element,
                                    failure: SubscriptFailure::TupleIndex,
                                },
                                None if elements.is_empty() => {
                                    SubscriptOutcome::AlwaysRaises(SubscriptFailure::TupleIndex)
                                }
                                None => {
                                    self.error(
                                        "P1104",
                                        *span,
                                        "A dynamic index requires a homogeneous fixed tuple",
                                    );
                                    return None;
                                }
                            },
                        }
                    }
                    Type::TupleVariadic(element) => {
                        if index.value_type != Type::Int {
                            self.error("P1104", slice.span(), "A tuple index must be an exact int");
                            return None;
                        }
                        SubscriptOutcome::MayRaise {
                            value_type: element.as_ref().clone(),
                            failure: SubscriptFailure::TupleIndex,
                        }
                    }
                    Type::FrozenMap(key, item) => {
                        if !is_assignable(key, &index.value_type) {
                            self.error(
                                "P1104",
                                slice.span(),
                                format!(
                                    "FrozenMap key type mismatch: expected {key}, got {}",
                                    index.value_type
                                ),
                            );
                            return None;
                        }
                        SubscriptOutcome::MayRaise {
                            value_type: item.as_ref().clone(),
                            failure: SubscriptFailure::MissingFrozenMapKey,
                        }
                    }
                    other => {
                        self.error(
                            "P1104",
                            *span,
                            format!("Type {other} does not support indexing"),
                        );
                        return None;
                    }
                };
                let value_type = match outcome {
                    SubscriptOutcome::Total(value_type) => value_type,
                    SubscriptOutcome::MayRaise {
                        value_type,
                        failure,
                    } => {
                        self.record_subscript_failure(&mut effects, failure, *span);
                        value_type
                    }
                    SubscriptOutcome::AlwaysRaises(failure) => {
                        self.record_subscript_failure(&mut effects, failure, *span);
                        Type::Never
                    }
                };
                Some(TypedExpression {
                    value_type,
                    effects,
                })
            }
        }
    }

    pub(super) fn binary_type(
        &mut self,
        operator: BinaryOperator,
        left: &Type,
        right: &Type,
        span: SourceSpan,
    ) -> Option<Type> {
        let result = match (operator, left, right) {
            (
                BinaryOperator::Add
                | BinaryOperator::Subtract
                | BinaryOperator::Multiply
                | BinaryOperator::FloorDivide
                | BinaryOperator::Modulo,
                Type::Int,
                Type::Int,
            ) => Type::Int,
            (BinaryOperator::Add, Type::Str, Type::Str) => Type::Str,
            (BinaryOperator::Add, Type::Bytes, Type::Bytes) => Type::Bytes,
            _ => {
                self.error(
                    "P1104",
                    span,
                    format!("Types {left} and {right} do not support this binary operation"),
                );
                return None;
            }
        };
        Some(result)
    }

    pub(super) fn record_binary_partiality(
        &mut self,
        effects: &mut EffectFormula,
        operator: BinaryOperator,
        right: &Expression,
        span: SourceSpan,
    ) {
        let partiality = match operator {
            BinaryOperator::FloorDivide | BinaryOperator::Modulo
                if static_integer(right) != Some(StaticInteger::NonZero) =>
            {
                let operation = match operator {
                    BinaryOperator::FloorDivide => "Integer floor division",
                    BinaryOperator::Modulo => "Integer modulo",
                    _ => unreachable!("the outer match restricts the binary operator"),
                };
                BinaryOperationPartiality::Raises {
                    exception: resolve_builtin_exception("ZeroDivisionError")
                        .expect("ZeroDivisionError is a registered builtin exception"),
                    operation,
                }
            }
            _ => BinaryOperationPartiality::Total,
        };
        match partiality {
            BinaryOperationPartiality::Total => {}
            BinaryOperationPartiality::Raises {
                exception,
                operation,
            } => self.record_effect(
                effects,
                Effect::Partial(PartialBehavior::Raise(exception)),
                span,
                operation,
            ),
        }
    }

    fn record_subscript_failure(
        &mut self,
        effects: &mut EffectFormula,
        failure: SubscriptFailure,
        span: SourceSpan,
    ) {
        let (exception, operation) = match failure {
            SubscriptFailure::TupleIndex => ("IndexError", "Index tuple"),
            SubscriptFailure::MissingFrozenMapKey => ("KeyError", "Index FrozenMap"),
        };
        self.record_effect(
            effects,
            Effect::Partial(PartialBehavior::Raise(
                resolve_builtin_exception(exception)
                    .expect("subscript exceptions are registered builtin exceptions"),
            )),
            span,
            operation,
        );
    }

    pub(super) fn error(
        &mut self,
        code: &'static str,
        span: SourceSpan,
        message: impl Into<String>,
    ) {
        self.diagnostics.push(Diagnostic::error(
            code,
            self.filename.to_owned(),
            Some(span),
            Some(self.function.name.clone()),
            message,
        ));
    }
}
