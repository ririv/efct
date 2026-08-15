use std::collections::{BTreeMap, BTreeSet};

use efct_model::Diagnostic;
use efct_protocol::{ConstantValue, UnaryOperator};

use crate::hir::{ConstantDefinition, Expression, Import, Module};
use crate::types::Type;

use super::FunctionSignature;
use super::signatures::resolve_type;
use super::typing::{is_assignable, type_mismatch};

pub(super) fn analyze_records(
    module: &Module,
    diagnostics: &mut Vec<Diagnostic>,
) -> BTreeMap<String, Type> {
    let mut records = BTreeMap::new();
    for record in &module.records {
        if records.contains_key(&record.name) {
            diagnostics.push(Diagnostic::error(
                "P1201",
                module.filename.clone(),
                Some(record.span),
                None,
                format!("Pure record {} is defined more than once", record.name),
            ));
            continue;
        }
        let mut fields = Vec::new();
        let mut names = BTreeSet::new();
        let mut valid = true;
        for field in &record.fields {
            if !names.insert(field.name.clone()) {
                diagnostics.push(Diagnostic::error(
                    "P1201",
                    module.filename.clone(),
                    Some(field.span),
                    None,
                    format!("Pure record field {} is defined more than once", field.name),
                ));
                valid = false;
                continue;
            }
            let Some(field_type) = resolve_type(
                &field.annotation,
                &module.filename,
                None,
                &records,
                &module.imports,
                diagnostics,
            ) else {
                valid = false;
                continue;
            };
            if !field_type.is_data_value() {
                diagnostics.push(Diagnostic::error(
                    "P1201",
                    module.filename.clone(),
                    Some(field.span),
                    None,
                    format!(
                        "Pure record field {} is not a deeply immutable pure value",
                        field.name
                    ),
                ));
                valid = false;
            }
            fields.push((field.name.clone(), field_type));
        }
        if valid {
            records.insert(
                record.name.clone(),
                Type::Record {
                    name: record.name.clone(),
                    fields,
                },
            );
        }
    }
    records
}

pub(super) fn reject_symbol_collisions(
    module: &Module,
    constants: &BTreeMap<String, Type>,
    records: &BTreeMap<String, Type>,
    signatures: &BTreeMap<String, FunctionSignature>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for name in constants.keys() {
        if signatures.contains_key(name) {
            diagnostics.push(Diagnostic::error(
                "P1004",
                module.filename.clone(),
                module
                    .functions
                    .iter()
                    .find(|function| &function.name == name)
                    .map(|function| function.span),
                Some(name.clone()),
                format!("Module name {name} is defined as both a constant and a function"),
            ));
        }
    }
    for exception in &module.exceptions {
        if constants.contains_key(&exception.name)
            || records.contains_key(&exception.name)
            || signatures.contains_key(&exception.name)
        {
            diagnostics.push(Diagnostic::error(
                "P1004",
                module.filename.clone(),
                Some(exception.span),
                None,
                format!(
                    "Module name {} is defined as both an exception class and another symbol",
                    exception.name
                ),
            ));
        }
    }
}

pub(super) fn validate_imports(module: &Module, diagnostics: &mut Vec<Diagnostic>) {
    let mut has_efct = false;
    for import in &module.imports {
        match import {
            Import::Module { path, binding, .. } if path == "efct" && binding == "efct" => {
                has_efct = true
            }
            Import::Module { path, .. } if path == "typing" => {}
            Import::Symbol { module, name, .. } if module == "typing" && name == "Optional" => {}
            Import::Symbol { module, name, .. }
                if module == "efct" && matches!(name.as_str(), "effect" | "partial") => {}
            Import::Module { path, .. }
                if matches!(
                    crate::python_import_role(path),
                    Some(crate::PythonImportRole::ModeledApi)
                ) && crate::api_model::is_module(path) => {}
            Import::Symbol { module, name, .. }
                if module == "__future__" && name == "annotations" => {}
            Import::Symbol { module, name, .. }
                if module == "dataclasses" && name == "dataclass" => {}
            Import::Symbol { module, name, .. }
                if crate::api_model::is_modeled_symbol(module, name) => {}
            Import::Module { path, span, .. } => diagnostics.push(Diagnostic::error(
                "P1301",
                module.filename.clone(),
                Some(*span),
                None,
                format!("Imported module {path} is not certified by the MVP"),
            )),
            Import::Symbol {
                module: imported,
                name,
                span,
                ..
            } => diagnostics.push(Diagnostic::error(
                "P1301",
                module.filename.clone(),
                Some(*span),
                None,
                format!("Imported symbol {imported}.{name} is not certified by the MVP"),
            )),
        }
    }
    if !module.functions.is_empty() && !has_efct {
        diagnostics.push(Diagnostic::error(
            "P1301",
            module.filename.clone(),
            module.functions.first().map(|function| function.span),
            None,
            "A checked module must use `import efct`",
        ));
    }
}

pub(super) fn analyze_constants(
    module: &Module,
    records: &BTreeMap<String, Type>,
    diagnostics: &mut Vec<Diagnostic>,
) -> BTreeMap<String, Type> {
    let mut constants = BTreeMap::new();
    for constant in &module.constants {
        if constant.name.to_uppercase() != constant.name
            || !constant.name.chars().any(char::is_alphabetic)
        {
            diagnostics.push(Diagnostic::error(
                "P1501",
                module.filename.clone(),
                Some(constant.span),
                None,
                format!(
                    "Module constant {} must use an uppercase name",
                    constant.name
                ),
            ));
            continue;
        }
        let Some(annotation) = resolve_type(
            &constant.annotation,
            &module.filename,
            None,
            records,
            &module.imports,
            diagnostics,
        ) else {
            continue;
        };
        let Some(value_type) = infer_constant_value(constant, &module.filename, diagnostics) else {
            continue;
        };
        if !is_assignable(&annotation, &value_type) {
            diagnostics.push(type_mismatch(
                &module.filename,
                None,
                constant.value.span(),
                &annotation,
                &value_type,
            ));
            continue;
        }
        if constants
            .insert(constant.name.clone(), annotation)
            .is_some()
        {
            diagnostics.push(Diagnostic::error(
                "P1501",
                module.filename.clone(),
                Some(constant.span),
                None,
                format!(
                    "Module constant {} is defined more than once",
                    constant.name
                ),
            ));
        }
    }
    constants
}

pub(super) fn infer_constant_value(
    constant: &ConstantDefinition,
    filename: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<Type> {
    fn infer(expression: &Expression) -> Option<Type> {
        match expression {
            Expression::Constant { value, .. } => match value {
                ConstantValue::None => Some(Type::None),
                ConstantValue::Bool(_) => Some(Type::Bool),
                ConstantValue::Int(_) => Some(Type::Int),
                ConstantValue::Str(_) => Some(Type::Str),
                ConstantValue::Bytes(_) => Some(Type::Bytes),
                ConstantValue::Ellipsis | ConstantValue::Unsupported(_) => None,
            },
            Expression::Tuple { elements, .. } => elements
                .iter()
                .map(infer)
                .collect::<Option<Vec<_>>>()
                .map(Type::TupleFixed),
            Expression::List { .. } => None,
            Expression::Unary {
                operator: UnaryOperator::Positive | UnaryOperator::Negative,
                operand,
                ..
            } if infer(operand) == Some(Type::Int) => Some(Type::Int),
            _ => None,
        }
    }

    let result = infer(&constant.value);
    if result.is_none() {
        diagnostics.push(Diagnostic::error(
            "P1501",
            filename.to_owned(),
            Some(constant.value.span()),
            None,
            "A module constant initializer must be statically evaluable",
        ));
    }
    result
}
