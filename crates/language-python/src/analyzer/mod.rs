use std::collections::{BTreeMap, BTreeSet};

use efct_engine::{
    CallEdge, DirectEffectOrigin, infer_effect_fixed_point, record_recursive_divergence,
};
use efct_model::{
    Diagnostic, Effect, EffectFormula, EffectSet, EffectTerm, EffectVariable, ExceptionId,
    PartialBehavior,
};
use efct_protocol::{SourceSpan, TrustPolicy};

use crate::exceptions::ExceptionHierarchy;
use crate::external::ExternalDefinition;
use crate::hir::{Expression, Function, FunctionDeclaration, Module};
use crate::types::Type;

mod calls;
mod context_managers;
mod expressions;
mod matches;
mod module_symbols;
mod signatures;
mod statements;
mod termination;
mod typing;
mod validation;

use module_symbols::{
    analyze_constants, analyze_records, reject_symbol_collisions, validate_imports,
};
use signatures::analyze_signatures;
use validation::{reject_calls_to_invalid_functions, verify_effect_declarations};

#[derive(Debug, Clone)]
pub(crate) struct FunctionSignature {
    pub(crate) declaration: FunctionDeclaration,
    pub(crate) parameters: Vec<Type>,
    pub(crate) returns: Type,
    pub(crate) effect_parameters: BTreeMap<String, EffectVariable>,
    pub(crate) declared_effects: EffectFormula,
}

pub(crate) struct RuntimeTypes {
    pub(crate) constants: BTreeMap<String, Type>,
    pub(crate) signatures: BTreeMap<String, FunctionSignature>,
}

pub(crate) enum AnalysisResult {
    Accepted(RuntimeTypes),
    Rejected(Vec<Diagnostic>),
}

impl AnalysisResult {
    fn into_diagnostics(self) -> Vec<Diagnostic> {
        match self {
            Self::Accepted(_) => Vec::new(),
            Self::Rejected(diagnostics) => diagnostics,
        }
    }
}

pub(crate) fn runtime_types_with_exceptions(
    module: &Module,
    exceptions: &ExceptionHierarchy,
    mut diagnostics: Vec<Diagnostic>,
) -> Result<RuntimeTypes, Vec<Diagnostic>> {
    let records = analyze_records(module, &mut diagnostics);
    let constants = analyze_constants(module, &records, &mut diagnostics);
    let signatures = analyze_signatures(module, &records, exceptions, &mut diagnostics);
    reject_symbol_collisions(module, &constants, &records, &signatures, &mut diagnostics);
    if diagnostics.is_empty() {
        Ok(RuntimeTypes {
            constants,
            signatures,
        })
    } else {
        Err(diagnostics)
    }
}

#[derive(Debug, Clone)]
struct TypedExpression {
    value_type: Type,
    effects: EffectFormula,
}

impl TypedExpression {
    fn pure(value_type: Type) -> Self {
        Self {
            value_type,
            effects: EffectFormula::new(),
        }
    }

    fn never(effects: EffectFormula) -> Self {
        Self {
            value_type: Type::Never,
            effects,
        }
    }

    fn is_never(&self) -> bool {
        self.value_type == Type::Never
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ControlFlow {
    FallsThrough,
    FunctionExit,
    ExceptionExit,
    Rethrow,
    Break,
    Continue,
    Diverges,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ControlFlowSet(BTreeSet<ControlFlow>);

impl ControlFlowSet {
    fn empty() -> Self {
        Self(BTreeSet::new())
    }

    fn single(flow: ControlFlow) -> Self {
        Self(BTreeSet::from([flow]))
    }

    fn may_fall_through(&self) -> bool {
        self.0.contains(&ControlFlow::FallsThrough)
    }

    fn contains_exception_exit(&self) -> bool {
        self.0.contains(&ControlFlow::ExceptionExit) || self.0.contains(&ControlFlow::Rethrow)
    }

    fn contains_rethrow(&self) -> bool {
        self.0.contains(&ControlFlow::Rethrow)
    }

    fn has_non_exception_path(&self) -> bool {
        self.0.iter().any(|flow| {
            !matches!(
                flow,
                ControlFlow::ExceptionExit | ControlFlow::Rethrow | ControlFlow::Diverges
            )
        })
    }

    fn reaches_finally(&self) -> bool {
        self.0.iter().any(|flow| *flow != ControlFlow::Diverges)
    }

    fn may_continue_loop(&self) -> bool {
        self.0
            .iter()
            .any(|flow| matches!(flow, ControlFlow::FallsThrough | ControlFlow::Continue))
    }

    fn without_exception_exit(&self) -> Self {
        Self(
            self.0
                .iter()
                .filter(|flow| !matches!(flow, ControlFlow::ExceptionExit | ControlFlow::Rethrow))
                .copied()
                .collect(),
        )
    }

    fn insert(&mut self, flow: ControlFlow) {
        self.0.insert(flow);
    }

    fn sequence(&self, next: &Self) -> Self {
        let mut result: BTreeSet<_> = self
            .0
            .iter()
            .filter(|flow| **flow != ControlFlow::FallsThrough)
            .copied()
            .collect();
        if self.may_fall_through() {
            result.extend(next.0.iter().copied());
        }
        Self(result)
    }

    fn union(&mut self, other: &Self) {
        self.0.extend(other.0.iter().copied());
    }

    fn followed_by_finally(&self, finalizer: &Self) -> Self {
        let mut result: BTreeSet<_> = self
            .0
            .iter()
            .filter(|flow| **flow == ControlFlow::Diverges)
            .copied()
            .collect();
        if self.reaches_finally() {
            result.extend(
                finalizer
                    .0
                    .iter()
                    .filter(|flow| **flow != ControlFlow::FallsThrough)
                    .map(|flow| match flow {
                        ControlFlow::Rethrow => ControlFlow::ExceptionExit,
                        other => *other,
                    }),
            );
            if finalizer.may_fall_through() {
                result.extend(
                    self.0
                        .iter()
                        .filter(|flow| **flow != ControlFlow::Diverges)
                        .copied(),
                );
            }
        }
        Self(result)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum BareRaiseOutcome {
    ReRaise(EffectTerm),
    MissingCurrentException(ExceptionId),
}

#[derive(Debug, Clone)]
enum RethrowContext {
    OutsideHandler,
    Handler {
        caught: BTreeSet<PartialBehavior>,
    },
    Finally {
        outcomes: BTreeSet<BareRaiseOutcome>,
    },
}

#[derive(Debug)]
struct StatementAnalysis {
    flows: ControlFlowSet,
    direct_effects: EffectFormula,
    possible_effects: EffectFormula,
}

impl StatementAnalysis {
    fn from_direct(flow: ControlFlow, direct_effects: EffectFormula) -> Self {
        let mut flows = ControlFlowSet::single(flow);
        if direct_effects.contains_divergence() {
            flows.insert(ControlFlow::Diverges);
        }
        Self {
            flows,
            possible_effects: direct_effects.clone(),
            direct_effects,
        }
    }

    fn from_parts(
        flow: ControlFlow,
        direct_effects: EffectFormula,
        possible_effects: EffectFormula,
    ) -> Self {
        let mut flows = ControlFlowSet::single(flow);
        if possible_effects.contains_divergence() {
            flows.insert(ControlFlow::Diverges);
        }
        Self {
            flows,
            direct_effects,
            possible_effects,
        }
    }

    fn from_flow_set(
        flows: ControlFlowSet,
        direct_effects: EffectFormula,
        possible_effects: EffectFormula,
    ) -> Self {
        let mut flows = flows;
        if possible_effects.contains_divergence() {
            flows.insert(ControlFlow::Diverges);
        }
        Self {
            flows,
            direct_effects,
            possible_effects,
        }
    }
}

struct ExpressionAnalysis {
    value: TypedExpression,
    possible_effects: EffectFormula,
}

pub fn analyze(module: &Module) -> Vec<Diagnostic> {
    analyze_runtime(module).into_diagnostics()
}

pub fn analyze_with_externals(
    module: &Module,
    external_definitions: Vec<ExternalDefinition>,
    policy: TrustPolicy,
) -> Vec<Diagnostic> {
    analyze_module(module, external_definitions, policy).into_diagnostics()
}

pub(crate) fn analyze_runtime(module: &Module) -> AnalysisResult {
    analyze_module(module, Vec::new(), TrustPolicy::Default)
}

fn analyze_module(
    module: &Module,
    external_definitions: Vec<ExternalDefinition>,
    policy: TrustPolicy,
) -> AnalysisResult {
    let mut diagnostics = Vec::new();
    validate_imports(module, &mut diagnostics);
    let exceptions = ExceptionHierarchy::analyze(module, &mut diagnostics);
    let records = analyze_records(module, &mut diagnostics);
    let constants = analyze_constants(module, &records, &mut diagnostics);
    let signatures = analyze_signatures(module, &records, &exceptions, &mut diagnostics);
    let externals: BTreeMap<String, ExternalDefinition> = external_definitions
        .into_iter()
        .map(|definition| (definition.path.clone(), definition))
        .collect();
    let api_imports = crate::api_model::import_bindings(&module.imports);
    reject_symbol_collisions(module, &constants, &records, &signatures, &mut diagnostics);

    let environment = AnalysisEnvironment {
        module,
        signatures: &signatures,
        constants: &constants,
        records: &records,
        exceptions: &exceptions,
        externals: &externals,
        api_imports: &api_imports,
        policy,
    };
    let mut provisional_diagnostics = Vec::new();
    let mut initial_bodies = environment.analyze_bodies(&mut provisional_diagnostics, None, false);
    let initial_functions = initial_bodies.keys().cloned().collect();
    record_python_recursive_divergence(
        module,
        &signatures,
        &mut initial_bodies,
        &initial_functions,
    );
    let mut reachability_effects = infer_effect_fixed_point(&initial_bodies, &initial_functions);
    loop {
        let mut refinement_diagnostics = Vec::new();
        let mut refined_bodies = environment.analyze_bodies(
            &mut refinement_diagnostics,
            Some(&reachability_effects),
            false,
        );
        let refined_functions = refined_bodies.keys().cloned().collect();
        record_python_recursive_divergence(
            module,
            &signatures,
            &mut refined_bodies,
            &refined_functions,
        );
        let refined_effects = infer_effect_fixed_point(&refined_bodies, &refined_functions);
        let stable = refined_effects == reachability_effects;
        reachability_effects = refined_effects;
        if stable {
            break;
        }
    }
    let mut bodies =
        environment.analyze_bodies(&mut diagnostics, Some(&reachability_effects), true);
    let analyzed_functions = bodies.keys().cloned().collect();
    record_python_recursive_divergence(module, &signatures, &mut bodies, &analyzed_functions);
    let valid_functions = reject_calls_to_invalid_functions(module, &bodies, &mut diagnostics);
    let actual_effects = infer_effect_fixed_point(&bodies, &valid_functions);
    verify_effect_declarations(
        module,
        &signatures,
        &bodies,
        &valid_functions,
        &actual_effects,
        &exceptions,
        &mut diagnostics,
    );

    diagnostics.sort_by(|left, right| {
        let left_position = left.span.map(|span| {
            (
                span.start_line,
                span.start_utf8_byte,
                span.end_line,
                span.end_utf8_byte,
            )
        });
        let right_position = right.span.map(|span| {
            (
                span.start_line,
                span.start_utf8_byte,
                span.end_line,
                span.end_utf8_byte,
            )
        });
        left.filename
            .cmp(&right.filename)
            .then_with(|| left_position.cmp(&right_position))
            .then_with(|| left.code.cmp(right.code))
    });
    if diagnostics.is_empty() {
        AnalysisResult::Accepted(RuntimeTypes {
            constants,
            signatures,
        })
    } else {
        AnalysisResult::Rejected(diagnostics)
    }
}

fn record_python_recursive_divergence(
    module: &Module,
    signatures: &BTreeMap<String, FunctionSignature>,
    bodies: &mut BTreeMap<String, efct_engine::FunctionEffects>,
    valid: &BTreeSet<String>,
) {
    let well_founded = termination::prove_well_founded_self_calls(module, signatures, bodies);
    record_recursive_divergence(bodies, valid, &well_founded);
}

struct AnalysisEnvironment<'a> {
    module: &'a Module,
    signatures: &'a BTreeMap<String, FunctionSignature>,
    constants: &'a BTreeMap<String, Type>,
    records: &'a BTreeMap<String, Type>,
    exceptions: &'a ExceptionHierarchy,
    externals: &'a BTreeMap<String, ExternalDefinition>,
    api_imports: &'a BTreeMap<String, crate::api_model::ImportBinding>,
    policy: TrustPolicy,
}

impl AnalysisEnvironment<'_> {
    fn analyze_bodies(
        &self,
        diagnostics: &mut Vec<Diagnostic>,
        known_effects: Option<&BTreeMap<String, EffectFormula>>,
        validate_returns: bool,
    ) -> BTreeMap<String, efct_engine::FunctionEffects> {
        let mut bodies = BTreeMap::new();
        for function in &self.module.functions {
            let Some(signature) = self.signatures.get(&function.name) else {
                continue;
            };
            let diagnostic_count = diagnostics.len();
            let mut analyzer = FunctionAnalyzer {
                filename: &self.module.filename,
                function,
                signature,
                signatures: self.signatures,
                constants: self.constants,
                records: self.records,
                exceptions: self.exceptions,
                type_imports: &self.module.imports,
                externals: self.externals,
                api_imports: self.api_imports,
                policy: self.policy,
                locals: BTreeMap::new(),
                diagnostics,
                loop_depth: 0,
                calls: BTreeSet::new(),
                direct_origins: BTreeMap::new(),
                handled_effects: EffectSet::new(),
                rethrow_context: RethrowContext::OutsideHandler,
                known_effects,
                validate_returns,
            };
            let result = analyzer.analyze();
            if diagnostics.len() == diagnostic_count
                && let Some(result) = result
            {
                bodies.insert(function.name.clone(), result);
            }
        }
        bodies
    }
}

struct FunctionAnalyzer<'a> {
    filename: &'a str,
    function: &'a Function,
    signature: &'a FunctionSignature,
    signatures: &'a BTreeMap<String, FunctionSignature>,
    constants: &'a BTreeMap<String, Type>,
    records: &'a BTreeMap<String, Type>,
    exceptions: &'a ExceptionHierarchy,
    type_imports: &'a [crate::hir::Import],
    externals: &'a BTreeMap<String, ExternalDefinition>,
    api_imports: &'a BTreeMap<String, crate::api_model::ImportBinding>,
    policy: TrustPolicy,
    locals: BTreeMap<String, Type>,
    diagnostics: &'a mut Vec<Diagnostic>,
    loop_depth: usize,
    calls: BTreeSet<CallEdge>,
    direct_origins: BTreeMap<EffectTerm, BTreeSet<DirectEffectOrigin>>,
    handled_effects: EffectSet,
    rethrow_context: RethrowContext,
    known_effects: Option<&'a BTreeMap<String, EffectFormula>>,
    validate_returns: bool,
}

impl FunctionAnalyzer<'_> {
    fn analyze_expression(&mut self, expression: &Expression) -> Option<ExpressionAnalysis> {
        let previous_calls = self.calls.clone();
        let value = self.expression(expression)?;
        let mut possible_effects = value.effects.clone();
        self.extend_possible_call_effects(&mut possible_effects, &previous_calls);
        Some(ExpressionAnalysis {
            value,
            possible_effects,
        })
    }

    fn extend_possible_call_effects(
        &self,
        possible_effects: &mut EffectFormula,
        previous_calls: &BTreeSet<CallEdge>,
    ) {
        let Some(known_effects) = self.known_effects else {
            return;
        };
        for call in self.calls.difference(previous_calls) {
            let CallEdge::Invoke {
                target, bindings, ..
            } = call
            else {
                continue;
            };
            if let Some(effects) = known_effects.get(target) {
                possible_effects.extend(effects.substitute(bindings));
            }
        }
    }

    fn record_effect(
        &mut self,
        effects: &mut EffectFormula,
        effect: Effect,
        span: SourceSpan,
        operation: impl Into<String>,
    ) {
        effects.insert(effect.clone());
        if self.handled_effects.contains(&effect) {
            return;
        }
        self.direct_origins
            .entry(EffectTerm::Concrete(effect))
            .or_default()
            .insert(DirectEffectOrigin {
                span,
                operation: operation.into(),
            });
    }

    fn record_formula(
        &mut self,
        effects: &mut EffectFormula,
        formula: &EffectFormula,
        span: SourceSpan,
        operation: impl Into<String>,
    ) {
        let operation = operation.into();
        for term in formula.iter() {
            effects.extend([term.clone()]);
            let handled = match term {
                EffectTerm::Concrete(effect) => self.handled_effects.contains(effect),
                EffectTerm::Variable(_) => false,
            };
            if !handled {
                self.direct_origins
                    .entry(term.clone())
                    .or_default()
                    .insert(DirectEffectOrigin {
                        span,
                        operation: operation.clone(),
                    });
            }
        }
    }
}
