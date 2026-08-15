use std::collections::{HashMap, HashSet};

use pyo3::prelude::*;
use pyo3::types::{PyBool, PyBytes, PyDict, PyFrozenSet, PyInt, PyString, PyTuple, PyType};

use crate::certificate::{CallableKind, Certificate, RuntimeType, ScalarKind};
use crate::error::contract_error;
use crate::function::{PureFunction, with_verified_certificate};

pub struct ValueTypes {
    frozen_map: Py<PyType>,
    ok: Py<PyType>,
    err: Py<PyType>,
}

impl ValueTypes {
    pub fn load(py: Python<'_>) -> PyResult<Self> {
        let values = py.import("efct.values")?;
        Ok(Self {
            frozen_map: values.getattr("FrozenMap")?.cast_into::<PyType>()?.unbind(),
            ok: values.getattr("Ok")?.cast_into::<PyType>()?.unbind(),
            err: values.getattr("Err")?.cast_into::<PyType>()?.unbind(),
        })
    }

    pub fn pure_value_members(&self, py: Python<'_>) -> Vec<(String, Py<PyAny>)> {
        vec![
            ("Err".to_owned(), self.err.clone_ref(py).into_any()),
            (
                "FrozenMap".to_owned(),
                self.frozen_map.clone_ref(py).into_any(),
            ),
            ("Ok".to_owned(), self.ok.clone_ref(py).into_any()),
        ]
    }
}

pub fn bind_arguments(
    py: Python<'_>,
    certificate: &Certificate,
    args: &Bound<'_, PyTuple>,
    kwargs: Option<&Bound<'_, PyDict>>,
) -> PyResult<Vec<Py<PyAny>>> {
    if args.len() > certificate.parameter_names.len() {
        return Err(contract_error(py, "Too many positional arguments"));
    }
    let positional: HashSet<&str> = certificate.parameter_names[..args.len()]
        .iter()
        .map(String::as_str)
        .collect();
    if let Some(kwargs) = kwargs {
        let mut duplicates = Vec::new();
        let mut unknown = Vec::new();
        for (key, _) in kwargs.iter() {
            let key = key.extract::<String>()?;
            if positional.contains(key.as_str()) {
                duplicates.push(key);
            } else if !certificate.parameter_names.contains(&key) {
                unknown.push(key);
            }
        }
        duplicates.sort();
        unknown.sort();
        if let Some(name) = duplicates.first() {
            return Err(contract_error(
                py,
                format!("Argument {name} was assigned more than once"),
            ));
        }
        if let Some(name) = unknown.first() {
            return Err(contract_error(py, format!("Unknown argument {name}")));
        }
    }
    let mut values = args.iter().map(Bound::unbind).collect::<Vec<Py<PyAny>>>();
    for name in &certificate.parameter_names[args.len()..] {
        let value = kwargs
            .and_then(|kwargs| kwargs.get_item(name).transpose())
            .transpose()?;
        let Some(value) = value else {
            return Err(contract_error(py, format!("Missing argument {name}")));
        };
        values.push(value.unbind());
    }
    Ok(values)
}

pub fn matches_type(
    py: Python<'_>,
    value: &Bound<'_, PyAny>,
    expected: &RuntimeType,
    value_types: &ValueTypes,
    bindings: &mut HashMap<String, Vec<String>>,
) -> PyResult<bool> {
    Ok(match expected {
        RuntimeType::Scalar(kind) => scalar_matches(py, value, *kind),
        RuntimeType::TupleFixed(elements) => {
            let Ok(tuple) = value.cast::<PyTuple>() else {
                return Ok(false);
            };
            if tuple.len() != elements.len() {
                return Ok(false);
            }
            let mut matches = true;
            for (item, expected) in tuple.iter().zip(elements) {
                if !matches_type(py, &item, expected, value_types, bindings)? {
                    matches = false;
                    break;
                }
            }
            matches
        }
        RuntimeType::TupleVariadic(element) => {
            let Ok(tuple) = value.cast::<PyTuple>() else {
                return Ok(false);
            };
            let mut matches = true;
            for item in tuple.iter() {
                if !matches_type(py, &item, element, value_types, bindings)? {
                    matches = false;
                    break;
                }
            }
            matches
        }
        RuntimeType::FrozenSet(element) => {
            let Ok(values) = value.cast::<PyFrozenSet>() else {
                return Ok(false);
            };
            let mut matches = true;
            for item in values.iter() {
                if !matches_type(py, &item, element, value_types, bindings)? {
                    matches = false;
                    break;
                }
            }
            matches
        }
        RuntimeType::FrozenMap {
            key,
            value: value_type,
        } => {
            if !value.get_type().is(value_types.frozen_map.bind(py)) {
                return Ok(false);
            }
            let mut matches = true;
            for item in value.try_iter()? {
                let item = item?;
                let item_value = value.get_item(&item)?;
                if !matches_type(py, &item, key, value_types, bindings)?
                    || !matches_type(py, &item_value, value_type, value_types, bindings)?
                {
                    matches = false;
                    break;
                }
            }
            matches
        }
        RuntimeType::Option(element) => {
            if value.is_none() {
                true
            } else {
                matches_type(py, value, element, value_types, bindings)?
            }
        }
        RuntimeType::Result {
            value: value_type,
            error,
        } => {
            if value.get_type().is(value_types.ok.bind(py)) {
                matches_type(
                    py,
                    &value.getattr("value")?,
                    value_type,
                    value_types,
                    bindings,
                )?
            } else if value.get_type().is(value_types.err.bind(py)) {
                matches_type(py, &value.getattr("error")?, error, value_types, bindings)?
            } else {
                false
            }
        }
        RuntimeType::Record { record, fields } => {
            if !value.get_type().is(record.bind(py).cast::<PyType>()?) {
                return Ok(false);
            }
            let mut matches = true;
            for (name, field_type) in fields {
                if !matches_type(py, &value.getattr(name)?, field_type, value_types, bindings)? {
                    matches = false;
                    break;
                }
            }
            matches
        }
        RuntimeType::PureCallable {
            parameters,
            returns,
        } => {
            let Some(matches) = pure_certificate_matches(value, |certificate| {
                certificate.callable_kind == CallableKind::BoundedPure
                    && certificate.declared_effects.is_empty()
                    && types_equivalent(py, &certificate.parameter_types, parameters)
                    && type_equivalent(py, &certificate.return_type, returns)
            }) else {
                return Ok(false);
            };
            matches
        }
        RuntimeType::EffectCallable {
            parameters,
            returns,
            effect_variable,
        } => {
            let Some(result) = with_verified_certificate(value, |certificate| {
                if !types_equivalent(py, &certificate.parameter_types, parameters)
                    || !type_equivalent(py, &certificate.return_type, returns)
                {
                    return None;
                }
                Some(match certificate.callable_kind {
                    CallableKind::BoundedPure => certificate.declared_effects.clone(),
                    CallableKind::BoundedEffect => certificate.declared_effects.clone(),
                    CallableKind::InferredPure | CallableKind::InferredEffect => return None,
                })
            }) else {
                return Ok(false);
            };
            let Some(effects) = result else {
                return Ok(false);
            };
            match bindings.get(effect_variable) {
                Some(bound) => bound == &effects,
                None => {
                    bindings.insert(effect_variable.clone(), effects);
                    true
                }
            }
        }
    })
}

fn scalar_matches(py: Python<'_>, value: &Bound<'_, PyAny>, expected: ScalarKind) -> bool {
    match expected {
        ScalarKind::None => value.is_none(),
        ScalarKind::Bool => value.get_type().is(py.get_type::<PyBool>()),
        ScalarKind::Int => value.get_type().is(py.get_type::<PyInt>()),
        ScalarKind::Str => value.get_type().is(py.get_type::<PyString>()),
        ScalarKind::Bytes => value.get_type().is(py.get_type::<PyBytes>()),
    }
}

fn pure_certificate_matches<T>(
    value: &Bound<'_, PyAny>,
    inspect: impl FnOnce(&Certificate) -> T,
) -> Option<T> {
    let wrapper = value.extract::<PyRef<'_, PureFunction>>().ok()?;
    Some(inspect(wrapper.native_certificate()))
}

fn types_equivalent(py: Python<'_>, left: &[RuntimeType], right: &[RuntimeType]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| type_equivalent(py, left, right))
}

fn type_equivalent(py: Python<'_>, left: &RuntimeType, right: &RuntimeType) -> bool {
    match (left, right) {
        (RuntimeType::Scalar(left), RuntimeType::Scalar(right)) => left == right,
        (RuntimeType::TupleFixed(left), RuntimeType::TupleFixed(right)) => {
            types_equivalent(py, left, right)
        }
        (RuntimeType::TupleVariadic(left), RuntimeType::TupleVariadic(right))
        | (RuntimeType::FrozenSet(left), RuntimeType::FrozenSet(right))
        | (RuntimeType::Option(left), RuntimeType::Option(right)) => {
            type_equivalent(py, left, right)
        }
        (
            RuntimeType::FrozenMap {
                key: left_key,
                value: left_value,
            },
            RuntimeType::FrozenMap {
                key: right_key,
                value: right_value,
            },
        ) => {
            type_equivalent(py, left_key, right_key) && type_equivalent(py, left_value, right_value)
        }
        (
            RuntimeType::Result {
                value: left_value,
                error: left_error,
            },
            RuntimeType::Result {
                value: right_value,
                error: right_error,
            },
        ) => {
            type_equivalent(py, left_value, right_value)
                && type_equivalent(py, left_error, right_error)
        }
        (
            RuntimeType::Record {
                record: left_record,
                fields: left_fields,
            },
            RuntimeType::Record {
                record: right_record,
                fields: right_fields,
            },
        ) => {
            left_record.bind(py).is(right_record.bind(py))
                && named_types_equivalent(py, left_fields, right_fields)
        }
        (
            RuntimeType::PureCallable {
                parameters: left_parameters,
                returns: left_returns,
            },
            RuntimeType::PureCallable {
                parameters: right_parameters,
                returns: right_returns,
            },
        ) => {
            types_equivalent(py, left_parameters, right_parameters)
                && type_equivalent(py, left_returns, right_returns)
        }
        (
            RuntimeType::EffectCallable {
                parameters: left_parameters,
                returns: left_returns,
                effect_variable: left_effect,
            },
            RuntimeType::EffectCallable {
                parameters: right_parameters,
                returns: right_returns,
                effect_variable: right_effect,
            },
        ) => {
            left_effect == right_effect
                && types_equivalent(py, left_parameters, right_parameters)
                && type_equivalent(py, left_returns, right_returns)
        }
        _ => false,
    }
}

fn named_types_equivalent(
    py: Python<'_>,
    left: &[(String, RuntimeType)],
    right: &[(String, RuntimeType)],
) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|((left_name, left_type), (right_name, right_type))| {
                left_name == right_name && type_equivalent(py, left_type, right_type)
            })
}
