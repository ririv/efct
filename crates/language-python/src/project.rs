use std::collections::{BTreeMap, BTreeSet};

use efct_model::TrustPolicy;
use efct_protocol::{ExternalSymbol, ProjectEnvelope, SourceSpan};

use crate::analyzer;
use crate::external;
use crate::hir::{
    ConstantDefinition, DeclarationNotation, ExceptionDefinition, Expression, Function, Import,
    Module, Pattern, RaiseCause, RecordDefinition, Statement,
};
use efct_model::Diagnostic;
use efct_project::{DependencyGraph, DependencyGraphError, validate_dependency_graph};

#[derive(Debug, Clone)]
enum ImportBinding {
    Module(String),
    Symbol(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnalysisMode {
    Diagnostics,
    Runtime,
}

pub fn check(project: ProjectEnvelope) -> Vec<Diagnostic> {
    let root = project.root;
    let policy = project.policy;
    let external_symbols = project.external_symbols;
    let modules = match prepare_modules(project.modules) {
        Ok(modules) => modules,
        Err(diagnostics) => return diagnostics,
    };
    check_prepared(root, policy, external_symbols, modules)
}

pub fn check_prepared(
    root: String,
    policy: TrustPolicy,
    external_symbols: Vec<ExternalSymbol>,
    modules: Vec<(String, crate::PreparedModule)>,
) -> Vec<Diagnostic> {
    match analyze(
        root,
        policy,
        external_symbols,
        modules,
        AnalysisMode::Diagnostics,
    ) {
        Ok(result) => result.diagnostics,
        Err(_) => unreachable!("diagnostic-only project analysis cannot build runtime plans"),
    }
}

pub fn check_runtime(project: ProjectEnvelope) -> Result<crate::ProjectRuntimeAnalysis, String> {
    let root = project.root;
    let policy = project.policy;
    let external_symbols = project.external_symbols;
    let modules = match prepare_modules(project.modules) {
        Ok(modules) => modules,
        Err(diagnostics) => {
            return Ok(crate::ProjectRuntimeAnalysis {
                diagnostics,
                modules: BTreeMap::new(),
            });
        }
    };
    analyze(
        root,
        policy,
        external_symbols,
        modules,
        AnalysisMode::Runtime,
    )
}

pub fn check_prepared_runtime(
    root: String,
    external_symbols: Vec<ExternalSymbol>,
    modules: Vec<(String, crate::PreparedModule)>,
) -> Result<crate::ProjectRuntimeAnalysis, String> {
    analyze(
        root,
        TrustPolicy::Default,
        external_symbols,
        modules,
        AnalysisMode::Runtime,
    )
}

fn analyze(
    root: String,
    policy: TrustPolicy,
    external_symbols: Vec<ExternalSymbol>,
    prepared_modules: Vec<(String, crate::PreparedModule)>,
    mode: AnalysisMode,
) -> Result<crate::ProjectRuntimeAnalysis, String> {
    let mut diagnostics = Vec::new();
    let external_definitions = match external::decode(external_symbols) {
        Ok(definitions) => definitions,
        Err((path, message)) => {
            return Ok(crate::ProjectRuntimeAnalysis {
                diagnostics: vec![Diagnostic::error(
                    "P1302",
                    root,
                    None,
                    None,
                    if path.is_empty() {
                        message
                    } else {
                        format!("Certification for external symbol {path} is invalid: {message}")
                    },
                )],
                modules: BTreeMap::new(),
            });
        }
    };
    let mut modules = BTreeMap::new();
    for (name, prepared) in prepared_modules {
        let filename = prepared.filename().to_owned();
        match prepared.into_result() {
            Ok(module) => {
                if modules.insert(name.clone(), module).is_some() {
                    diagnostics.push(Diagnostic::error(
                        "P1301",
                        filename,
                        None,
                        None,
                        format!("Project module name {name} is duplicated"),
                    ));
                }
            }
            Err(mut errors) => diagnostics.append(&mut errors),
        }
    }
    if !diagnostics.is_empty() {
        return Ok(crate::ProjectRuntimeAnalysis {
            diagnostics: sorted(diagnostics),
            modules: BTreeMap::new(),
        });
    }

    validate_import_graph(&modules, &external_definitions, &mut diagnostics);
    if !diagnostics.is_empty() {
        return Ok(crate::ProjectRuntimeAnalysis {
            diagnostics: sorted(diagnostics),
            modules: BTreeMap::new(),
        });
    }

    let filenames: BTreeMap<String, String> = modules
        .iter()
        .flat_map(|(module_name, module)| {
            module.functions.iter().map(|function| {
                (
                    qualify(module_name, &function.name),
                    module.filename.clone(),
                )
            })
        })
        .collect();
    let flattened = flatten_modules(modules.clone());
    let mut analyzed = analyzer::analyze_with_externals(&flattened, external_definitions, policy);
    for diagnostic in &mut analyzed {
        if let Some(function) = &diagnostic.function
            && let Some(filename) = filenames.get(function)
        {
            diagnostic.filename.clone_from(filename);
        }
        for frame in &mut diagnostic.effect_trace {
            if let Some(filename) = filenames.get(&frame.function) {
                frame.filename.clone_from(filename);
            }
        }
    }
    diagnostics.append(&mut analyzed);
    let diagnostics = sorted(diagnostics);
    let runtime_modules = if diagnostics.is_empty() && mode == AnalysisMode::Runtime {
        let mut hierarchy_diagnostics = Vec::new();
        let hierarchy =
            crate::exceptions::ExceptionHierarchy::analyze(&flattened, &mut hierarchy_diagnostics);
        if !hierarchy_diagnostics.is_empty() {
            return Err("A validated project produced an invalid exception hierarchy".to_owned());
        }
        modules
            .iter()
            .map(|(name, module)| {
                let aliases = exception_aliases(name, module, &modules);
                let hierarchy = hierarchy.with_aliases(aliases);
                Ok((
                    name.clone(),
                    crate::runtime_plan::build_module_with_exceptions(module, &hierarchy)?,
                ))
            })
            .collect::<Result<BTreeMap<_, _>, String>>()?
    } else {
        BTreeMap::new()
    };
    Ok(crate::ProjectRuntimeAnalysis {
        modules: runtime_modules,
        diagnostics,
    })
}

fn exception_aliases(
    module_name: &str,
    module: &Module,
    modules: &BTreeMap<String, Module>,
) -> Vec<(String, efct_model::ExceptionId)> {
    let mut aliases = Vec::new();
    for exception in &module.exceptions {
        aliases.push((
            exception.name.clone(),
            efct_model::ExceptionId::parse(&qualify(module_name, &exception.name))
                .expect("project exception names are valid"),
        ));
    }
    for import in &module.imports {
        match import {
            Import::Symbol {
                module: imported,
                name,
                binding,
                ..
            } if modules
                .get(imported)
                .is_some_and(|module| module.exceptions.iter().any(|item| item.name == *name)) =>
            {
                aliases.push((
                    binding.clone(),
                    efct_model::ExceptionId::parse(&qualify(imported, name))
                        .expect("project exception names are valid"),
                ));
            }
            Import::Module { path, binding, .. } => {
                if let Some(imported) = modules.get(path) {
                    aliases.extend(imported.exceptions.iter().map(|exception| {
                        (
                            format!("{binding}.{}", exception.name),
                            efct_model::ExceptionId::parse(&qualify(path, &exception.name))
                                .expect("project exception names are valid"),
                        )
                    }));
                }
            }
            Import::Symbol { .. } => {}
        }
    }
    aliases
}

fn prepare_modules(
    modules: Vec<efct_protocol::ProjectModule>,
) -> Result<Vec<(String, crate::PreparedModule)>, Vec<Diagnostic>> {
    modules
        .into_iter()
        .map(|source| {
            let filename = source.envelope.filename.clone();
            crate::prepare(source.envelope)
                .map(|module| (source.name, module))
                .map_err(|message| vec![Diagnostic::error("P0002", filename, None, None, message)])
        })
        .collect()
}

fn validate_import_graph(
    modules: &BTreeMap<String, Module>,
    externals: &[external::ExternalDefinition],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut graph = DependencyGraph::new();
    for (name, module) in modules {
        let mut dependencies = BTreeSet::new();
        for import in &module.imports {
            let (path, span) = match import {
                Import::Module { path, span, .. } => (path, span),
                Import::Symbol { module, span, .. } => (module, span),
            };
            if is_system_import(path) {
                continue;
            }
            if !modules.contains_key(path)
                && !externals.iter().any(|external| {
                    external.path.starts_with(&format!("{path}."))
                        || matches!(import, Import::Symbol { name, .. } if external.path == format!("{path}.{name}"))
                })
            {
                diagnostics.push(Diagnostic::error(
                    "P1301",
                    module.filename.clone(),
                    Some(*span),
                    None,
                    format!("Module {path} cannot be resolved in the checked project"),
                ));
            } else if modules.contains_key(path) {
                dependencies.insert(path.clone());
            }
        }
        graph.insert(name.clone(), dependencies);
    }
    if diagnostics.is_empty() {
        if let Err(error) = validate_dependency_graph(&graph) {
            let module_name = match error {
                DependencyGraphError::Cycle { module }
                | DependencyGraphError::MissingNode { module } => module,
            };
            diagnostics.push(Diagnostic::error(
                "P1301",
                modules.get(&module_name).map_or_else(
                    || "<efct-project>".to_owned(),
                    |module| module.filename.clone(),
                ),
                None,
                None,
                format!(
                    "Cyclic or missing dependencies are not allowed; module involved: {module_name}"
                ),
            ));
        }
    }
}

fn flatten_modules(modules: BTreeMap<String, Module>) -> Module {
    let mut flattened = Module {
        filename: "<efct-project>".to_owned(),
        source_sha256: String::new(),
        imports: Vec::new(),
        constants: Vec::new(),
        records: Vec::new(),
        exceptions: Vec::new(),
        functions: Vec::new(),
    };
    for (module_name, module) in modules {
        let bindings = import_bindings(&module);
        let local_functions: BTreeSet<String> = module
            .functions
            .iter()
            .map(|item| item.name.clone())
            .collect();
        let local_constants: BTreeSet<String> = module
            .constants
            .iter()
            .map(|item| item.name.clone())
            .collect();
        let local_exceptions: BTreeSet<String> = module
            .exceptions
            .iter()
            .map(|item| item.name.clone())
            .collect();
        let local_values: BTreeSet<String> =
            local_constants.union(&local_exceptions).cloned().collect();
        if module_name == "__single__" {
            flattened.filename.clone_from(&module.filename);
            flattened.source_sha256.clone_from(&module.source_sha256);
        }
        if module.imports.iter().any(|item| {
            matches!(item, Import::Module { path, .. } if path == "efct")
                || matches!(item, Import::Symbol { module, .. } if module == "efct")
        }) && !flattened
            .imports
            .iter()
            .any(|item| matches!(item, Import::Module { path, .. } if path == "efct"))
        {
            flattened.imports.push(Import::Module {
                path: "efct".to_owned(),
                binding: "efct".to_owned(),
                span: SourceSpan {
                    start_line: 1,
                    start_utf8_byte: 0,
                    end_line: 1,
                    end_utf8_byte: 0,
                },
            });
        }
        flattened
            .constants
            .extend(module.constants.into_iter().map(|constant| {
                transform_constant(
                    constant,
                    &module_name,
                    &bindings,
                    &local_functions,
                    &local_values,
                )
            }));
        flattened
            .records
            .extend(module.records.into_iter().map(|record| {
                transform_record(
                    record,
                    &module_name,
                    &bindings,
                    &local_functions,
                    &local_values,
                )
            }));
        flattened
            .exceptions
            .extend(module.exceptions.into_iter().map(|exception| {
                transform_exception(
                    exception,
                    &module_name,
                    &bindings,
                    &local_functions,
                    &local_values,
                )
            }));
        flattened
            .functions
            .extend(module.functions.into_iter().map(|function| {
                transform_function(
                    function,
                    &module_name,
                    &bindings,
                    &local_functions,
                    &local_values,
                    &local_exceptions,
                )
            }));
    }
    flattened
}

fn transform_exception(
    mut exception: ExceptionDefinition,
    module: &str,
    bindings: &BTreeMap<String, ImportBinding>,
    functions: &BTreeSet<String>,
    values: &BTreeSet<String>,
) -> ExceptionDefinition {
    exception.name = qualify(module, &exception.name);
    transform_exception_reference(&mut exception.base, bindings);
    transform_expression(&mut exception.base, module, bindings, functions, values);
    exception
}

fn transform_record(
    mut record: RecordDefinition,
    module: &str,
    bindings: &BTreeMap<String, ImportBinding>,
    functions: &BTreeSet<String>,
    constants: &BTreeSet<String>,
) -> RecordDefinition {
    for field in &mut record.fields {
        transform_expression(
            &mut field.annotation,
            module,
            bindings,
            functions,
            constants,
        );
    }
    record
}

fn import_bindings(module: &Module) -> BTreeMap<String, ImportBinding> {
    module
        .imports
        .iter()
        .map(|import| match import {
            Import::Module { path, binding, .. } => {
                (binding.clone(), ImportBinding::Module(path.clone()))
            }
            Import::Symbol {
                module,
                name,
                binding,
                ..
            } => (
                binding.clone(),
                ImportBinding::Symbol(qualify(module, name)),
            ),
        })
        .collect()
}

fn transform_constant(
    mut constant: ConstantDefinition,
    module: &str,
    bindings: &BTreeMap<String, ImportBinding>,
    functions: &BTreeSet<String>,
    constants: &BTreeSet<String>,
) -> ConstantDefinition {
    constant.name = qualify(module, &constant.name);
    transform_expression(
        &mut constant.annotation,
        module,
        bindings,
        functions,
        constants,
    );
    transform_expression(&mut constant.value, module, bindings, functions, constants);
    constant
}

fn transform_function(
    mut function: Function,
    module: &str,
    bindings: &BTreeMap<String, ImportBinding>,
    functions: &BTreeSet<String>,
    constants: &BTreeSet<String>,
    exceptions: &BTreeSet<String>,
) -> Function {
    function.name = qualify(module, &function.name);
    match &mut function.declaration {
        crate::hir::FunctionDeclaration::BoundedPure(values)
        | crate::hir::FunctionDeclaration::BoundedEffects(values) => {
            for value in values {
                if value.notation != DeclarationNotation::Typed {
                    continue;
                }
                let Some((prefix, exception)) = value
                    .name
                    .strip_prefix("raise:")
                    .map(|exception| ("raise:", exception))
                    .or_else(|| {
                        value
                            .name
                            .strip_prefix("raise-group:")
                            .map(|exception| ("raise-group:", exception))
                    })
                else {
                    continue;
                };
                if exceptions.contains(exception) {
                    value.name = format!("{prefix}{}", qualify(module, exception));
                } else if let Some((root, suffix)) = exception.split_once('.')
                    && let Some(ImportBinding::Module(imported)) = bindings.get(root)
                {
                    value.name = format!("{prefix}{imported}.{suffix}");
                } else if let Some(ImportBinding::Symbol(imported)) = bindings.get(exception) {
                    value.name = format!("{prefix}{imported}");
                }
            }
        }
        crate::hir::FunctionDeclaration::InferredPure
        | crate::hir::FunctionDeclaration::InferredEffects => {}
    }
    for parameter in &mut function.parameters {
        if let Some(annotation) = &mut parameter.annotation {
            transform_expression(annotation, module, bindings, functions, constants);
        }
    }
    if let Some(returns) = &mut function.returns {
        transform_expression(returns, module, bindings, functions, constants);
    }
    transform_statements(&mut function.body, module, bindings, functions, constants);
    function
}

fn transform_statements(
    statements: &mut [Statement],
    module: &str,
    bindings: &BTreeMap<String, ImportBinding>,
    functions: &BTreeSet<String>,
    constants: &BTreeSet<String>,
) {
    for statement in statements {
        match statement {
            Statement::ModuleImport { .. } => {}
            Statement::Return { value, .. } => {
                if let Some(value) = value {
                    transform_expression(value, module, bindings, functions, constants);
                }
            }
            Statement::Assign { target, value, .. }
            | Statement::AugmentedAssignment { target, value, .. } => {
                transform_expression(target, module, bindings, functions, constants);
                transform_expression(value, module, bindings, functions, constants);
            }
            Statement::AnnotatedAssignment {
                target,
                annotation,
                value,
                ..
            } => {
                transform_expression(target, module, bindings, functions, constants);
                transform_expression(annotation, module, bindings, functions, constants);
                if let Some(value) = value {
                    transform_expression(value, module, bindings, functions, constants);
                }
            }
            Statement::Expression { value, .. } => {
                transform_expression(value, module, bindings, functions, constants);
            }
            Statement::If {
                condition,
                body,
                otherwise,
                ..
            }
            | Statement::While {
                condition,
                body,
                otherwise,
                ..
            } => {
                transform_expression(condition, module, bindings, functions, constants);
                transform_statements(body, module, bindings, functions, constants);
                transform_statements(otherwise, module, bindings, functions, constants);
            }
            Statement::For {
                target,
                iterable,
                body,
                otherwise,
                ..
            } => {
                transform_expression(target, module, bindings, functions, constants);
                transform_expression(iterable, module, bindings, functions, constants);
                transform_statements(body, module, bindings, functions, constants);
                transform_statements(otherwise, module, bindings, functions, constants);
            }
            Statement::Match { subject, cases, .. } => {
                transform_expression(subject, module, bindings, functions, constants);
                for case in cases {
                    transform_pattern(&mut case.pattern, module, bindings, functions, constants);
                    transform_statements(&mut case.body, module, bindings, functions, constants);
                }
            }
            Statement::Try {
                body,
                handlers,
                otherwise,
                finalizer,
                ..
            } => {
                transform_statements(body, module, bindings, functions, constants);
                for handler in match handlers {
                    crate::hir::ExceptionHandlers::Standard(handlers)
                    | crate::hir::ExceptionHandlers::Group(handlers) => handlers,
                } {
                    let (first, remaining) = handler.selector.parts_mut();
                    for exception in std::iter::once(first).chain(remaining) {
                        transform_exception_reference(exception, bindings);
                        transform_expression(exception, module, bindings, functions, constants);
                    }
                    transform_statements(&mut handler.body, module, bindings, functions, constants);
                }
                transform_statements(otherwise, module, bindings, functions, constants);
                transform_statements(finalizer, module, bindings, functions, constants);
            }
            Statement::With { items, body, .. } => {
                for item in items {
                    let (context, target) = match item {
                        crate::hir::WithItem::Unbound { context } => (context, None),
                        crate::hir::WithItem::Bound { context, target } => (context, Some(target)),
                    };
                    if let Expression::Call { arguments, .. } = context {
                        for argument in arguments {
                            transform_exception_reference(argument, bindings);
                        }
                    }
                    transform_expression(context, module, bindings, functions, constants);
                    if let Some(target) = target {
                        transform_expression(target, module, bindings, functions, constants);
                    }
                }
                transform_statements(body, module, bindings, functions, constants);
            }
            Statement::Raise {
                exception, cause, ..
            } => {
                if let Some(exception) = exception {
                    transform_exception_reference(exception, bindings);
                    transform_expression(exception, module, bindings, functions, constants);
                }
                if let RaiseCause::Explicit(cause) = cause {
                    transform_exception_reference(cause, bindings);
                    transform_expression(cause, module, bindings, functions, constants);
                }
            }
            Statement::Assert {
                condition, message, ..
            } => {
                transform_expression(condition, module, bindings, functions, constants);
                if let Some(message) = message {
                    transform_expression(message, module, bindings, functions, constants);
                }
            }
            Statement::Break(_) | Statement::Continue(_) | Statement::Pass(_) => {}
        }
    }
}

fn transform_pattern(
    pattern: &mut Pattern,
    module: &str,
    bindings: &BTreeMap<String, ImportBinding>,
    functions: &BTreeSet<String>,
    constants: &BTreeSet<String>,
) {
    if let Pattern::Class {
        class, positional, ..
    } = pattern
    {
        transform_expression(class, module, bindings, functions, constants);
        for pattern in positional {
            transform_pattern(pattern, module, bindings, functions, constants);
        }
    }
}

fn transform_expression(
    expression: &mut Expression,
    module: &str,
    bindings: &BTreeMap<String, ImportBinding>,
    functions: &BTreeSet<String>,
    constants: &BTreeSet<String>,
) {
    if let Expression::Name { identifier, .. } = expression {
        if functions.contains(identifier) || constants.contains(identifier) {
            *identifier = qualify(module, identifier);
        } else if let Some(ImportBinding::Symbol(target)) = bindings.get(identifier) {
            identifier.clone_from(target);
        }
        return;
    }
    if let Expression::Call { callee, .. } = expression
        && let Some(target) = resolve_module_member(callee, bindings)
    {
        let span = callee.span();
        **callee = Expression::Name {
            identifier: target,
            span,
        };
    }
    match expression {
        Expression::Tuple { elements, .. }
        | Expression::List { elements, .. }
        | Expression::Boolean {
            values: elements, ..
        } => {
            for item in elements {
                transform_expression(item, module, bindings, functions, constants);
            }
        }
        Expression::Unary { operand, .. } => {
            transform_expression(operand, module, bindings, functions, constants);
        }
        Expression::Binary { left, right, .. } => {
            transform_expression(left, module, bindings, functions, constants);
            transform_expression(right, module, bindings, functions, constants);
        }
        Expression::Compare {
            left, comparators, ..
        } => {
            transform_expression(left, module, bindings, functions, constants);
            for item in comparators {
                transform_expression(item, module, bindings, functions, constants);
            }
        }
        Expression::Conditional {
            condition,
            then_value,
            else_value,
            ..
        } => {
            transform_expression(condition, module, bindings, functions, constants);
            transform_expression(then_value, module, bindings, functions, constants);
            transform_expression(else_value, module, bindings, functions, constants);
        }
        Expression::Call {
            callee, arguments, ..
        } => {
            transform_expression(callee, module, bindings, functions, constants);
            for item in arguments {
                transform_expression(item, module, bindings, functions, constants);
            }
        }
        Expression::Attribute { value, .. } => {
            transform_expression(value, module, bindings, functions, constants);
        }
        Expression::Subscript { value, slice, .. } => {
            transform_expression(value, module, bindings, functions, constants);
            transform_expression(slice, module, bindings, functions, constants);
        }
        Expression::Name { .. } | Expression::Constant { .. } => {}
    }
}

fn transform_exception_reference(
    expression: &mut Expression,
    bindings: &BTreeMap<String, ImportBinding>,
) {
    let Some(target) = resolve_module_member(expression, bindings) else {
        return;
    };
    let span = expression.span();
    *expression = Expression::Name {
        identifier: target,
        span,
    };
}

fn resolve_module_member(
    expression: &Expression,
    bindings: &BTreeMap<String, ImportBinding>,
) -> Option<String> {
    let qualified = qualified_name(expression)?;
    let (root, suffix) = qualified.split_once('.')?;
    match bindings.get(root)? {
        ImportBinding::Module(module) => Some(format!("{module}.{suffix}")),
        ImportBinding::Symbol(_) => None,
    }
}

fn qualified_name(expression: &Expression) -> Option<String> {
    match expression {
        Expression::Name { identifier, .. } => Some(identifier.clone()),
        Expression::Attribute { value, name, .. } => {
            Some(format!("{}.{}", qualified_name(value)?, name))
        }
        _ => None,
    }
}

fn qualify(module: &str, symbol: &str) -> String {
    if module == "__single__" {
        symbol.to_owned()
    } else {
        format!("{module}.{symbol}")
    }
}

fn is_system_import(name: &str) -> bool {
    crate::python_import_role(name).is_some()
}

fn sorted(mut diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
    diagnostics.sort_by(|left, right| {
        let position = |span: Option<SourceSpan>| {
            span.map(|value| {
                (
                    value.start_line,
                    value.start_utf8_byte,
                    value.end_line,
                    value.end_utf8_byte,
                )
            })
        };
        left.filename
            .cmp(&right.filename)
            .then_with(|| position(left.span).cmp(&position(right.span)))
            .then_with(|| left.code.cmp(right.code))
    });
    diagnostics
}
