use std::collections::{BTreeMap, BTreeSet};

use efct_engine::{FunctionEffects, effect_trace_details};
use efct_model::{Diagnostic, Effect, EffectFormula, EffectTerm, PartialBehavior, Severity};

use crate::exceptions::ExceptionHierarchy;
use crate::hir::{FunctionDeclaration, FunctionKind, Module};

use super::FunctionSignature;

pub(super) fn reject_calls_to_invalid_functions(
    module: &Module,
    bodies: &BTreeMap<String, FunctionEffects>,
    diagnostics: &mut Vec<Diagnostic>,
) -> BTreeSet<String> {
    let mut valid: BTreeSet<String> = bodies.keys().cloned().collect();
    let mut reported = BTreeSet::new();
    loop {
        let invalid_callers: Vec<(String, String)> = valid
            .iter()
            .filter_map(|caller| {
                bodies[caller]
                    .calls
                    .iter()
                    .find(|call| !valid.contains(call.target()))
                    .map(|call| (caller.clone(), call.target().to_owned()))
            })
            .collect();
        if invalid_callers.is_empty() {
            break;
        }
        for (caller, callee) in invalid_callers {
            valid.remove(&caller);
            if reported.insert((caller.clone(), callee.clone())) {
                let span = module
                    .functions
                    .iter()
                    .find(|function| function.name == caller)
                    .map(|function| function.span);
                diagnostics.push(Diagnostic::error(
                    "P1004",
                    module.filename.clone(),
                    span,
                    Some(caller),
                    format!("Call target {callee} did not pass validation"),
                ));
            }
        }
    }
    valid
}

pub(super) fn verify_effect_declarations(
    module: &Module,
    signatures: &BTreeMap<String, FunctionSignature>,
    bodies: &BTreeMap<String, FunctionEffects>,
    valid: &BTreeSet<String>,
    actual_effects: &BTreeMap<String, EffectFormula>,
    exceptions: &ExceptionHierarchy,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for function in &module.functions {
        if !valid.contains(&function.name) {
            continue;
        }
        let signature = &signatures[&function.name];
        let actual = &actual_effects[&function.name];
        if matches!(signature.declaration, FunctionDeclaration::InferredEffects) {
            continue;
        }
        let undeclared = actual
            .iter()
            .filter(|term| !is_covered(term, &signature.declared_effects, exceptions))
            .filter(|term| {
                !matches!(
                    (&signature.declaration, term),
                    (
                        FunctionDeclaration::InferredPure,
                        EffectTerm::Concrete(effect)
                    ) if effect.is_partial()
                )
            });
        for term in undeclared {
            let (message, trace, effect_trace, suggestion) = match term {
                EffectTerm::Concrete(effect) => {
                    let details = effect_trace_details(&function.name, effect, bodies);
                    let partial_expression = match effect {
                        Effect::Partial(PartialBehavior::RaiseGroup(_)) => "RaiseGroup(...)",
                        Effect::Partial(PartialBehavior::Diverge) => "Diverge()",
                        Effect::Partial(PartialBehavior::Raise(_)) | Effect::External(_) => {
                            "Raise(...)"
                        }
                    };
                    let message = match function.kind {
                        FunctionKind::Declared if effect.is_partial() => format!(
                            "Function {} contains undeclared partial behavior {effect}",
                            function.name
                        ),
                        FunctionKind::Declared => {
                            format!(
                                "Function {} contains undeclared effect {effect}",
                                function.name
                            )
                        }
                        FunctionKind::ModuleInitializer if effect.is_partial() => {
                            format!("Module initialization contains undeclared partial behavior {effect}")
                        }
                        FunctionKind::ModuleInitializer => {
                            format!("Module initialization contains undeclared effect {effect}")
                        }
                    };
                    let suggestion = match function.kind {
                        FunctionKind::Declared if signature.declaration.is_pure() && effect.is_partial() => partial_suggestion(
                            &format!("@efct.pure(efct.partial.{partial_expression})"),
                            effect,
                        ),
                        FunctionKind::Declared if effect.is_partial() => partial_suggestion(
                            &format!("@efct.effects(efct.partial.{partial_expression})"),
                            effect,
                        ),
                        FunctionKind::Declared => format!(
                            "Declare @efct.effects(\"{effect}\") or remove the effectful operation"
                        ),
                        FunctionKind::ModuleInitializer
                            if signature.declaration.is_pure() && effect.is_partial() => partial_suggestion(
                            &format!("`_efct = efct.pure(efct.partial.{partial_expression})`"),
                            effect,
                        ),
                        FunctionKind::ModuleInitializer if effect.is_partial() => partial_suggestion(
                            &format!("`_efct = efct.effects(efct.partial.{partial_expression})`"),
                            effect,
                        ),
                        FunctionKind::ModuleInitializer => format!(
                            "Declare `_efct = efct.effects(\"{effect}\")` or remove the effectful operation"
                        ),
                    };
                    (
                        message,
                        details.labels,
                        details.frames,
                        suggestion,
                    )
                }
                EffectTerm::Variable(variable) => (
                    format!("The function propagates undeclared effect variable {variable}"),
                    Vec::new(),
                    Vec::new(),
                    "Mark the effect-generic function with @efct.effects, or remove the callback invocation".to_owned(),
                ),
            };
            let diagnostic_span = effect_trace
                .first()
                .map_or(function.span, |frame| frame.span);
            let mut diagnostic = Diagnostic::error(
                "P1001",
                module.filename.clone(),
                Some(diagnostic_span),
                Some(function.name.clone()),
                message,
            );
            diagnostic.trace = trace;
            diagnostic.effect_trace = effect_trace;
            diagnostic.suggestion = Some(suggestion);
            diagnostics.push(diagnostic);
        }
        if matches!(signature.declaration, FunctionDeclaration::InferredPure) {
            continue;
        }
        for term in signature
            .declared_effects
            .iter()
            .filter(|term| !is_used(term, actual, exceptions))
        {
            let message = match function.kind {
                FunctionKind::Declared if matches!(term, EffectTerm::Concrete(effect) if effect.is_partial()) =>
                {
                    format!("Declared partial behavior {term} is not used")
                }
                FunctionKind::Declared => format!("Declared effect {term} is not used"),
                FunctionKind::ModuleInitializer => {
                    format!("Declared module initialization effect {term} is not used")
                }
            };
            diagnostics.push(Diagnostic {
                code: "W1001",
                severity: Severity::Warning,
                filename: module.filename.clone(),
                span: Some(function.span),
                function: Some(function.name.clone()),
                message,
                trace: Vec::new(),
                effect_trace: Vec::new(),
                suggestion: Some(match function.kind {
                    FunctionKind::Declared
                        if matches!(term, EffectTerm::Concrete(effect) if effect.is_partial()) =>
                    {
                        "Remove the unused partial declaration".to_owned()
                    }
                    FunctionKind::Declared => "Remove the unused effect declaration".to_owned(),
                    FunctionKind::ModuleInitializer => {
                        "Remove the unused module initialization effect declaration".to_owned()
                    }
                }),
            });
        }
    }
}

fn partial_suggestion(declaration: &str, effect: &Effect) -> String {
    match effect {
        Effect::Partial(PartialBehavior::Diverge) => {
            format!("Declare {declaration} or prove that the operation terminates")
        }
        Effect::Partial(_) => {
            format!("Declare {declaration} or remove the partial operation {effect}")
        }
        Effect::External(_) => unreachable!("partial suggestions require partial behavior"),
    }
}

fn is_covered(
    actual: &EffectTerm,
    declared: &EffectFormula,
    exceptions: &ExceptionHierarchy,
) -> bool {
    declared
        .iter()
        .any(|declaration| declaration_covers(actual, declaration, exceptions))
}

fn is_used(
    declaration: &EffectTerm,
    actual: &EffectFormula,
    exceptions: &ExceptionHierarchy,
) -> bool {
    actual
        .iter()
        .any(|behavior| declaration_covers(behavior, declaration, exceptions))
}

fn declaration_covers(
    actual: &EffectTerm,
    declared: &EffectTerm,
    exceptions: &ExceptionHierarchy,
) -> bool {
    match (actual, declared) {
        (
            EffectTerm::Concrete(Effect::Partial(PartialBehavior::Raise(actual))),
            EffectTerm::Concrete(Effect::Partial(PartialBehavior::Raise(declared))),
        )
        | (
            EffectTerm::Concrete(Effect::Partial(PartialBehavior::RaiseGroup(actual))),
            EffectTerm::Concrete(Effect::Partial(PartialBehavior::RaiseGroup(declared))),
        ) => exceptions.is_subtype(actual, declared),
        (EffectTerm::Concrete(actual), EffectTerm::Concrete(declared)) => actual == declared,
        (EffectTerm::Variable(actual), EffectTerm::Variable(declared)) => actual == declared,
        _ => false,
    }
}
