use std::collections::{BTreeMap, BTreeSet};

use efct_model::{Diagnostic, Effect, ExceptionId, PartialBehavior};

use crate::hir::{Expression, Module};
use crate::types::Type;

#[derive(Debug, Clone, Copy)]
struct BuiltinException {
    name: &'static str,
    parent: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuiltinExceptionKind {
    MissingImplementation,
    OperatingSystemFailure,
    InvalidValue,
}

impl BuiltinExceptionKind {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::MissingImplementation => "builtins.NotImplementedError",
            Self::OperatingSystemFailure => "builtins.OSError",
            Self::InvalidValue => "builtins.ValueError",
        }
    }

    pub(crate) fn identifier(self) -> ExceptionId {
        parse_registered(self.name())
    }
}

const BUILTIN_EXCEPTIONS: &[BuiltinException] = &[
    BuiltinException {
        name: "builtins.Exception",
        parent: None,
    },
    BuiltinException {
        name: "builtins.ExceptionGroup",
        parent: Some("builtins.Exception"),
    },
    BuiltinException {
        name: "builtins.ArithmeticError",
        parent: Some("builtins.Exception"),
    },
    BuiltinException {
        name: "builtins.AssertionError",
        parent: Some("builtins.Exception"),
    },
    BuiltinException {
        name: "builtins.IndexError",
        parent: Some("builtins.LookupError"),
    },
    BuiltinException {
        name: "builtins.KeyError",
        parent: Some("builtins.LookupError"),
    },
    BuiltinException {
        name: "builtins.LookupError",
        parent: Some("builtins.Exception"),
    },
    BuiltinException {
        name: "builtins.NotImplementedError",
        parent: Some("builtins.RuntimeError"),
    },
    BuiltinException {
        name: "builtins.OSError",
        parent: Some("builtins.Exception"),
    },
    BuiltinException {
        name: "builtins.RuntimeError",
        parent: Some("builtins.Exception"),
    },
    BuiltinException {
        name: "builtins.TypeError",
        parent: Some("builtins.Exception"),
    },
    BuiltinException {
        name: "builtins.ValueError",
        parent: Some("builtins.Exception"),
    },
    BuiltinException {
        name: "builtins.ZeroDivisionError",
        parent: Some("builtins.ArithmeticError"),
    },
];

pub(crate) fn registered_builtin_exception_names() -> impl Iterator<Item = &'static str> {
    BUILTIN_EXCEPTIONS.iter().map(|exception| {
        exception
            .name
            .strip_prefix("builtins.")
            .expect("registered builtin exceptions use the builtins module")
    })
}

pub(crate) fn resolve_builtin_exception(name: &str) -> Option<ExceptionId> {
    let qualified = if name.contains('.') {
        name.to_owned()
    } else {
        format!("builtins.{name}")
    };
    BUILTIN_EXCEPTIONS
        .iter()
        .any(|exception| exception.name == qualified)
        .then(|| parse_registered(&qualified))
}

#[derive(Debug, Clone)]
pub(crate) struct ExceptionHierarchy {
    parents: BTreeMap<ExceptionId, Option<ExceptionId>>,
    aliases: BTreeMap<String, ExceptionId>,
}

impl ExceptionHierarchy {
    pub(crate) fn analyze(module: &Module, diagnostics: &mut Vec<Diagnostic>) -> Self {
        let mut parents: BTreeMap<ExceptionId, Option<ExceptionId>> = BUILTIN_EXCEPTIONS
            .iter()
            .map(|exception| {
                (
                    parse_registered(exception.name),
                    exception.parent.map(parse_registered),
                )
            })
            .collect();
        let custom: BTreeSet<ExceptionId> = module
            .exceptions
            .iter()
            .filter_map(|exception| match ExceptionId::parse(&exception.name) {
                Ok(identifier) => Some(identifier),
                Err(error) => {
                    diagnostics.push(Diagnostic::error(
                        "P1201",
                        module.filename.clone(),
                        Some(exception.span),
                        None,
                        error.to_string(),
                    ));
                    None
                }
            })
            .collect();

        let mut registered_custom = BTreeSet::new();
        for exception in &module.exceptions {
            let Ok(identifier) = ExceptionId::parse(&exception.name) else {
                continue;
            };
            if parents.contains_key(&identifier) {
                diagnostics.push(Diagnostic::error(
                    "P1201",
                    module.filename.clone(),
                    Some(exception.span),
                    None,
                    format!("Exception class {identifier} is defined more than once"),
                ));
                continue;
            }
            let Some(base_name) = qualified_name(&exception.base) else {
                diagnostics.push(Diagnostic::error(
                    "P1201",
                    module.filename.clone(),
                    Some(exception.span),
                    None,
                    "An exception base must be a registered exception type name",
                ));
                continue;
            };
            let base = resolve_builtin_exception(&base_name).or_else(|| {
                ExceptionId::parse(&base_name)
                    .ok()
                    .filter(|candidate| custom.contains(candidate))
            });
            let Some(base) = base else {
                diagnostics.push(Diagnostic::error(
                    "P1201",
                    module.filename.clone(),
                    Some(exception.span),
                    None,
                    format!("Exception base {base_name} is not registered"),
                ));
                continue;
            };
            if base == exception_group_id() {
                diagnostics.push(Diagnostic::error(
                    "P1201",
                    module.filename.clone(),
                    Some(exception.span),
                    None,
                    "Custom exceptions cannot inherit from ExceptionGroup",
                ));
                continue;
            }
            if custom.contains(&base)
                && same_exception_namespace(&identifier, &base)
                && !registered_custom.contains(&base)
            {
                diagnostics.push(Diagnostic::error(
                    "P1201",
                    module.filename.clone(),
                    Some(exception.span),
                    None,
                    format!("Exception base {base} must be defined before subclass {identifier}"),
                ));
                continue;
            }
            parents.insert(identifier.clone(), Some(base));
            registered_custom.insert(identifier);
        }

        let hierarchy = Self {
            parents,
            aliases: BTreeMap::new(),
        };
        for exception in &module.exceptions {
            let Ok(identifier) = ExceptionId::parse(&exception.name) else {
                continue;
            };
            if !registered_custom.contains(&identifier) {
                continue;
            }
            if !hierarchy.reaches_exception(&identifier) {
                diagnostics.push(Diagnostic::error(
                    "P1201",
                    module.filename.clone(),
                    Some(exception.span),
                    None,
                    format!(
                        "Exception class {identifier} must have an acyclic base chain ending at builtins.Exception"
                    ),
                ));
            }
        }
        hierarchy
    }

    pub(crate) fn resolve(&self, name: &str) -> Option<ExceptionId> {
        self.aliases
            .get(name)
            .cloned()
            .or_else(|| resolve_builtin_exception(name))
            .or_else(|| {
                ExceptionId::parse(name)
                    .ok()
                    .filter(|candidate| self.parents.contains_key(candidate))
            })
    }

    pub(crate) fn with_aliases(
        &self,
        aliases: impl IntoIterator<Item = (String, ExceptionId)>,
    ) -> Self {
        let mut hierarchy = self.clone();
        hierarchy.aliases.extend(aliases);
        hierarchy
    }

    pub(crate) fn contains(&self, exception: &ExceptionId) -> bool {
        self.parents.contains_key(exception)
    }

    pub(crate) fn constructor_accepts(&self, exception: &ExceptionId, arguments: &[Type]) -> bool {
        self.contains(exception) && matches!(arguments, [] | [Type::Str])
    }

    pub(crate) fn is_subtype(&self, actual: &ExceptionId, expected: &ExceptionId) -> bool {
        let mut current = Some(actual);
        let mut visited = BTreeSet::new();
        while let Some(exception) = current {
            if exception == expected {
                return true;
            }
            if !visited.insert(exception.clone()) {
                return false;
            }
            current = self.parents.get(exception).and_then(Option::as_ref);
        }
        false
    }

    pub(crate) fn caught_effects(&self, exception: &ExceptionId) -> BTreeSet<Effect> {
        let mut effects: BTreeSet<Effect> = self
            .parents
            .keys()
            .filter(|candidate| self.is_subtype(candidate, exception))
            .cloned()
            .map(|candidate| Effect::Partial(PartialBehavior::Raise(candidate)))
            .collect();
        if self.is_subtype(&exception_group_id(), exception) {
            effects.extend(self.group_leaf_effects());
        }
        effects
    }

    pub(crate) fn caught_group_leaf_effects(&self, exception: &ExceptionId) -> BTreeSet<Effect> {
        self.parents
            .keys()
            .filter(|candidate| {
                candidate != &&exception_group_id() && self.is_subtype(candidate, exception)
            })
            .cloned()
            .map(|candidate| Effect::Partial(PartialBehavior::RaiseGroup(candidate)))
            .collect()
    }

    pub(crate) fn is_exception_group(&self, exception: &ExceptionId) -> bool {
        exception == &exception_group_id()
    }

    fn group_leaf_effects(&self) -> BTreeSet<Effect> {
        self.parents
            .keys()
            .filter(|candidate| candidate != &&exception_group_id())
            .cloned()
            .map(|candidate| Effect::Partial(PartialBehavior::RaiseGroup(candidate)))
            .collect()
    }

    fn reaches_exception(&self, exception: &ExceptionId) -> bool {
        self.is_subtype(exception, &parse_registered("builtins.Exception"))
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

fn parse_registered(name: &str) -> ExceptionId {
    ExceptionId::parse(name).expect("registered exception names are valid")
}

fn exception_group_id() -> ExceptionId {
    parse_registered("builtins.ExceptionGroup")
}

fn same_exception_namespace(left: &ExceptionId, right: &ExceptionId) -> bool {
    exception_namespace(left) == exception_namespace(right)
}

fn exception_namespace(identifier: &ExceptionId) -> Option<&str> {
    identifier
        .as_str()
        .rsplit_once('.')
        .map(|(module, _)| module)
}

#[cfg(test)]
mod tests {
    use crate::hir::Module;

    use super::{BuiltinExceptionKind, ExceptionHierarchy, resolve_builtin_exception};

    fn hierarchy() -> ExceptionHierarchy {
        ExceptionHierarchy::analyze(
            &Module {
                filename: "fixture.py".to_owned(),
                source_sha256: String::new(),
                imports: Vec::new(),
                constants: Vec::new(),
                records: Vec::new(),
                exceptions: Vec::new(),
                functions: Vec::new(),
            },
            &mut Vec::new(),
        )
    }

    #[test]
    fn resolves_only_the_closed_builtin_exception_set() {
        assert!(resolve_builtin_exception("ValueError").is_some());
        assert!(resolve_builtin_exception("builtins.KeyError").is_some());
        assert!(resolve_builtin_exception("OSError").is_some());
        assert!(resolve_builtin_exception("NotImplementedError").is_some());
        assert!(resolve_builtin_exception("BaseException").is_none());
        assert!(resolve_builtin_exception("ExceptionGroup").is_some());
        assert_eq!(
            BuiltinExceptionKind::OperatingSystemFailure
                .identifier()
                .as_str(),
            "builtins.OSError"
        );
    }

    #[test]
    fn follows_the_registered_python_exception_hierarchy() {
        let hierarchy = hierarchy();
        let zero_division = resolve_builtin_exception("ZeroDivisionError").unwrap();
        let arithmetic = resolve_builtin_exception("ArithmeticError").unwrap();
        let exception = resolve_builtin_exception("Exception").unwrap();
        let value = resolve_builtin_exception("ValueError").unwrap();
        let not_implemented = resolve_builtin_exception("NotImplementedError").unwrap();
        let os_error = resolve_builtin_exception("OSError").unwrap();
        let runtime = resolve_builtin_exception("RuntimeError").unwrap();

        assert!(hierarchy.is_subtype(&zero_division, &arithmetic));
        assert!(hierarchy.is_subtype(&zero_division, &exception));
        assert!(!hierarchy.is_subtype(&zero_division, &value));
        assert!(!hierarchy.is_subtype(&exception, &zero_division));
        assert!(hierarchy.is_subtype(&not_implemented, &runtime));
        assert!(hierarchy.is_subtype(&os_error, &exception));
    }
}
