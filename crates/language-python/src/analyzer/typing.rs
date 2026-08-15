use std::collections::BTreeMap;

use efct_model::{Diagnostic, EffectFormula, EffectTerm, EffectVariable};
use efct_protocol::{ConstantValue, SourceSpan, UnaryOperator};

use crate::hir::{ExceptionHandlerBinding, Expression, Function, Pattern, Statement};
use crate::types::Type;

use super::ControlFlowSet;

pub(super) struct EvaluatedArguments {
    pub(super) types: Vec<Type>,
    pub(super) effects: EffectFormula,
}

impl EvaluatedArguments {
    pub(super) fn contains_local_mutable(&self) -> bool {
        self.types.iter().any(Type::contains_local_mutable)
    }
}

pub(super) fn same_binding_type(left: &Type, right: &Type) -> bool {
    match (left, right) {
        (Type::LocalList { element: left, .. }, Type::LocalList { element: right, .. }) => {
            left == right
        }
        _ => left == right,
    }
}

pub(super) fn merge_fallthrough_locals(
    initial: &BTreeMap<String, Type>,
    branches: &[(ControlFlowSet, BTreeMap<String, Type>)],
) -> BTreeMap<String, Type> {
    let mut fallthrough = branches
        .iter()
        .filter(|(flows, _)| flows.may_fall_through())
        .map(|(_, locals)| locals);
    let Some(first) = fallthrough.next() else {
        return initial.clone();
    };
    let mut merged = first.clone();
    for locals in fallthrough {
        merged.retain(|name, value_type| locals.get(name) == Some(value_type));
    }
    merged
}

pub(super) fn function_binds_name(function: &Function, name: &str) -> bool {
    function
        .parameters
        .iter()
        .any(|parameter| parameter.name == name)
        || statements_bind_name(&function.body, name)
}

pub(super) fn function_binds_exception_name(function: &Function, name: &str) -> bool {
    statements_bind_exception_name(&function.body, name)
}

fn statements_bind_exception_name(statements: &[Statement], name: &str) -> bool {
    statements
        .iter()
        .any(|statement| statement_binds_exception_name(statement, name))
}

fn statement_binds_exception_name(statement: &Statement, name: &str) -> bool {
    match statement {
        Statement::If {
            body, otherwise, ..
        }
        | Statement::For {
            body, otherwise, ..
        }
        | Statement::While {
            body, otherwise, ..
        } => {
            statements_bind_exception_name(body, name)
                || statements_bind_exception_name(otherwise, name)
        }
        Statement::Match { cases, .. } => cases
            .iter()
            .any(|case| statements_bind_exception_name(&case.body, name)),
        Statement::Try {
            body,
            handlers,
            otherwise,
            finalizer,
            ..
        } => {
            statements_bind_exception_name(body, name)
                || handlers.as_slice().iter().any(|handler| {
                    matches!(
                        &handler.binding,
                        ExceptionHandlerBinding::Bound(binding) if binding == name
                    ) || statements_bind_exception_name(&handler.body, name)
                })
                || statements_bind_exception_name(otherwise, name)
                || statements_bind_exception_name(finalizer, name)
        }
        Statement::With { body, .. } => statements_bind_exception_name(body, name),
        Statement::ModuleImport { .. }
        | Statement::Return { .. }
        | Statement::Assign { .. }
        | Statement::AnnotatedAssignment { .. }
        | Statement::AugmentedAssignment { .. }
        | Statement::Expression { .. }
        | Statement::Raise { .. }
        | Statement::Assert { .. }
        | Statement::Break(_)
        | Statement::Continue(_)
        | Statement::Pass(_) => false,
    }
}

pub(super) fn statements_bind_name(statements: &[Statement], name: &str) -> bool {
    statements
        .iter()
        .any(|statement| statement_binds_name(statement, name))
}

pub(super) fn statement_binds_name(statement: &Statement, name: &str) -> bool {
    match statement {
        Statement::Assign { target, .. }
        | Statement::AnnotatedAssignment { target, .. }
        | Statement::AugmentedAssignment { target, .. } => {
            matches!(target, Expression::Name { identifier, .. } if identifier == name)
        }
        Statement::For {
            target,
            body,
            otherwise,
            ..
        } => {
            matches!(target, Expression::Name { identifier, .. } if identifier == name)
                || statements_bind_name(body, name)
                || statements_bind_name(otherwise, name)
        }
        Statement::If {
            body, otherwise, ..
        }
        | Statement::While {
            body, otherwise, ..
        } => statements_bind_name(body, name) || statements_bind_name(otherwise, name),
        Statement::Match { cases, .. } => cases.iter().any(|case| {
            pattern_binds_name(&case.pattern, name) || statements_bind_name(&case.body, name)
        }),
        Statement::Try {
            body,
            handlers,
            otherwise,
            finalizer,
            ..
        } => {
            statements_bind_name(body, name)
                || handlers.as_slice().iter().any(|handler| {
                    matches!(
                        &handler.binding,
                        ExceptionHandlerBinding::Bound(binding) if binding == name
                    ) || statements_bind_name(&handler.body, name)
                })
                || statements_bind_name(otherwise, name)
                || statements_bind_name(finalizer, name)
        }
        Statement::With { items, body, .. } => {
            items.iter().any(|item| {
                matches!(
                    item,
                    crate::hir::WithItem::Bound {
                        target: Expression::Name { identifier, .. },
                        ..
                    } if identifier == name
                )
            }) || statements_bind_name(body, name)
        }
        Statement::ModuleImport { .. }
        | Statement::Return { .. }
        | Statement::Expression { .. }
        | Statement::Raise { .. }
        | Statement::Assert { .. }
        | Statement::Break(_)
        | Statement::Continue(_)
        | Statement::Pass(_) => false,
    }
}

fn pattern_binds_name(pattern: &Pattern, name: &str) -> bool {
    match pattern {
        Pattern::Class { positional, .. } => positional
            .iter()
            .any(|pattern| pattern_binds_name(pattern, name)),
        Pattern::Capture { name: binding, .. } => binding == name,
        Pattern::Wildcard { .. } => false,
    }
}

pub(super) fn is_assignable(expected: &Type, actual: &Type) -> bool {
    if expected == actual {
        return true;
    }
    if actual == &Type::Never {
        return true;
    }
    match (expected, actual) {
        (Type::Option(_), Type::None) => true,
        (Type::Option(expected), Type::Option(actual)) => is_assignable(expected, actual),
        (Type::Option(expected), actual) => is_assignable(expected, actual),
        (Type::Result(expected, _), Type::Ok(actual)) => is_assignable(expected, actual),
        (Type::Result(_, expected), Type::Err(actual)) => is_assignable(expected, actual),
        (Type::TupleVariadic(expected), Type::TupleFixed(actual)) => actual
            .iter()
            .all(|element| is_assignable(expected, element)),
        (Type::TupleVariadic(expected), Type::TupleVariadic(actual)) => {
            is_assignable(expected, actual)
        }
        (Type::TupleFixed(expected), Type::TupleFixed(actual))
            if expected.len() == actual.len() =>
        {
            expected
                .iter()
                .zip(actual)
                .all(|(expected, actual)| is_assignable(expected, actual))
        }
        _ => false,
    }
}

pub(super) fn bind_call_parameters(
    expected: &[Type],
    actual: &[Type],
) -> Option<BTreeMap<EffectVariable, EffectFormula>> {
    if expected.len() != actual.len() {
        return None;
    }
    let mut bindings = BTreeMap::new();
    for (expected, actual) in expected.iter().zip(actual) {
        if !bind_call_type(expected, actual, &mut bindings) {
            return None;
        }
    }
    Some(bindings)
}

pub(super) fn bind_call_type(
    expected: &Type,
    actual: &Type,
    bindings: &mut BTreeMap<EffectVariable, EffectFormula>,
) -> bool {
    let Type::EffectCallable {
        parameters: expected_parameters,
        returns: expected_returns,
        effects: expected_effects,
    } = expected
    else {
        return is_assignable(expected, actual);
    };
    let actual_effects = match actual {
        Type::PureCallable {
            parameters,
            returns,
        } if parameters == expected_parameters && returns == expected_returns => {
            EffectFormula::new()
        }
        Type::EffectCallable {
            parameters,
            returns,
            effects,
        } if parameters == expected_parameters && returns == expected_returns => effects.clone(),
        _ => return false,
    };
    let mut terms = expected_effects.iter();
    let Some(EffectTerm::Variable(variable)) = terms.next() else {
        return false;
    };
    if terms.next().is_some() {
        return false;
    }
    match bindings.get(variable) {
        Some(bound) => bound == &actual_effects,
        None => {
            bindings.insert(variable.clone(), actual_effects);
            true
        }
    }
}

pub(super) fn substitute_type_effects(
    value_type: &Type,
    bindings: &BTreeMap<EffectVariable, EffectFormula>,
) -> Type {
    match value_type {
        Type::EffectCallable {
            parameters,
            returns,
            effects,
        } => Type::EffectCallable {
            parameters: parameters.clone(),
            returns: returns.clone(),
            effects: effects.substitute(bindings),
        },
        _ => value_type.clone(),
    }
}

pub(super) fn homogeneous_tuple_element(value: &Type) -> Option<Type> {
    let Type::TupleFixed(elements) = value else {
        return None;
    };
    let first = elements.first()?.clone();
    elements
        .iter()
        .all(|element| element == &first)
        .then_some(first)
}

pub(super) fn homogeneous_map_entries(value: &Type) -> Option<(Type, Type)> {
    let Type::TupleFixed(entries) = value else {
        return None;
    };
    let Type::TupleFixed(first) = entries.first()? else {
        return None;
    };
    if first.len() != 2 {
        return None;
    }
    let key = first[0].clone();
    let value = first[1].clone();
    entries
        .iter()
        .all(|entry| matches!(entry, Type::TupleFixed(pair) if pair.as_slice() == [key.clone(), value.clone()]))
        .then_some((key, value))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StaticMapKeys {
    Unique,
    Duplicate,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StaticBoolean {
    True,
    False,
    Unknown,
}

pub(super) fn static_boolean(expression: &Expression) -> StaticBoolean {
    match expression {
        Expression::Constant {
            value: ConstantValue::Bool(true),
            ..
        } => StaticBoolean::True,
        Expression::Constant {
            value: ConstantValue::Bool(false),
            ..
        } => StaticBoolean::False,
        Expression::Unary {
            operator: UnaryOperator::Not,
            operand,
            ..
        } => match static_boolean(operand) {
            StaticBoolean::True => StaticBoolean::False,
            StaticBoolean::False => StaticBoolean::True,
            StaticBoolean::Unknown => StaticBoolean::Unknown,
        },
        _ => StaticBoolean::Unknown,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StaticEquality {
    Equal,
    Distinct,
    Unknown,
}

pub(super) fn static_map_keys(expression: &Expression) -> StaticMapKeys {
    let Expression::Tuple {
        elements: entries, ..
    } = expression
    else {
        return StaticMapKeys::Unknown;
    };
    let Some(keys) = entries
        .iter()
        .map(|entry| match entry {
            Expression::Tuple { elements, .. } if elements.len() == 2 => elements.first(),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()
    else {
        return StaticMapKeys::Unknown;
    };
    let mut result = StaticMapKeys::Unique;
    for (index, left) in keys.iter().enumerate() {
        for right in &keys[index + 1..] {
            match static_equality(left, right) {
                StaticEquality::Equal => return StaticMapKeys::Duplicate,
                StaticEquality::Distinct => {}
                StaticEquality::Unknown => result = StaticMapKeys::Unknown,
            }
        }
    }
    result
}

fn static_equality(left: &Expression, right: &Expression) -> StaticEquality {
    if let (Some(left), Some(right)) = (
        normalized_static_integer(left),
        normalized_static_integer(right),
    ) {
        return if left == right {
            StaticEquality::Equal
        } else {
            StaticEquality::Distinct
        };
    }
    match (left, right) {
        (
            Expression::Constant {
                value: ConstantValue::None,
                ..
            },
            Expression::Constant {
                value: ConstantValue::None,
                ..
            },
        ) => StaticEquality::Equal,
        (
            Expression::Constant {
                value: ConstantValue::Bool(left),
                ..
            },
            Expression::Constant {
                value: ConstantValue::Bool(right),
                ..
            },
        ) => equality(left, right),
        (
            Expression::Constant {
                value: ConstantValue::Str(left),
                ..
            },
            Expression::Constant {
                value: ConstantValue::Str(right),
                ..
            },
        )
        | (
            Expression::Constant {
                value: ConstantValue::Bytes(left),
                ..
            },
            Expression::Constant {
                value: ConstantValue::Bytes(right),
                ..
            },
        ) => equality(left, right),
        (
            Expression::Name {
                identifier: left, ..
            },
            Expression::Name {
                identifier: right, ..
            },
        ) if left == right => StaticEquality::Equal,
        (
            Expression::Tuple { elements: left, .. },
            Expression::Tuple {
                elements: right, ..
            },
        ) => {
            if left.len() != right.len() {
                return StaticEquality::Distinct;
            }
            let mut result = StaticEquality::Equal;
            for (left, right) in left.iter().zip(right) {
                match static_equality(left, right) {
                    StaticEquality::Equal => {}
                    StaticEquality::Distinct => return StaticEquality::Distinct,
                    StaticEquality::Unknown => result = StaticEquality::Unknown,
                }
            }
            result
        }
        _ => StaticEquality::Unknown,
    }
}

fn equality<T: PartialEq>(left: &T, right: &T) -> StaticEquality {
    if left == right {
        StaticEquality::Equal
    } else {
        StaticEquality::Distinct
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StaticInteger {
    Zero,
    NonZero,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StaticTupleIndex {
    InBounds(usize),
    OutOfBounds,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StaticIntegerSign {
    Positive,
    Negative,
}

impl StaticIntegerSign {
    fn flipped(self) -> Self {
        match self {
            Self::Positive => Self::Negative,
            Self::Negative => Self::Positive,
        }
    }
}

pub(super) fn static_tuple_index(
    expression: &Expression,
    length: usize,
) -> Option<StaticTupleIndex> {
    let (sign, magnitude) = static_integer_literal(expression, StaticIntegerSign::Positive)?;
    let Ok(magnitude) = magnitude.parse::<usize>() else {
        return Some(StaticTupleIndex::OutOfBounds);
    };
    if sign == StaticIntegerSign::Positive || magnitude == 0 {
        return Some(if magnitude < length {
            StaticTupleIndex::InBounds(magnitude)
        } else {
            StaticTupleIndex::OutOfBounds
        });
    }
    Some(if magnitude <= length {
        StaticTupleIndex::InBounds(length - magnitude)
    } else {
        StaticTupleIndex::OutOfBounds
    })
}

pub(super) fn static_integer(expression: &Expression) -> Option<StaticInteger> {
    let (_, magnitude) = static_integer_literal(expression, StaticIntegerSign::Positive)?;
    Some(if magnitude.bytes().all(|byte| byte == b'0') {
        StaticInteger::Zero
    } else {
        StaticInteger::NonZero
    })
}

fn static_integer_literal(
    expression: &Expression,
    sign: StaticIntegerSign,
) -> Option<(StaticIntegerSign, &str)> {
    match expression {
        Expression::Constant {
            value: ConstantValue::Int(value),
            ..
        } => Some((sign, value)),
        Expression::Unary {
            operator: UnaryOperator::Positive,
            operand,
            ..
        } => static_integer_literal(operand, sign),
        Expression::Unary {
            operator: UnaryOperator::Negative,
            operand,
            ..
        } => static_integer_literal(operand, sign.flipped()),
        _ => None,
    }
}

fn normalized_static_integer(expression: &Expression) -> Option<(StaticIntegerSign, &str)> {
    let (sign, magnitude) = static_integer_literal(expression, StaticIntegerSign::Positive)?;
    let magnitude = magnitude.trim_start_matches('0');
    if magnitude.is_empty() {
        Some((StaticIntegerSign::Positive, "0"))
    } else {
        Some((sign, magnitude))
    }
}

pub(super) fn type_mismatch(
    filename: &str,
    function: Option<&str>,
    span: SourceSpan,
    expected: &Type,
    actual: &Type,
) -> Diagnostic {
    Diagnostic::error(
        "P1104",
        filename.to_owned(),
        Some(span),
        function.map(str::to_owned),
        format!("Type mismatch: expected {expected}, got {actual}"),
    )
}

#[cfg(test)]
mod tests {
    use efct_protocol::{ConstantValue, SourceSpan, UnaryOperator};

    use crate::hir::Expression;

    use super::{
        StaticBoolean, StaticMapKeys, StaticTupleIndex, static_boolean, static_map_keys,
        static_tuple_index,
    };

    const SPAN: SourceSpan = SourceSpan {
        start_line: 1,
        start_utf8_byte: 0,
        end_line: 1,
        end_utf8_byte: 1,
    };

    fn integer(value: &str) -> Expression {
        Expression::Constant {
            value: ConstantValue::Int(value.to_owned()),
            span: SPAN,
        }
    }

    fn boolean(value: bool) -> Expression {
        Expression::Constant {
            value: ConstantValue::Bool(value),
            span: SPAN,
        }
    }

    fn unary(operator: UnaryOperator, operand: Expression) -> Expression {
        Expression::Unary {
            operator,
            operand: Box::new(operand),
            span: SPAN,
        }
    }

    fn string(value: &str) -> Expression {
        Expression::Constant {
            value: ConstantValue::Str(value.to_owned()),
            span: SPAN,
        }
    }

    fn name(identifier: &str) -> Expression {
        Expression::Name {
            identifier: identifier.to_owned(),
            span: SPAN,
        }
    }

    fn tuple(elements: Vec<Expression>) -> Expression {
        Expression::Tuple {
            elements,
            span: SPAN,
        }
    }

    fn entry(key: Expression) -> Expression {
        tuple(vec![key, integer("1")])
    }

    #[test]
    fn resolves_positive_and_negative_tuple_indices() {
        assert_eq!(
            static_tuple_index(&integer("1"), 3),
            Some(StaticTupleIndex::InBounds(1))
        );
        assert_eq!(
            static_tuple_index(&unary(UnaryOperator::Negative, integer("1")), 3),
            Some(StaticTupleIndex::InBounds(2))
        );
        assert_eq!(
            static_tuple_index(&unary(UnaryOperator::Negative, integer("3")), 3),
            Some(StaticTupleIndex::InBounds(0))
        );
    }

    #[test]
    fn rejects_static_tuple_indices_outside_both_bounds() {
        assert_eq!(
            static_tuple_index(&integer("3"), 3),
            Some(StaticTupleIndex::OutOfBounds)
        );
        assert_eq!(
            static_tuple_index(&unary(UnaryOperator::Negative, integer("4")), 3),
            Some(StaticTupleIndex::OutOfBounds)
        );
        assert_eq!(
            static_tuple_index(&integer("999999999999999999999999999999"), 3),
            Some(StaticTupleIndex::OutOfBounds)
        );
        assert_eq!(
            static_tuple_index(&integer("0"), 0),
            Some(StaticTupleIndex::OutOfBounds)
        );
    }

    #[test]
    fn composes_static_integer_unary_signs() {
        let expression = unary(
            UnaryOperator::Negative,
            unary(UnaryOperator::Negative, integer("1")),
        );
        assert_eq!(
            static_tuple_index(&expression, 3),
            Some(StaticTupleIndex::InBounds(1))
        );
    }

    #[test]
    fn classifies_boolean_literals_and_static_negation() {
        assert_eq!(static_boolean(&boolean(true)), StaticBoolean::True);
        assert_eq!(static_boolean(&boolean(false)), StaticBoolean::False);
        assert_eq!(
            static_boolean(&unary(UnaryOperator::Not, boolean(false))),
            StaticBoolean::True
        );
        assert_eq!(static_boolean(&name("condition")), StaticBoolean::Unknown);
    }

    #[test]
    fn classifies_static_frozen_map_key_sets() {
        let unique = tuple(vec![entry(string("left")), entry(string("right"))]);
        let duplicate = tuple(vec![entry(string("same")), entry(string("same"))]);
        let same_binding = tuple(vec![entry(name("key")), entry(name("key"))]);
        let unknown = tuple(vec![entry(name("left")), entry(name("right"))]);

        assert_eq!(static_map_keys(&unique), StaticMapKeys::Unique);
        assert_eq!(static_map_keys(&duplicate), StaticMapKeys::Duplicate);
        assert_eq!(static_map_keys(&same_binding), StaticMapKeys::Duplicate);
        assert_eq!(static_map_keys(&unknown), StaticMapKeys::Unknown);
    }

    #[test]
    fn normalizes_signed_integer_frozen_map_keys() {
        let duplicate_zero = tuple(vec![
            entry(integer("0")),
            entry(unary(UnaryOperator::Negative, integer("0"))),
        ]);
        let unique = tuple(vec![
            entry(unary(UnaryOperator::Negative, integer("1"))),
            entry(integer("1")),
        ]);

        assert_eq!(static_map_keys(&duplicate_zero), StaticMapKeys::Duplicate);
        assert_eq!(static_map_keys(&unique), StaticMapKeys::Unique);
    }
}
