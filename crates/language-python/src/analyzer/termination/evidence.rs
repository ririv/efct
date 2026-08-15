use std::collections::{BTreeMap, BTreeSet};

use efct_protocol::{BinaryOperator, ComparisonOperator, ConstantValue, SourceSpan, UnaryOperator};

use crate::hir::{Expression, Function};
use crate::types::Type;

use crate::analyzer::FunctionSignature;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum IntegerBound {
    Lower,
    Upper,
}

#[derive(Debug, Clone)]
pub(super) struct PathFacts {
    eligible_parameters: BTreeSet<String>,
    bounds: BTreeMap<String, BTreeSet<IntegerBound>>,
}

impl PathFacts {
    pub(super) fn new(function: &Function, signature: &FunctionSignature) -> Self {
        let eligible_parameters = function
            .parameters
            .iter()
            .zip(&signature.parameters)
            .filter(|(_, parameter_type)| **parameter_type == Type::Int)
            .map(|(parameter, _)| parameter.name.clone())
            .collect();
        Self {
            eligible_parameters,
            bounds: BTreeMap::new(),
        }
    }

    pub(super) fn disabled() -> Self {
        Self {
            eligible_parameters: BTreeSet::new(),
            bounds: BTreeMap::new(),
        }
    }

    pub(super) fn apply_condition_bound(&mut self, condition: &Expression, outcome: bool) {
        let Some((parameter, bound)) = condition_bound(condition, outcome) else {
            return;
        };
        if self.eligible_parameters.contains(parameter) {
            self.bounds
                .entry(parameter.to_owned())
                .or_default()
                .insert(bound);
        }
    }

    pub(super) fn invalidate_target(&mut self, target: &Expression) {
        if let Expression::Name { identifier, .. } = target {
            self.eligible_parameters.remove(identifier);
            self.bounds.remove(identifier);
        }
    }

    pub(super) fn intersect(&self, other: &Self) -> Self {
        let eligible_parameters = self
            .eligible_parameters
            .intersection(&other.eligible_parameters)
            .cloned()
            .collect();
        let bounds = self
            .bounds
            .iter()
            .filter_map(|(parameter, left)| {
                if !other.eligible_parameters.contains(parameter) {
                    return None;
                }
                let right = other.bounds.get(parameter)?;
                let common: BTreeSet<_> = left.intersection(right).copied().collect();
                (!common.is_empty()).then(|| (parameter.clone(), common))
            })
            .collect();
        Self {
            eligible_parameters,
            bounds,
        }
    }
}

#[derive(Debug)]
pub(super) struct CallProof {
    pub(super) span: SourceSpan,
    pub(super) measures: BTreeSet<String>,
}

pub(super) fn analyze_expression(
    expression: &Expression,
    facts: &PathFacts,
    function: &Function,
    calls: &mut Vec<CallProof>,
) {
    match expression {
        Expression::Name { .. } | Expression::Constant { .. } => {}
        Expression::Tuple { elements, .. } | Expression::List { elements, .. } => {
            for element in elements {
                analyze_expression(element, facts, function, calls);
            }
        }
        Expression::Unary { operand, .. } => analyze_expression(operand, facts, function, calls),
        Expression::Binary { left, right, .. } => {
            analyze_expression(left, facts, function, calls);
            analyze_expression(right, facts, function, calls);
        }
        Expression::Boolean { values, .. } => {
            for value in values {
                analyze_expression(value, &PathFacts::disabled(), function, calls);
            }
        }
        Expression::Compare {
            left, comparators, ..
        } => {
            analyze_expression(left, facts, function, calls);
            for comparator in comparators {
                analyze_expression(comparator, facts, function, calls);
            }
        }
        Expression::Conditional {
            condition,
            then_value,
            else_value,
            ..
        } => {
            analyze_expression(condition, facts, function, calls);
            let mut then_facts = facts.clone();
            then_facts.apply_condition_bound(condition, true);
            analyze_expression(then_value, &then_facts, function, calls);
            let mut else_facts = facts.clone();
            else_facts.apply_condition_bound(condition, false);
            analyze_expression(else_value, &else_facts, function, calls);
        }
        Expression::Call {
            callee,
            arguments,
            span,
        } => {
            analyze_expression(callee, facts, function, calls);
            for argument in arguments {
                analyze_expression(argument, facts, function, calls);
            }
            if matches!(callee.as_ref(), Expression::Name { identifier, .. } if identifier == &function.name)
            {
                calls.push(CallProof {
                    span: *span,
                    measures: recursive_call_measures(arguments, facts, function),
                });
            }
        }
        Expression::Attribute { value, .. } => analyze_expression(value, facts, function, calls),
        Expression::Subscript { value, slice, .. } => {
            analyze_expression(value, facts, function, calls);
            analyze_expression(slice, facts, function, calls);
        }
    }
}

fn recursive_call_measures(
    arguments: &[Expression],
    facts: &PathFacts,
    function: &Function,
) -> BTreeSet<String> {
    function
        .parameters
        .iter()
        .enumerate()
        .filter_map(|(index, parameter)| {
            let argument = arguments.get(index)?;
            let bounds = facts.bounds.get(&parameter.name)?;
            let decreases = bounds.contains(&IntegerBound::Lower)
                && decreases_parameter(argument, &parameter.name);
            let increases = bounds.contains(&IntegerBound::Upper)
                && increases_parameter(argument, &parameter.name);
            (decreases || increases).then(|| parameter.name.clone())
        })
        .collect()
}

fn decreases_parameter(expression: &Expression, parameter: &str) -> bool {
    matches!(
        expression,
        Expression::Binary {
            operator: BinaryOperator::Subtract,
            left,
            right,
            ..
        } if is_name(left, parameter) && is_positive_integer_literal(right)
    )
}

fn increases_parameter(expression: &Expression, parameter: &str) -> bool {
    matches!(
        expression,
        Expression::Binary {
            operator: BinaryOperator::Add,
            left,
            right,
            ..
        } if (is_name(left, parameter) && is_positive_integer_literal(right))
            || (is_positive_integer_literal(left) && is_name(right, parameter))
    )
}

fn condition_bound(condition: &Expression, outcome: bool) -> Option<(&str, IntegerBound)> {
    if let Expression::Unary {
        operator: UnaryOperator::Not,
        operand,
        ..
    } = condition
    {
        return condition_bound(operand, !outcome);
    }
    let Expression::Compare {
        left,
        operators,
        comparators,
        ..
    } = condition
    else {
        return None;
    };
    let [operator] = operators.as_slice() else {
        return None;
    };
    let [right] = comparators.as_slice() else {
        return None;
    };
    let (parameter, relation) = if let Expression::Name { identifier, .. } = left.as_ref()
        && is_integer_literal(right)
    {
        (identifier.as_str(), *operator)
    } else if let Expression::Name { identifier, .. } = right
        && is_integer_literal(left)
    {
        (identifier.as_str(), reverse_comparison(*operator)?)
    } else {
        return None;
    };
    let relation = if outcome {
        relation
    } else {
        negate_comparison(relation)?
    };
    let bound = match relation {
        ComparisonOperator::Greater | ComparisonOperator::GreaterEqual => IntegerBound::Lower,
        ComparisonOperator::Less | ComparisonOperator::LessEqual => IntegerBound::Upper,
        ComparisonOperator::Equal
        | ComparisonOperator::NotEqual
        | ComparisonOperator::Is
        | ComparisonOperator::IsNot
        | ComparisonOperator::In
        | ComparisonOperator::NotIn
        | ComparisonOperator::Unknown => return None,
    };
    Some((parameter, bound))
}

fn reverse_comparison(operator: ComparisonOperator) -> Option<ComparisonOperator> {
    Some(match operator {
        ComparisonOperator::Less => ComparisonOperator::Greater,
        ComparisonOperator::LessEqual => ComparisonOperator::GreaterEqual,
        ComparisonOperator::Greater => ComparisonOperator::Less,
        ComparisonOperator::GreaterEqual => ComparisonOperator::LessEqual,
        ComparisonOperator::Equal => ComparisonOperator::Equal,
        ComparisonOperator::NotEqual => ComparisonOperator::NotEqual,
        ComparisonOperator::Is
        | ComparisonOperator::IsNot
        | ComparisonOperator::In
        | ComparisonOperator::NotIn
        | ComparisonOperator::Unknown => return None,
    })
}

fn negate_comparison(operator: ComparisonOperator) -> Option<ComparisonOperator> {
    Some(match operator {
        ComparisonOperator::Less => ComparisonOperator::GreaterEqual,
        ComparisonOperator::LessEqual => ComparisonOperator::Greater,
        ComparisonOperator::Greater => ComparisonOperator::LessEqual,
        ComparisonOperator::GreaterEqual => ComparisonOperator::Less,
        ComparisonOperator::Equal => ComparisonOperator::NotEqual,
        ComparisonOperator::NotEqual => ComparisonOperator::Equal,
        ComparisonOperator::Is
        | ComparisonOperator::IsNot
        | ComparisonOperator::In
        | ComparisonOperator::NotIn
        | ComparisonOperator::Unknown => return None,
    })
}

fn is_name(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Name { identifier, .. } if identifier == expected)
}

fn is_integer_literal(expression: &Expression) -> bool {
    match expression {
        Expression::Constant {
            value: ConstantValue::Int(_),
            ..
        } => true,
        Expression::Unary {
            operator: UnaryOperator::Positive | UnaryOperator::Negative,
            operand,
            ..
        } => is_integer_literal(operand),
        _ => false,
    }
}

fn is_positive_integer_literal(expression: &Expression) -> bool {
    integer_literal(expression, true).is_some_and(|(positive, zero)| positive && !zero)
}

fn integer_literal(expression: &Expression, positive: bool) -> Option<(bool, bool)> {
    match expression {
        Expression::Constant {
            value: ConstantValue::Int(value),
            ..
        } => Some((positive, value.bytes().all(|byte| byte == b'0'))),
        Expression::Unary {
            operator: UnaryOperator::Positive,
            operand,
            ..
        } => integer_literal(operand, positive),
        Expression::Unary {
            operator: UnaryOperator::Negative,
            operand,
            ..
        } => integer_literal(operand, !positive),
        _ => None,
    }
}
