use std::collections::{BTreeMap, BTreeSet};

use efct_model::EffectTerm;
use efct_protocol::{ModuleItem, ProtocolEnvelope, SourceLanguage};
use serde::Serialize;

use crate::analyzer;
use crate::hir::{
    Expression, Function, FunctionDeclaration, FunctionKind, Import, Module, Pattern, RaiseCause,
    Statement,
};
use crate::types::Type;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuntimePlan {
    pub callable_kind: RuntimeCallableKind,
    pub declared_effects: Vec<String>,
    pub parameter_names: Vec<String>,
    pub parameter_types: Vec<RuntimeType>,
    pub return_type: RuntimeType,
    pub constant_types: Vec<NamedRuntimeType>,
    pub symbol_imports: Vec<SymbolImport>,
    pub module_members: Vec<ModuleMembers>,
    pub exception_bindings: Vec<ExceptionBinding>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCallableKind {
    InferredPure,
    BoundedPure,
    InferredEffect,
    BoundedEffect,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NamedRuntimeType {
    pub name: String,
    pub value_type: RuntimeType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NamedField {
    pub name: String,
    pub value_type: RuntimeType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SymbolImport {
    pub binding: String,
    pub module: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModuleMembers {
    pub binding: String,
    pub module: String,
    pub members: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExceptionBinding {
    pub binding: String,
    pub module: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimeType {
    Scalar {
        name: String,
    },
    TupleFixed {
        elements: Vec<RuntimeType>,
    },
    TupleVariadic {
        element: Box<RuntimeType>,
    },
    FrozenSet {
        element: Box<RuntimeType>,
    },
    FrozenMap {
        key: Box<RuntimeType>,
        value: Box<RuntimeType>,
    },
    Option {
        element: Box<RuntimeType>,
    },
    Result {
        value: Box<RuntimeType>,
        error: Box<RuntimeType>,
    },
    Record {
        name: String,
        fields: Vec<NamedField>,
    },
    PureCallable {
        parameters: Vec<RuntimeType>,
        returns: Box<RuntimeType>,
    },
    EffectCallable {
        parameters: Vec<RuntimeType>,
        returns: Box<RuntimeType>,
        effect_variable: String,
    },
}

pub fn module_imports(envelope: &ProtocolEnvelope) -> Result<Vec<String>, String> {
    let SourceLanguage::Python { root, .. } = &envelope.language else {
        return Err("The Python runtime planner received a non-Python payload".to_owned());
    };
    let mut imports = BTreeSet::new();
    for item in &root.items {
        match item {
            ModuleItem::Import { names, .. } => {
                imports.extend(names.iter().map(|name| name.name.clone()));
            }
            ModuleItem::ImportFrom {
                module: Some(module),
                level: 0,
                ..
            } => {
                imports.insert(module.clone());
            }
            _ => {}
        }
    }
    Ok(imports.into_iter().collect())
}

pub(crate) fn build_module_with_exceptions(
    module: &Module,
    exceptions: &crate::exceptions::ExceptionHierarchy,
) -> Result<BTreeMap<String, RuntimePlan>, String> {
    let runtime_types = analyzer::runtime_types_with_exceptions(module, exceptions, Vec::new())
        .map_err(format_diagnostics)?;
    build_module_with_types_and_exceptions(module, &runtime_types, Some(exceptions))
}

pub(crate) fn build_module_with_types(
    module: &Module,
    runtime_types: &analyzer::RuntimeTypes,
) -> Result<BTreeMap<String, RuntimePlan>, String> {
    build_module_with_types_and_exceptions(module, runtime_types, None)
}

fn build_module_with_types_and_exceptions(
    module: &Module,
    runtime_types: &analyzer::RuntimeTypes,
    exceptions: Option<&crate::exceptions::ExceptionHierarchy>,
) -> Result<BTreeMap<String, RuntimePlan>, String> {
    let constant_types = runtime_types
        .constants
        .iter()
        .map(|(name, value_type)| {
            Ok(NamedRuntimeType {
                name: name.clone(),
                value_type: RuntimeType::from_analyzed(value_type)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let symbol_imports = module
        .imports
        .iter()
        .filter_map(|import| match import {
            Import::Symbol {
                module,
                name,
                binding,
                ..
            } => Some(SymbolImport {
                binding: binding.clone(),
                module: module.clone(),
                name: name.clone(),
            }),
            Import::Module { .. } => None,
        })
        .collect::<Vec<_>>();
    let module_bindings: BTreeMap<&str, &str> = module
        .imports
        .iter()
        .filter_map(|import| match import {
            Import::Module { path, binding, .. } => Some((binding.as_str(), path.as_str())),
            Import::Symbol { .. } => None,
        })
        .collect();
    let mut plans = BTreeMap::new();
    for function in module
        .functions
        .iter()
        .filter(|function| function.kind == FunctionKind::Declared)
    {
        let signature = runtime_types
            .signatures
            .get(&function.name)
            .ok_or_else(|| format!("Function {} has no valid runtime signature", function.name))?;
        let plan = build_function(
            function,
            signature,
            &constant_types,
            &symbol_imports,
            &module_bindings,
            module,
            exceptions,
        )?;
        if plans.insert(function.name.clone(), plan).is_some() {
            return Err(format!(
                "Function {} cannot be uniquely located in the module source",
                function.name
            ));
        }
    }
    Ok(plans)
}

fn build_function(
    function: &Function,
    signature: &analyzer::FunctionSignature,
    constant_types: &[NamedRuntimeType],
    symbol_imports: &[SymbolImport],
    module_bindings: &BTreeMap<&str, &str>,
    module: &Module,
    exceptions: Option<&crate::exceptions::ExceptionHierarchy>,
) -> Result<RuntimePlan, String> {
    let (callable_kind, declared_effects) = match &signature.declaration {
        FunctionDeclaration::InferredPure => (RuntimeCallableKind::InferredPure, Vec::new()),
        FunctionDeclaration::BoundedPure(_) => (
            RuntimeCallableKind::BoundedPure,
            concrete_effect_names(&signature.declared_effects),
        ),
        FunctionDeclaration::InferredEffects => (RuntimeCallableKind::InferredEffect, Vec::new()),
        FunctionDeclaration::BoundedEffects(_) => (
            RuntimeCallableKind::BoundedEffect,
            concrete_effect_names(&signature.declared_effects),
        ),
    };
    let parameter_types = signature
        .parameters
        .iter()
        .map(RuntimeType::from_analyzed)
        .collect::<Result<Vec<_>, _>>()?;
    let return_type = RuntimeType::from_analyzed(&signature.returns)?;
    let mut used_members = BTreeMap::<String, BTreeSet<String>>::new();
    for statement in &function.body {
        collect_statement_members(statement, module_bindings, &mut used_members);
    }
    let module_members: Vec<ModuleMembers> = used_members
        .into_iter()
        .map(|(binding, members)| ModuleMembers {
            module: module_bindings[binding.as_str()].to_owned(),
            binding,
            members: members.into_iter().collect(),
        })
        .collect();
    let exception_bindings = exceptions.map_or_else(Vec::new, |exceptions| {
        let local = module.exceptions.iter().filter_map(|exception| {
            exceptions
                .resolve(&exception.name)
                .and_then(|identifier| exception_binding(exception.name.clone(), &identifier))
        });
        let symbols = module.imports.iter().filter_map(|import| match import {
            Import::Symbol { binding, .. } => exceptions
                .resolve(binding)
                .and_then(|identifier| exception_binding(binding.clone(), &identifier)),
            Import::Module { .. } => None,
        });
        let members = module_members.iter().flat_map(|module| {
            module.members.iter().filter_map(|member| {
                let binding = format!("{}.{}", module.binding, member);
                exceptions
                    .resolve(&binding)
                    .and_then(|identifier| exception_binding(binding, &identifier))
            })
        });
        local.chain(symbols).chain(members).collect()
    });
    Ok(RuntimePlan {
        callable_kind,
        declared_effects,
        parameter_names: function
            .parameters
            .iter()
            .map(|parameter| parameter.name.clone())
            .collect(),
        parameter_types,
        return_type,
        constant_types: constant_types.to_vec(),
        symbol_imports: symbol_imports.to_vec(),
        module_members,
        exception_bindings,
    })
}

fn exception_binding(
    binding: String,
    identifier: &efct_model::ExceptionId,
) -> Option<ExceptionBinding> {
    let (module, name) = identifier.as_str().rsplit_once('.')?;
    (module != "builtins").then(|| ExceptionBinding {
        binding,
        module: module.to_owned(),
        name: name.to_owned(),
    })
}

impl RuntimeType {
    fn from_analyzed(value_type: &Type) -> Result<Self, String> {
        Ok(match value_type {
            Type::Never => {
                return Err(
                    "The internal Never type cannot appear at a runtime boundary".to_owned(),
                );
            }
            Type::None => Self::scalar("None"),
            Type::Bool => Self::scalar("bool"),
            Type::Int => Self::scalar("int"),
            Type::Str => Self::scalar("str"),
            Type::Bytes => Self::scalar("bytes"),
            Type::TupleFixed(elements) => Self::TupleFixed {
                elements: elements
                    .iter()
                    .map(Self::from_analyzed)
                    .collect::<Result<Vec<_>, _>>()?,
            },
            Type::TupleVariadic(element) => Self::TupleVariadic {
                element: Box::new(Self::from_analyzed(element)?),
            },
            Type::FrozenSet(element) => Self::FrozenSet {
                element: Box::new(Self::from_analyzed(element)?),
            },
            Type::FrozenMap(key, value) => Self::FrozenMap {
                key: Box::new(Self::from_analyzed(key)?),
                value: Box::new(Self::from_analyzed(value)?),
            },
            Type::Option(element) => Self::Option {
                element: Box::new(Self::from_analyzed(element)?),
            },
            Type::Result(value, error) => Self::Result {
                value: Box::new(Self::from_analyzed(value)?),
                error: Box::new(Self::from_analyzed(error)?),
            },
            Type::Record { name, fields } => Self::Record {
                name: name.clone(),
                fields: fields
                    .iter()
                    .map(|(name, value_type)| {
                        Ok(NamedField {
                            name: name.clone(),
                            value_type: Self::from_analyzed(value_type)?,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?,
            },
            Type::PureCallable {
                parameters,
                returns,
            } => Self::PureCallable {
                parameters: parameters
                    .iter()
                    .map(Self::from_analyzed)
                    .collect::<Result<Vec<_>, _>>()?,
                returns: Box::new(Self::from_analyzed(returns)?),
            },
            Type::EffectCallable {
                parameters,
                returns,
                effects,
            } => {
                let variables: Vec<&str> = effects
                    .iter()
                    .filter_map(|term| match term {
                        EffectTerm::Variable(variable) => Some(variable.name.as_str()),
                        EffectTerm::Concrete(_) => None,
                    })
                    .collect();
                let [effect_variable] = variables.as_slice() else {
                    return Err(
                        "An EffectCallable runtime type requires exactly one effect variable"
                            .to_owned(),
                    );
                };
                Self::EffectCallable {
                    parameters: parameters
                        .iter()
                        .map(Self::from_analyzed)
                        .collect::<Result<Vec<_>, _>>()?,
                    returns: Box::new(Self::from_analyzed(returns)?),
                    effect_variable: (*effect_variable).to_owned(),
                }
            }
            Type::Ok(_)
            | Type::Err(_)
            | Type::LocalList { .. }
            | Type::Range
            | Type::Exception(_)
            | Type::ExceptionGroup(_)
            | Type::CaughtException(_)
            | Type::External(_) => {
                return Err(format!(
                    "The analyzed type {value_type} cannot be represented at runtime"
                ));
            }
        })
    }

    fn scalar(name: &str) -> Self {
        Self::Scalar {
            name: name.to_owned(),
        }
    }
}

fn collect_statement_members(
    statement: &Statement,
    module_bindings: &BTreeMap<&str, &str>,
    result: &mut BTreeMap<String, BTreeSet<String>>,
) {
    match statement {
        Statement::ModuleImport { .. } => {}
        Statement::Return { value, .. } => {
            if let Some(value) = value {
                collect_expression_members(value, module_bindings, result);
            }
        }
        Statement::Assign { target, value, .. }
        | Statement::AugmentedAssignment { target, value, .. } => {
            collect_expression_members(target, module_bindings, result);
            collect_expression_members(value, module_bindings, result);
        }
        Statement::AnnotatedAssignment {
            target,
            annotation,
            value,
            ..
        } => {
            collect_expression_members(target, module_bindings, result);
            collect_expression_members(annotation, module_bindings, result);
            if let Some(value) = value {
                collect_expression_members(value, module_bindings, result);
            }
        }
        Statement::Expression { value, .. } => {
            collect_expression_members(value, module_bindings, result);
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
            collect_expression_members(condition, module_bindings, result);
            collect_statement_list(body, module_bindings, result);
            collect_statement_list(otherwise, module_bindings, result);
        }
        Statement::For {
            target,
            iterable,
            body,
            otherwise,
            ..
        } => {
            collect_expression_members(target, module_bindings, result);
            collect_expression_members(iterable, module_bindings, result);
            collect_statement_list(body, module_bindings, result);
            collect_statement_list(otherwise, module_bindings, result);
        }
        Statement::Match { subject, cases, .. } => {
            collect_expression_members(subject, module_bindings, result);
            for case in cases {
                collect_pattern_members(&case.pattern, module_bindings, result);
                collect_statement_list(&case.body, module_bindings, result);
            }
        }
        Statement::Try {
            body,
            handlers,
            otherwise,
            finalizer,
            ..
        } => {
            collect_statement_list(body, module_bindings, result);
            for handler in handlers.as_slice() {
                let (first, remaining) = handler.selector.parts();
                for exception in std::iter::once(first).chain(remaining) {
                    collect_expression_members(exception, module_bindings, result);
                }
                collect_statement_list(&handler.body, module_bindings, result);
            }
            collect_statement_list(otherwise, module_bindings, result);
            collect_statement_list(finalizer, module_bindings, result);
        }
        Statement::With { items, body, .. } => {
            for item in items {
                match item {
                    crate::hir::WithItem::Unbound { context } => {
                        collect_expression_members(context, module_bindings, result);
                    }
                    crate::hir::WithItem::Bound { context, target } => {
                        collect_expression_members(context, module_bindings, result);
                        collect_expression_members(target, module_bindings, result);
                    }
                }
            }
            collect_statement_list(body, module_bindings, result);
        }
        Statement::Raise {
            exception, cause, ..
        } => {
            if let Some(exception) = exception {
                collect_expression_members(exception, module_bindings, result);
            }
            if let RaiseCause::Explicit(cause) = cause {
                collect_expression_members(cause, module_bindings, result);
            }
        }
        Statement::Assert {
            condition, message, ..
        } => {
            collect_expression_members(condition, module_bindings, result);
            if let Some(message) = message {
                collect_expression_members(message, module_bindings, result);
            }
        }
        Statement::Break(_) | Statement::Continue(_) | Statement::Pass(_) => {}
    }
}

fn collect_pattern_members(
    pattern: &Pattern,
    module_bindings: &BTreeMap<&str, &str>,
    result: &mut BTreeMap<String, BTreeSet<String>>,
) {
    if let Pattern::Class {
        class, positional, ..
    } = pattern
    {
        collect_expression_members(class, module_bindings, result);
        for pattern in positional {
            collect_pattern_members(pattern, module_bindings, result);
        }
    }
}

fn collect_statement_list(
    statements: &[Statement],
    module_bindings: &BTreeMap<&str, &str>,
    result: &mut BTreeMap<String, BTreeSet<String>>,
) {
    for statement in statements {
        collect_statement_members(statement, module_bindings, result);
    }
}

fn collect_expression_members(
    expression: &Expression,
    module_bindings: &BTreeMap<&str, &str>,
    result: &mut BTreeMap<String, BTreeSet<String>>,
) {
    match expression {
        Expression::Call {
            callee, arguments, ..
        } => {
            if let Expression::Attribute { value, name, .. } = callee.as_ref()
                && let Expression::Name { identifier, .. } = value.as_ref()
                && module_bindings.contains_key(identifier.as_str())
            {
                result
                    .entry(identifier.clone())
                    .or_default()
                    .insert(name.clone());
            }
            collect_expression_members(callee, module_bindings, result);
            for argument in arguments {
                collect_expression_members(argument, module_bindings, result);
            }
        }
        Expression::Attribute { value, name, .. } => {
            if let Expression::Name { identifier, .. } = value.as_ref()
                && module_bindings.contains_key(identifier.as_str())
            {
                result
                    .entry(identifier.clone())
                    .or_default()
                    .insert(name.clone());
            }
            collect_expression_members(value, module_bindings, result);
        }
        Expression::Subscript { value, .. } | Expression::Unary { operand: value, .. } => {
            collect_expression_members(value, module_bindings, result);
            if let Expression::Subscript { slice, .. } = expression {
                collect_expression_members(slice, module_bindings, result);
            }
        }
        Expression::Binary { left, right, .. } => {
            collect_expression_members(left, module_bindings, result);
            collect_expression_members(right, module_bindings, result);
        }
        Expression::Boolean { values, .. }
        | Expression::Tuple {
            elements: values, ..
        }
        | Expression::List {
            elements: values, ..
        } => {
            for value in values {
                collect_expression_members(value, module_bindings, result);
            }
        }
        Expression::Compare {
            left, comparators, ..
        } => {
            collect_expression_members(left, module_bindings, result);
            for comparator in comparators {
                collect_expression_members(comparator, module_bindings, result);
            }
        }
        Expression::Conditional {
            condition,
            then_value,
            else_value,
            ..
        } => {
            collect_expression_members(condition, module_bindings, result);
            collect_expression_members(then_value, module_bindings, result);
            collect_expression_members(else_value, module_bindings, result);
        }
        Expression::Name { .. } | Expression::Constant { .. } => {}
    }
}

fn format_diagnostics(diagnostics: Vec<efct_model::Diagnostic>) -> String {
    diagnostics
        .into_iter()
        .map(|diagnostic| diagnostic.message)
        .collect::<Vec<_>>()
        .join("; ")
}

fn concrete_effect_names(effects: &efct_model::EffectFormula) -> Vec<String> {
    effects
        .iter()
        .filter_map(|term| match term {
            EffectTerm::Concrete(effect) => Some(effect.to_string()),
            EffectTerm::Variable(_) => None,
        })
        .collect()
}
