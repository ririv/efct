use std::collections::BTreeMap;

use efct_model::{Diagnostic, Effect, EffectFormula, EffectVariable, PartialBehavior};
use efct_protocol::{BinaryOperator, ConstantValue};

use crate::exceptions::ExceptionHierarchy;
use crate::hir::{Expression, Function, FunctionDeclaration, Import, Module};
use crate::types::Type;

use super::FunctionSignature;

pub(super) fn analyze_signatures(
    module: &Module,
    records: &BTreeMap<String, Type>,
    exceptions: &ExceptionHierarchy,
    diagnostics: &mut Vec<Diagnostic>,
) -> BTreeMap<String, FunctionSignature> {
    let mut signatures = BTreeMap::new();
    for function in &module.functions {
        if signatures.contains_key(&function.name) {
            diagnostics.push(Diagnostic::error(
                "P1004",
                module.filename.clone(),
                Some(function.span),
                Some(function.name.clone()),
                format!("Function {} is defined more than once", function.name),
            ));
            continue;
        }
        let effect_parameters: BTreeMap<String, EffectVariable> = function
            .effect_parameters
            .iter()
            .map(|parameter| {
                (
                    parameter.name.clone(),
                    EffectVariable::new(function.name.clone(), parameter.name.clone()),
                )
            })
            .collect();
        let mut parameter_types = Vec::with_capacity(function.parameters.len());
        let mut valid = true;
        for parameter in &function.parameters {
            let Some(annotation) = &parameter.annotation else {
                diagnostics.push(Diagnostic::error(
                    "P1101",
                    module.filename.clone(),
                    Some(parameter.span),
                    Some(function.name.clone()),
                    format!("Parameter {} is missing a type annotation", parameter.name),
                ));
                valid = false;
                continue;
            };
            let Some(parameter_type) = resolve_type_with_effects(
                annotation,
                &module.filename,
                Some(&function.name),
                records,
                &module.imports,
                &effect_parameters,
                diagnostics,
            ) else {
                valid = false;
                continue;
            };
            if !parameter_type.is_boundary_value() {
                diagnostics.push(Diagnostic::error(
                    "P1201",
                    module.filename.clone(),
                    Some(annotation.span()),
                    Some(function.name.clone()),
                    format!(
                        "The type of parameter {} cannot be used at a function boundary",
                        parameter.name
                    ),
                ));
                valid = false;
            }
            parameter_types.push(parameter_type);
        }
        let Some(return_annotation) = &function.returns else {
            diagnostics.push(Diagnostic::error(
                "P1102",
                module.filename.clone(),
                Some(function.span),
                Some(function.name.clone()),
                "The function is missing a return type annotation",
            ));
            continue;
        };
        let Some(return_type) = resolve_type_with_effects(
            return_annotation,
            &module.filename,
            Some(&function.name),
            records,
            &module.imports,
            &effect_parameters,
            diagnostics,
        ) else {
            continue;
        };
        if !return_type.is_boundary_value() {
            diagnostics.push(Diagnostic::error(
                "P1201",
                module.filename.clone(),
                Some(return_annotation.span()),
                Some(function.name.clone()),
                "The return type cannot be used at a function boundary",
            ));
            valid = false;
        }
        let Some(declared_effects) = parse_declared_effects(
            function,
            &module.filename,
            &effect_parameters,
            exceptions,
            diagnostics,
        ) else {
            continue;
        };
        if valid {
            signatures.insert(
                function.name.clone(),
                FunctionSignature {
                    declaration: function.declaration.clone(),
                    parameters: parameter_types,
                    returns: return_type,
                    effect_parameters,
                    declared_effects,
                },
            );
        }
    }
    signatures
}

pub(super) fn parse_declared_effects(
    function: &Function,
    filename: &str,
    effect_parameters: &BTreeMap<String, EffectVariable>,
    exceptions: &ExceptionHierarchy,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<EffectFormula> {
    match &function.declaration {
        FunctionDeclaration::InferredPure | FunctionDeclaration::InferredEffects => {
            Some(EffectFormula::new())
        }
        FunctionDeclaration::BoundedPure(values) | FunctionDeclaration::BoundedEffects(values) => {
            let mut effects = EffectFormula::new();
            let mut valid = true;
            for value in values {
                match Effect::parse(&value.name) {
                    Ok(mut effect) => {
                        if value.notation == crate::hir::DeclarationNotation::String
                            && effect
                                .raised_exception()
                                .is_some_and(|exception| !exception.as_str().contains('.'))
                        {
                            diagnostics.push(Diagnostic::error(
                                "P1006",
                                filename.to_owned(),
                                Some(function.span),
                                Some(function.name.clone()),
                                "A string exception declaration must use a fully qualified registered name",
                            ));
                            valid = false;
                            continue;
                        }
                        if let Some(exception) = effect.raised_exception() {
                            let Some(exception) = exceptions.resolve(exception.as_str()) else {
                                diagnostics.push(Diagnostic::error(
                                    "P1006",
                                    filename.to_owned(),
                                    Some(function.span),
                                    Some(function.name.clone()),
                                    format!("Exception type {effect} is not registered"),
                                ));
                                valid = false;
                                continue;
                            };
                            if exceptions.is_exception_group(&exception) {
                                diagnostics.push(Diagnostic::error(
                                    "P1006",
                                    filename.to_owned(),
                                    Some(function.span),
                                    Some(function.name.clone()),
                                    "ExceptionGroup declarations must name their registered leaf exception types",
                                ));
                                valid = false;
                                continue;
                            }
                            effect = Effect::Partial(match effect {
                                Effect::Partial(PartialBehavior::Raise(_)) => {
                                    PartialBehavior::Raise(exception)
                                }
                                Effect::Partial(PartialBehavior::RaiseGroup(_)) => {
                                    PartialBehavior::RaiseGroup(exception)
                                }
                                Effect::Partial(PartialBehavior::Diverge) => {
                                    unreachable!("diverge does not carry an exception")
                                }
                                Effect::Partial(PartialBehavior::Throw) => {
                                    unreachable!(
                                        "JavaScript throw does not carry a Python exception"
                                    )
                                }
                                Effect::External(_) => {
                                    unreachable!("raised_exception only returns partial behavior")
                                }
                            });
                        }
                        if matches!(function.declaration, FunctionDeclaration::BoundedPure(_))
                            && !effect.is_partial()
                        {
                            diagnostics.push(Diagnostic::error(
                                "P1006",
                                filename.to_owned(),
                                Some(function.span),
                                Some(function.name.clone()),
                                format!("A pure contract cannot declare external effect {effect}"),
                            ));
                            valid = false;
                            continue;
                        }
                        if !effects.insert(effect) {
                            diagnostics.push(Diagnostic::error(
                                "P1006",
                                filename.to_owned(),
                                Some(function.span),
                                Some(function.name.clone()),
                                format!("Declaration {} appears more than once", value.name),
                            ));
                            valid = false;
                        }
                    }
                    Err(error) => {
                        diagnostics.push(Diagnostic::error(
                            "P1006",
                            filename.to_owned(),
                            Some(function.span),
                            Some(function.name.clone()),
                            error.to_string(),
                        ));
                        valid = false;
                    }
                }
            }
            if matches!(function.declaration, FunctionDeclaration::BoundedEffects(_)) {
                for variable in effect_parameters.values() {
                    effects.insert_variable(variable.clone());
                }
            }
            valid.then_some(effects)
        }
    }
}

pub(super) fn resolve_type(
    expression: &Expression,
    filename: &str,
    function: Option<&str>,
    records: &BTreeMap<String, Type>,
    imports: &[Import],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Type> {
    resolve_type_with_effects(
        expression,
        filename,
        function,
        records,
        imports,
        &BTreeMap::new(),
        diagnostics,
    )
}

pub(super) fn resolve_type_with_effects(
    expression: &Expression,
    filename: &str,
    function: Option<&str>,
    records: &BTreeMap<String, Type>,
    imports: &[Import],
    effect_parameters: &BTreeMap<String, EffectVariable>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Type> {
    let resolved = match expression {
        Expression::Name { identifier, .. } => match identifier.as_str() {
            "bool" => Some(Type::Bool),
            "int" => Some(Type::Int),
            "str" => Some(Type::Str),
            "bytes" => Some(Type::Bytes),
            "Any" => {
                diagnostics.push(Diagnostic::error(
                    "P1103",
                    filename.to_owned(),
                    Some(expression.span()),
                    function.map(str::to_owned),
                    "Any is not allowed",
                ));
                return None;
            }
            "object" | "list" | "dict" | "set" | "bytearray" => {
                diagnostics.push(Diagnostic::error(
                    "P1201",
                    filename.to_owned(),
                    Some(expression.span()),
                    function.map(str::to_owned),
                    format!("Mutable or imprecise type {identifier} is not allowed"),
                ));
                return None;
            }
            _ => records.get(identifier).cloned(),
        },
        Expression::Constant {
            value: ConstantValue::None,
            ..
        } => Some(Type::None),
        Expression::Binary {
            operator: BinaryOperator::BitOr,
            left,
            right,
            ..
        } => {
            let left = resolve_type_with_effects(
                left,
                filename,
                function,
                records,
                imports,
                effect_parameters,
                diagnostics,
            )?;
            let right = resolve_type_with_effects(
                right,
                filename,
                function,
                records,
                imports,
                effect_parameters,
                diagnostics,
            )?;
            let element = match (left, right) {
                (Type::None, element) | (element, Type::None) => element,
                _ => {
                    diagnostics.push(Diagnostic::error(
                        "P1104",
                        filename.to_owned(),
                        Some(expression.span()),
                        function.map(str::to_owned),
                        "Only a union of one type and None is supported",
                    ));
                    return None;
                }
            };
            if matches!(element, Type::None | Type::Option(_)) {
                diagnostics.push(Diagnostic::error(
                    "P1104",
                    filename.to_owned(),
                    Some(expression.span()),
                    function.map(str::to_owned),
                    "An optional type must contain exactly one non-None type",
                ));
                return None;
            }
            Some(Type::Option(Box::new(element)))
        }
        Expression::Attribute { value, name, .. }
            if qualified_name(value).as_deref() == Some("typing") && name == "Any" =>
        {
            diagnostics.push(Diagnostic::error(
                "P1103",
                filename.to_owned(),
                Some(expression.span()),
                function.map(str::to_owned),
                "typing.Any is not allowed",
            ));
            return None;
        }
        Expression::Subscript { value, slice, .. }
            if qualified_name(value).as_deref() == Some("tuple") =>
        {
            match slice.as_ref() {
                Expression::Tuple { elements, .. }
                    if elements.len() == 2 && is_ellipsis(&elements[1]) =>
                {
                    resolve_type_with_effects(
                        &elements[0],
                        filename,
                        function,
                        records,
                        imports,
                        effect_parameters,
                        diagnostics,
                    )
                    .map(|element| Type::TupleVariadic(Box::new(element)))
                }
                Expression::Tuple { elements, .. } => elements
                    .iter()
                    .map(|element| {
                        resolve_type_with_effects(
                            element,
                            filename,
                            function,
                            records,
                            imports,
                            effect_parameters,
                            diagnostics,
                        )
                    })
                    .collect::<Option<Vec<_>>>()
                    .map(Type::TupleFixed),
                other => resolve_type_with_effects(
                    other,
                    filename,
                    function,
                    records,
                    imports,
                    effect_parameters,
                    diagnostics,
                )
                .map(|element| Type::TupleFixed(vec![element])),
            }
        }
        Expression::Subscript { value, slice, .. }
            if qualified_name(value).as_deref() == Some("frozenset") =>
        {
            resolve_type_with_effects(
                slice,
                filename,
                function,
                records,
                imports,
                effect_parameters,
                diagnostics,
            )
            .map(|element| Type::FrozenSet(Box::new(element)))
        }
        Expression::Subscript { value, slice, .. }
            if matches!(
                qualified_name(value).as_deref(),
                Some("efct.FrozenMap" | "efct.Result")
            ) =>
        {
            let name = qualified_name(value)?;
            let Expression::Tuple { elements, .. } = slice.as_ref() else {
                diagnostics.push(Diagnostic::error(
                    "P1104",
                    filename.to_owned(),
                    Some(slice.span()),
                    function.map(str::to_owned),
                    format!("{name} requires two type arguments"),
                ));
                return None;
            };
            if elements.len() != 2 {
                diagnostics.push(Diagnostic::error(
                    "P1104",
                    filename.to_owned(),
                    Some(slice.span()),
                    function.map(str::to_owned),
                    format!("{name} requires two type arguments"),
                ));
                return None;
            }
            let first = resolve_type_with_effects(
                &elements[0],
                filename,
                function,
                records,
                imports,
                effect_parameters,
                diagnostics,
            )?;
            let second = resolve_type_with_effects(
                &elements[1],
                filename,
                function,
                records,
                imports,
                effect_parameters,
                diagnostics,
            )?;
            if name == "efct.FrozenMap" {
                Some(Type::FrozenMap(Box::new(first), Box::new(second)))
            } else {
                Some(Type::Result(Box::new(first), Box::new(second)))
            }
        }
        Expression::Subscript { value, slice, .. }
            if is_imported_typing_symbol(value, imports, "Optional") =>
        {
            let element = resolve_type_with_effects(
                slice,
                filename,
                function,
                records,
                imports,
                effect_parameters,
                diagnostics,
            )?;
            if matches!(element, Type::None | Type::Option(_)) {
                diagnostics.push(Diagnostic::error(
                    "P1104",
                    filename.to_owned(),
                    Some(expression.span()),
                    function.map(str::to_owned),
                    "An optional type must contain exactly one non-None type",
                ));
                return None;
            }
            Some(Type::Option(Box::new(element)))
        }
        Expression::Subscript { value, slice, .. }
            if qualified_name(value).as_deref() == Some("efct.PureCallable") =>
        {
            let Expression::Tuple { elements, .. } = slice.as_ref() else {
                diagnostics.push(Diagnostic::error(
                    "P1104",
                    filename.to_owned(),
                    Some(slice.span()),
                    function.map(str::to_owned),
                    "efct.PureCallable requires a parameter type list and a return type",
                ));
                return None;
            };
            if elements.len() != 2 {
                diagnostics.push(Diagnostic::error(
                    "P1104",
                    filename.to_owned(),
                    Some(slice.span()),
                    function.map(str::to_owned),
                    "efct.PureCallable requires two type arguments",
                ));
                return None;
            }
            let Expression::List {
                elements: parameter_nodes,
                ..
            } = &elements[0]
            else {
                diagnostics.push(Diagnostic::error(
                    "P1104",
                    filename.to_owned(),
                    Some(elements[0].span()),
                    function.map(str::to_owned),
                    "The first argument to efct.PureCallable must be a type list",
                ));
                return None;
            };
            let parameters = parameter_nodes
                .iter()
                .map(|parameter| {
                    resolve_type_with_effects(
                        parameter,
                        filename,
                        function,
                        records,
                        imports,
                        effect_parameters,
                        diagnostics,
                    )
                })
                .collect::<Option<Vec<_>>>()?;
            let returns = resolve_type_with_effects(
                &elements[1],
                filename,
                function,
                records,
                imports,
                effect_parameters,
                diagnostics,
            )?;
            if parameters
                .iter()
                .any(|parameter| !parameter.is_boundary_value())
                || !returns.is_boundary_value()
            {
                diagnostics.push(Diagnostic::error(
                    "P1201",
                    filename.to_owned(),
                    Some(expression.span()),
                    function.map(str::to_owned),
                    "PureCallable parameter and return types must be valid at function boundaries",
                ));
                return None;
            }
            Some(Type::PureCallable {
                parameters,
                returns: Box::new(returns),
            })
        }
        Expression::Subscript { value, slice, .. }
            if qualified_name(value).as_deref() == Some("efct.EffectCallable") =>
        {
            let Expression::Tuple { elements, .. } = slice.as_ref() else {
                diagnostics.push(Diagnostic::error(
                    "P1104",
                    filename.to_owned(),
                    Some(slice.span()),
                    function.map(str::to_owned),
                    "efct.EffectCallable requires a parameter type list, a return type, and an effect variable",
                ));
                return None;
            };
            if elements.len() != 3 {
                diagnostics.push(Diagnostic::error(
                    "P1104",
                    filename.to_owned(),
                    Some(slice.span()),
                    function.map(str::to_owned),
                    "efct.EffectCallable requires three type arguments",
                ));
                return None;
            }
            let Expression::List {
                elements: parameter_nodes,
                ..
            } = &elements[0]
            else {
                diagnostics.push(Diagnostic::error(
                    "P1104",
                    filename.to_owned(),
                    Some(elements[0].span()),
                    function.map(str::to_owned),
                    "The first argument to efct.EffectCallable must be a type list",
                ));
                return None;
            };
            let parameters = parameter_nodes
                .iter()
                .map(|parameter| {
                    resolve_type_with_effects(
                        parameter,
                        filename,
                        function,
                        records,
                        imports,
                        effect_parameters,
                        diagnostics,
                    )
                })
                .collect::<Option<Vec<_>>>()?;
            let returns = resolve_type_with_effects(
                &elements[1],
                filename,
                function,
                records,
                imports,
                effect_parameters,
                diagnostics,
            )?;
            let Expression::Name {
                identifier: effect_name,
                ..
            } = &elements[2]
            else {
                diagnostics.push(Diagnostic::error(
                    "P1104",
                    filename.to_owned(),
                    Some(elements[2].span()),
                    function.map(str::to_owned),
                    "The EffectCallable effect argument must be a declared effect variable",
                ));
                return None;
            };
            let Some(variable) = effect_parameters.get(effect_name) else {
                diagnostics.push(Diagnostic::error(
                    "P1104",
                    filename.to_owned(),
                    Some(elements[2].span()),
                    function.map(str::to_owned),
                    format!("Effect variable {effect_name} is not declared"),
                ));
                return None;
            };
            if parameters
                .iter()
                .any(|parameter| !parameter.is_boundary_value())
                || !returns.is_boundary_value()
            {
                diagnostics.push(Diagnostic::error(
                    "P1201",
                    filename.to_owned(),
                    Some(expression.span()),
                    function.map(str::to_owned),
                    "EffectCallable parameter and return types must be valid at function boundaries",
                ));
                return None;
            }
            let mut effects = EffectFormula::new();
            effects.insert_variable(variable.clone());
            Some(Type::EffectCallable {
                parameters,
                returns: Box::new(returns),
                effects,
            })
        }
        Expression::Subscript { value, .. } => {
            let name = qualified_name(value).unwrap_or_else(|| "<dynamic type>".to_owned());
            if matches!(name.as_str(), "list" | "dict" | "set") {
                diagnostics.push(Diagnostic::error(
                    "P1201",
                    filename.to_owned(),
                    Some(expression.span()),
                    function.map(str::to_owned),
                    format!("Mutable type {name} is not allowed"),
                ));
                return None;
            }
            None
        }
        _ => None,
    };
    if resolved.is_none() {
        diagnostics.push(Diagnostic::error(
            "P1104",
            filename.to_owned(),
            Some(expression.span()),
            function.map(str::to_owned),
            "The type annotation cannot be resolved",
        ));
    }
    resolved
}

pub(super) fn is_ellipsis(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Constant {
            value: ConstantValue::Ellipsis,
            ..
        }
    )
}

pub(super) fn qualified_name(expression: &Expression) -> Option<String> {
    match expression {
        Expression::Name { identifier, .. } => Some(identifier.clone()),
        Expression::Attribute { value, name, .. } => {
            Some(format!("{}.{}", qualified_name(value)?, name))
        }
        _ => None,
    }
}

fn is_imported_typing_symbol(expression: &Expression, imports: &[Import], symbol: &str) -> bool {
    match expression {
        Expression::Name { identifier, .. } => imports.iter().any(|import| {
            matches!(
                import,
                Import::Symbol {
                    module,
                    name,
                    binding,
                    ..
                } if module == "typing" && name == symbol && binding == identifier
            )
        }),
        Expression::Attribute { value, name, .. } if name == symbol => {
            let Expression::Name { identifier, .. } = value.as_ref() else {
                return false;
            };
            imports.iter().any(|import| {
                matches!(
                    import,
                    Import::Module { path, binding, .. }
                        if path == "typing" && binding == identifier
                )
            })
        }
        _ => false,
    }
}
