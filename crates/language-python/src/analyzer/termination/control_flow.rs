use crate::hir::{Function, RaiseCause, Statement, WithItem};

use super::evidence::{CallProof, PathFacts, analyze_expression};
use crate::analyzer::typing::{StaticBoolean, static_boolean};

#[derive(Debug)]
pub(super) struct BlockProof {
    fallthrough: Option<PathFacts>,
    pub(super) calls: Vec<CallProof>,
}

pub(super) fn analyze_block(
    statements: &[Statement],
    mut facts: PathFacts,
    function: &Function,
) -> BlockProof {
    let mut calls = Vec::new();
    for statement in statements {
        let proof = analyze_statement(statement, facts, function);
        calls.extend(proof.calls);
        let Some(next) = proof.fallthrough else {
            return BlockProof {
                fallthrough: None,
                calls,
            };
        };
        facts = next;
    }
    BlockProof {
        fallthrough: Some(facts),
        calls,
    }
}

fn analyze_statement(
    statement: &Statement,
    mut facts: PathFacts,
    function: &Function,
) -> BlockProof {
    let mut calls = Vec::new();
    match statement {
        Statement::ModuleImport { .. } | Statement::Pass(_) => {}
        Statement::Return { value, .. } => {
            if let Some(value) = value {
                analyze_expression(value, &facts, function, &mut calls);
            }
            return no_fallthrough(calls);
        }
        Statement::Assign { target, value, .. } => {
            analyze_expression(value, &facts, function, &mut calls);
            facts.invalidate_target(target);
        }
        Statement::AnnotatedAssignment { target, value, .. } => {
            if let Some(value) = value {
                analyze_expression(value, &facts, function, &mut calls);
            }
            facts.invalidate_target(target);
        }
        Statement::AugmentedAssignment { target, value, .. } => {
            analyze_expression(value, &facts, function, &mut calls);
            facts.invalidate_target(target);
        }
        Statement::Expression { value, .. } => {
            analyze_expression(value, &facts, function, &mut calls);
        }
        Statement::If {
            condition,
            body,
            otherwise,
            ..
        } => {
            analyze_expression(condition, &facts, function, &mut calls);
            let truth = static_boolean(condition);
            let mut body_facts = facts.clone();
            body_facts.apply_condition_bound(condition, true);
            let mut otherwise_facts = facts;
            otherwise_facts.apply_condition_bound(condition, false);
            let body_proof = analyze_block(body, body_facts, function);
            let otherwise_proof = analyze_block(otherwise, otherwise_facts, function);
            match truth {
                StaticBoolean::True => {
                    calls.extend(body_proof.calls);
                    return BlockProof {
                        fallthrough: body_proof.fallthrough,
                        calls,
                    };
                }
                StaticBoolean::False => {
                    calls.extend(otherwise_proof.calls);
                    return BlockProof {
                        fallthrough: otherwise_proof.fallthrough,
                        calls,
                    };
                }
                StaticBoolean::Unknown => {
                    calls.extend(body_proof.calls);
                    calls.extend(otherwise_proof.calls);
                    return BlockProof {
                        fallthrough: merge_fallthrough(
                            body_proof.fallthrough,
                            otherwise_proof.fallthrough,
                        ),
                        calls,
                    };
                }
            }
        }
        Statement::Assert {
            condition, message, ..
        } => {
            analyze_expression(condition, &facts, function, &mut calls);
            if let Some(message) = message {
                analyze_expression(message, &PathFacts::disabled(), function, &mut calls);
            }
        }
        Statement::Raise {
            exception, cause, ..
        } => {
            if let Some(exception) = exception {
                analyze_expression(exception, &facts, function, &mut calls);
            }
            if let RaiseCause::Explicit(cause) = cause {
                analyze_expression(cause, &facts, function, &mut calls);
            }
            return no_fallthrough(calls);
        }
        Statement::For {
            iterable,
            body,
            otherwise,
            ..
        } => {
            analyze_expression(iterable, &facts, function, &mut calls);
            calls.extend(analyze_block(body, PathFacts::disabled(), function).calls);
            calls.extend(analyze_block(otherwise, PathFacts::disabled(), function).calls);
            facts = PathFacts::disabled();
        }
        Statement::While {
            condition,
            body,
            otherwise,
            ..
        } => {
            analyze_expression(condition, &facts, function, &mut calls);
            calls.extend(analyze_block(body, PathFacts::disabled(), function).calls);
            calls.extend(analyze_block(otherwise, PathFacts::disabled(), function).calls);
            facts = PathFacts::disabled();
        }
        Statement::Match { subject, cases, .. } => {
            analyze_expression(subject, &facts, function, &mut calls);
            for case in cases {
                calls.extend(analyze_block(&case.body, PathFacts::disabled(), function).calls);
            }
            facts = PathFacts::disabled();
        }
        Statement::Try {
            body,
            handlers,
            otherwise,
            finalizer,
            ..
        } => {
            calls.extend(analyze_block(body, PathFacts::disabled(), function).calls);
            for handler in handlers.as_slice() {
                calls.extend(analyze_block(&handler.body, PathFacts::disabled(), function).calls);
            }
            calls.extend(analyze_block(otherwise, PathFacts::disabled(), function).calls);
            calls.extend(analyze_block(finalizer, PathFacts::disabled(), function).calls);
            facts = PathFacts::disabled();
        }
        Statement::With { items, body, .. } => {
            for item in items {
                match item {
                    WithItem::Unbound { context } => {
                        analyze_expression(context, &facts, function, &mut calls);
                    }
                    WithItem::Bound { context, target } => {
                        analyze_expression(context, &facts, function, &mut calls);
                        facts.invalidate_target(target);
                    }
                }
            }
            calls.extend(analyze_block(body, PathFacts::disabled(), function).calls);
            facts = PathFacts::disabled();
        }
        Statement::Break(_) | Statement::Continue(_) => return no_fallthrough(calls),
    }
    BlockProof {
        fallthrough: Some(facts),
        calls,
    }
}

fn no_fallthrough(calls: Vec<CallProof>) -> BlockProof {
    BlockProof {
        fallthrough: None,
        calls,
    }
}

fn merge_fallthrough(left: Option<PathFacts>, right: Option<PathFacts>) -> Option<PathFacts> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.intersect(&right)),
        (Some(facts), None) | (None, Some(facts)) => Some(facts),
        (None, None) => None,
    }
}
