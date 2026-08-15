use std::collections::BTreeMap;

use efct_model::{Effect, ExternalEffect, PartialBehavior};
use efct_protocol::ConstantValue;

use crate::hir::Import;
use crate::types::Type;

use crate::exceptions::BuiltinExceptionKind;

mod environment;
mod filesystem;
mod nondeterminism;
mod process;

const CONTEXT_MANAGERS: &[(&str, &str)] = &[("contextlib", "suppress")];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum ApiSignatureType {
    None,
    Bool,
    Int,
    Str,
    Bytes,
    TupleFixed(&'static [ApiSignatureType]),
    TupleVariadic(&'static ApiSignatureType),
    FrozenSet(&'static ApiSignatureType),
    FrozenMap {
        key: &'static ApiSignatureType,
        value: &'static ApiSignatureType,
    },
    Option(&'static ApiSignatureType),
    Result {
        value: &'static ApiSignatureType,
        error: &'static ApiSignatureType,
    },
    External(&'static str),
}

impl ApiSignatureType {
    pub(crate) fn matches(self, value: &Type) -> bool {
        match (self, value) {
            (Self::None, Type::None)
            | (Self::Bool, Type::Bool)
            | (Self::Int, Type::Int)
            | (Self::Str, Type::Str)
            | (Self::Bytes, Type::Bytes) => true,
            (Self::TupleFixed(expected), Type::TupleFixed(actual)) => {
                expected.len() == actual.len()
                    && expected
                        .iter()
                        .zip(actual)
                        .all(|(expected, actual)| expected.matches(actual))
            }
            (Self::TupleVariadic(expected), Type::TupleVariadic(actual))
            | (Self::FrozenSet(expected), Type::FrozenSet(actual))
            | (Self::Option(expected), Type::Option(actual)) => expected.matches(actual),
            (
                Self::FrozenMap {
                    key: expected_key,
                    value: expected_value,
                },
                Type::FrozenMap(actual_key, actual_value),
            )
            | (
                Self::Result {
                    value: expected_key,
                    error: expected_value,
                },
                Type::Result(actual_key, actual_value),
            ) => expected_key.matches(actual_key) && expected_value.matches(actual_value),
            (Self::External(expected), Type::External(actual)) => expected == actual,
            _ => false,
        }
    }

    pub(crate) fn to_type(self) -> Type {
        match self {
            Self::None => Type::None,
            Self::Bool => Type::Bool,
            Self::Int => Type::Int,
            Self::Str => Type::Str,
            Self::Bytes => Type::Bytes,
            Self::TupleFixed(elements) => {
                Type::TupleFixed(elements.iter().map(|element| element.to_type()).collect())
            }
            Self::TupleVariadic(element) => Type::TupleVariadic(Box::new(element.to_type())),
            Self::FrozenSet(element) => Type::FrozenSet(Box::new(element.to_type())),
            Self::FrozenMap { key, value } => {
                Type::FrozenMap(Box::new(key.to_type()), Box::new(value.to_type()))
            }
            Self::Option(element) => Type::Option(Box::new(element.to_type())),
            Self::Result { value, error } => {
                Type::Result(Box::new(value.to_type()), Box::new(error.to_type()))
            }
            Self::External(name) => Type::External(name.to_owned()),
        }
    }
}

#[derive(Debug)]
pub(crate) struct Operation {
    pub(crate) name: &'static str,
    pub(crate) parameters: &'static [ApiSignatureType],
    pub(crate) returns: ApiSignatureType,
    pub(crate) effects: OperationEffects,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ApiEffect {
    External(ExternalEffect),
    Raise(BuiltinExceptionKind),
    Diverge,
}

impl ApiEffect {
    fn materialize(self) -> Effect {
        match self {
            Self::External(effect) => Effect::External(effect),
            Self::Raise(exception) => {
                Effect::Partial(PartialBehavior::Raise(exception.identifier()))
            }
            Self::Diverge => Effect::Partial(PartialBehavior::Diverge),
        }
    }
}

impl Operation {
    pub(crate) fn accepts(&self, arguments: &[Type]) -> bool {
        self.parameters.len() == arguments.len()
            && self
                .parameters
                .iter()
                .zip(arguments)
                .all(|(expected, actual)| expected.matches(actual))
    }

    pub(crate) fn resolve_effects(
        &self,
        arguments: &[crate::hir::Expression],
    ) -> Result<Vec<Effect>, FileModeError> {
        match self.effects {
            OperationEffects::Fixed(effects) => Ok(materialize_effects(effects)),
            OperationEffects::FileOpenMode { parameter } => {
                file_open_effects(arguments.get(parameter))
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum OperationEffects {
    Fixed(&'static [ApiEffect]),
    FileOpenMode { parameter: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileModeError {
    Dynamic,
    Unsupported,
}

const FILE_READ: &[ApiEffect] = &[
    ApiEffect::External(ExternalEffect::FileRead),
    ApiEffect::Raise(BuiltinExceptionKind::OperatingSystemFailure),
    ApiEffect::Raise(BuiltinExceptionKind::InvalidValue),
];
const FILE_WRITE: &[ApiEffect] = &[
    ApiEffect::External(ExternalEffect::FileWrite),
    ApiEffect::Raise(BuiltinExceptionKind::OperatingSystemFailure),
    ApiEffect::Raise(BuiltinExceptionKind::InvalidValue),
];
const FILE_READ_WRITE: &[ApiEffect] = &[
    ApiEffect::External(ExternalEffect::FileRead),
    ApiEffect::External(ExternalEffect::FileWrite),
    ApiEffect::Raise(BuiltinExceptionKind::OperatingSystemFailure),
    ApiEffect::Raise(BuiltinExceptionKind::InvalidValue),
];

pub(crate) fn file_open_effects(
    mode: Option<&crate::hir::Expression>,
) -> Result<Vec<Effect>, FileModeError> {
    let Some(mode) = mode else {
        return Ok(materialize_effects(FILE_READ));
    };
    let crate::hir::Expression::Constant {
        value: ConstantValue::Str(mode),
        ..
    } = mode
    else {
        return Err(FileModeError::Dynamic);
    };
    match mode.as_str() {
        "r" | "rb" | "rt" => Ok(materialize_effects(FILE_READ)),
        "w" | "wb" | "wt" | "a" | "ab" | "at" | "x" | "xb" | "xt" => {
            Ok(materialize_effects(FILE_WRITE))
        }
        "r+" | "r+b" | "rb+" | "r+t" | "rt+" | "w+" | "w+b" | "wb+" | "w+t" | "wt+" | "a+"
        | "a+b" | "ab+" | "a+t" | "at+" | "x+" | "x+b" | "xb+" | "x+t" | "xt+" => {
            Ok(materialize_effects(FILE_READ_WRITE))
        }
        _ => Err(FileModeError::Unsupported),
    }
}

pub(crate) fn console_effects() -> Vec<Effect> {
    materialize_effects(&[
        ApiEffect::External(ExternalEffect::Console),
        ApiEffect::Raise(BuiltinExceptionKind::OperatingSystemFailure),
        ApiEffect::Raise(BuiltinExceptionKind::InvalidValue),
    ])
}

fn materialize_effects(effects: &[ApiEffect]) -> Vec<Effect> {
    effects
        .iter()
        .copied()
        .map(ApiEffect::materialize)
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ImportBinding {
    Module(String),
    Symbol(String),
}

pub(crate) fn import_bindings(imports: &[Import]) -> BTreeMap<String, ImportBinding> {
    imports
        .iter()
        .filter_map(|import| match import {
            Import::Module { path, binding, .. } if is_module(path) => {
                Some((binding.clone(), ImportBinding::Module(path.clone())))
            }
            Import::Symbol {
                module,
                name,
                binding,
                ..
            } if is_modeled_symbol(module, name) => Some((
                binding.clone(),
                ImportBinding::Symbol(format!("{module}.{name}")),
            )),
            _ => None,
        })
        .collect()
}

pub(crate) fn resolve_attribute(
    lexical_name: &str,
    bindings: &BTreeMap<String, ImportBinding>,
) -> Option<String> {
    let (root, suffix) = lexical_name.split_once('.')?;
    match bindings.get(root)? {
        ImportBinding::Module(module) => Some(format!("{module}.{suffix}")),
        ImportBinding::Symbol(_) => None,
    }
}

pub(crate) fn resolve_name(
    name: &str,
    bindings: &BTreeMap<String, ImportBinding>,
) -> Option<String> {
    match bindings.get(name) {
        Some(ImportBinding::Symbol(symbol)) => Some(symbol.clone()),
        Some(ImportBinding::Module(_)) => None,
        None if find(name).is_some() => Some(name.to_owned()),
        None => None,
    }
}

pub(crate) fn find(name: &str) -> Option<&'static Operation> {
    operations().find(|operation| operation.name == name)
}

pub(crate) fn find_matching(name: &str, arguments: &[Type]) -> Option<&'static Operation> {
    operations().find(|operation| operation.name == name && operation.accepts(arguments))
}

pub(crate) fn operations() -> impl Iterator<Item = &'static Operation> {
    filesystem::OPERATIONS
        .iter()
        .chain(environment::OPERATIONS)
        .chain(nondeterminism::OPERATIONS)
        .chain(process::OPERATIONS)
}

pub(crate) fn is_module(name: &str) -> bool {
    CONTEXT_MANAGERS.iter().any(|(module, _)| module == &name)
        || operations().any(|operation| {
            operation
                .name
                .split_once('.')
                .is_some_and(|(module, _)| module == name)
        })
}

pub(crate) fn is_modeled_symbol(module: &str, name: &str) -> bool {
    CONTEXT_MANAGERS
        .iter()
        .any(|(registered_module, registered_name)| {
            registered_module == &module && registered_name == &name
        })
        || find(&format!("{module}.{name}")).is_some()
}

pub(crate) fn context_manager_members() -> impl Iterator<Item = (&'static str, &'static str)> {
    CONTEXT_MANAGERS.iter().copied()
}

pub(crate) fn is_contextlib_suppress(name: &str) -> bool {
    name == "contextlib.suppress"
}

#[cfg(test)]
mod tests {
    use efct_protocol::{ConstantValue, SourceSpan};

    use super::{
        ApiSignatureType, FILE_READ, FILE_READ_WRITE, FILE_WRITE, FileModeError, file_open_effects,
        materialize_effects,
    };
    use crate::hir::Expression;
    use crate::types::Type;

    const SPAN: SourceSpan = SourceSpan {
        start_line: 1,
        start_utf8_byte: 0,
        end_line: 1,
        end_utf8_byte: 0,
    };

    #[test]
    fn converts_and_matches_common_signature_types() {
        let cases = [
            (ApiSignatureType::Bool, Type::Bool),
            (
                ApiSignatureType::TupleFixed(&[ApiSignatureType::Int, ApiSignatureType::Str]),
                Type::TupleFixed(vec![Type::Int, Type::Str]),
            ),
            (
                ApiSignatureType::TupleVariadic(&ApiSignatureType::Bytes),
                Type::TupleVariadic(Box::new(Type::Bytes)),
            ),
            (
                ApiSignatureType::FrozenSet(&ApiSignatureType::Int),
                Type::FrozenSet(Box::new(Type::Int)),
            ),
            (
                ApiSignatureType::FrozenMap {
                    key: &ApiSignatureType::Str,
                    value: &ApiSignatureType::Int,
                },
                Type::FrozenMap(Box::new(Type::Str), Box::new(Type::Int)),
            ),
            (
                ApiSignatureType::Option(&ApiSignatureType::Str),
                Type::Option(Box::new(Type::Str)),
            ),
            (
                ApiSignatureType::Result {
                    value: &ApiSignatureType::Int,
                    error: &ApiSignatureType::Str,
                },
                Type::Result(Box::new(Type::Int), Box::new(Type::Str)),
            ),
        ];

        for (signature, analyzed) in cases {
            assert_eq!(signature.to_type(), analyzed);
            assert!(signature.matches(&analyzed));
        }
    }

    #[test]
    fn rejects_a_different_nested_signature_type() {
        let signature = ApiSignatureType::Option(&ApiSignatureType::Str);
        assert!(!signature.matches(&Type::Option(Box::new(Type::Bytes))));
    }

    #[test]
    fn classifies_supported_static_file_modes() {
        for mode in ["r", "rb", "rt"] {
            assert_eq!(
                file_open_effects(Some(&string(mode))),
                Ok(materialize_effects(FILE_READ))
            );
        }
        for mode in ["w", "wb", "wt", "a", "ab", "at", "x", "xb", "xt"] {
            assert_eq!(
                file_open_effects(Some(&string(mode))),
                Ok(materialize_effects(FILE_WRITE))
            );
        }
        for mode in [
            "r+", "r+b", "rb+", "r+t", "rt+", "w+", "w+b", "wb+", "w+t", "wt+", "a+", "a+b", "ab+",
            "a+t", "at+", "x+", "x+b", "xb+", "x+t", "xt+",
        ] {
            assert_eq!(
                file_open_effects(Some(&string(mode))),
                Ok(materialize_effects(FILE_READ_WRITE))
            );
        }
    }

    #[test]
    fn rejects_dynamic_and_unsupported_file_modes() {
        let dynamic = Expression::Name {
            identifier: "mode".to_owned(),
            span: SPAN,
        };

        assert_eq!(
            file_open_effects(Some(&dynamic)),
            Err(FileModeError::Dynamic)
        );
        assert_eq!(
            file_open_effects(Some(&string("unknown"))),
            Err(FileModeError::Unsupported)
        );
    }

    fn string(value: &str) -> Expression {
        Expression::Constant {
            value: ConstantValue::Str(value.to_owned()),
            span: SPAN,
        }
    }
}
