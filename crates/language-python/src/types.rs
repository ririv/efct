use std::{collections::BTreeSet, fmt};

use efct_model::{EffectFormula, ExceptionId, PartialBehavior};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum LocalSource {
    Created {
        binding: String,
    },
    Borrowed {
        binding: String,
        source: Box<LocalSource>,
    },
}

impl LocalSource {
    #[must_use]
    pub fn borrowed_by(&self, binding: String) -> Self {
        Self::Borrowed {
            binding,
            source: Box::new(self.clone()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Type {
    Never,
    None,
    Bool,
    Int,
    Str,
    Bytes,
    TupleFixed(Vec<Type>),
    TupleVariadic(Box<Type>),
    FrozenSet(Box<Type>),
    FrozenMap(Box<Type>, Box<Type>),
    Option(Box<Type>),
    Result(Box<Type>, Box<Type>),
    Ok(Box<Type>),
    Err(Box<Type>),
    Record {
        name: String,
        fields: Vec<(String, Type)>,
    },
    PureCallable {
        parameters: Vec<Type>,
        returns: Box<Type>,
    },
    EffectCallable {
        parameters: Vec<Type>,
        returns: Box<Type>,
        effects: EffectFormula,
    },
    LocalList {
        element: Box<Type>,
        source: LocalSource,
    },
    Range,
    Exception(ExceptionId),
    ExceptionGroup(BTreeSet<ExceptionId>),
    CaughtException(BTreeSet<PartialBehavior>),
    External(String),
}

impl Type {
    #[must_use]
    pub fn is_boundary_value(&self) -> bool {
        match self {
            Self::Never => false,
            Self::None | Self::Bool | Self::Int | Self::Str | Self::Bytes => true,
            Self::TupleFixed(elements) => elements.iter().all(Self::is_data_value),
            Self::TupleVariadic(element) => element.is_data_value(),
            Self::FrozenSet(element) | Self::Option(element) => element.is_data_value(),
            Self::FrozenMap(key, value) | Self::Result(key, value) => {
                key.is_data_value() && value.is_data_value()
            }
            Self::Record { fields, .. } => fields.iter().all(|(_, field)| field.is_data_value()),
            Self::PureCallable {
                parameters,
                returns,
            }
            | Self::EffectCallable {
                parameters,
                returns,
                ..
            } => parameters.iter().all(Self::is_boundary_value) && returns.is_boundary_value(),
            Self::Ok(_)
            | Self::Err(_)
            | Self::LocalList { .. }
            | Self::Range
            | Self::Exception(_)
            | Self::ExceptionGroup(_)
            | Self::CaughtException(_)
            | Self::External(_) => false,
        }
    }

    #[must_use]
    pub fn is_data_value(&self) -> bool {
        self.is_boundary_value()
            && match self {
                Self::PureCallable { .. } | Self::EffectCallable { .. } => false,
                Self::TupleFixed(elements) => elements.iter().all(Self::is_data_value),
                Self::TupleVariadic(element) | Self::FrozenSet(element) | Self::Option(element) => {
                    element.is_data_value()
                }
                Self::FrozenMap(key, value) | Self::Result(key, value) => {
                    key.is_data_value() && value.is_data_value()
                }
                Self::Record { fields, .. } => {
                    fields.iter().all(|(_, field)| field.is_data_value())
                }
                _ => true,
            }
    }

    #[must_use]
    pub fn contains_local_mutable(&self) -> bool {
        match self {
            Self::LocalList { .. } => true,
            Self::TupleFixed(elements) => elements.iter().any(Self::contains_local_mutable),
            Self::TupleVariadic(element)
            | Self::FrozenSet(element)
            | Self::Option(element)
            | Self::Ok(element)
            | Self::Err(element) => element.contains_local_mutable(),
            Self::FrozenMap(key, value) | Self::Result(key, value) => {
                key.contains_local_mutable() || value.contains_local_mutable()
            }
            Self::Record { fields, .. } => fields
                .iter()
                .any(|(_, field)| field.contains_local_mutable()),
            Self::PureCallable {
                parameters,
                returns,
            }
            | Self::EffectCallable {
                parameters,
                returns,
                ..
            } => {
                parameters.iter().any(Self::contains_local_mutable)
                    || returns.contains_local_mutable()
            }
            Self::None
            | Self::Never
            | Self::Bool
            | Self::Int
            | Self::Str
            | Self::Bytes
            | Self::Range
            | Self::Exception(_)
            | Self::ExceptionGroup(_)
            | Self::CaughtException(_)
            | Self::External(_) => false,
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Never => formatter.write_str("Never"),
            Self::None => formatter.write_str("None"),
            Self::Bool => formatter.write_str("bool"),
            Self::Int => formatter.write_str("int"),
            Self::Str => formatter.write_str("str"),
            Self::Bytes => formatter.write_str("bytes"),
            Self::TupleFixed(elements) => {
                formatter.write_str("tuple[")?;
                for (index, element) in elements.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str(", ")?;
                    }
                    write!(formatter, "{element}")?;
                }
                formatter.write_str("]")
            }
            Self::TupleVariadic(element) => write!(formatter, "tuple[{element}, ...]"),
            Self::FrozenSet(element) => write!(formatter, "frozenset[{element}]"),
            Self::FrozenMap(key, value) => write!(formatter, "efct.FrozenMap[{key}, {value}]"),
            Self::Option(element) => write!(formatter, "{element} | None"),
            Self::Result(value, error) => write!(formatter, "efct.Result[{value}, {error}]"),
            Self::Ok(value) => write!(formatter, "efct.Ok[{value}]"),
            Self::Err(error) => write!(formatter, "efct.Err[{error}]"),
            Self::Record { name, .. } => formatter.write_str(name),
            Self::PureCallable {
                parameters,
                returns,
            } => {
                formatter.write_str("efct.PureCallable[[")?;
                for (index, parameter) in parameters.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str(", ")?;
                    }
                    write!(formatter, "{parameter}")?;
                }
                write!(formatter, "], {returns}]")
            }
            Self::EffectCallable {
                parameters,
                returns,
                effects,
            } => {
                formatter.write_str("efct.EffectCallable[[")?;
                for (index, parameter) in parameters.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str(", ")?;
                    }
                    write!(formatter, "{parameter}")?;
                }
                write!(formatter, "], {returns}, ")?;
                for (index, effect) in effects.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str(" | ")?;
                    }
                    write!(formatter, "{effect}")?;
                }
                formatter.write_str("]")
            }
            Self::LocalList { element, .. } => write!(formatter, "local list[{element}]"),
            Self::Range => formatter.write_str("range"),
            Self::Exception(exception) => write!(formatter, "exception:{exception}"),
            Self::ExceptionGroup(exceptions) => {
                formatter.write_str("exception group[")?;
                for (index, exception) in exceptions.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str(" | ")?;
                    }
                    write!(formatter, "{exception}")?;
                }
                formatter.write_str("]")
            }
            Self::CaughtException(partials) => {
                formatter.write_str("caught exception[")?;
                for (index, partial) in partials.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str(" | ")?;
                    }
                    write!(formatter, "{partial}")?;
                }
                formatter.write_str("]")
            }
            Self::External(name) => write!(formatter, "external:{name}"),
        }
    }
}
