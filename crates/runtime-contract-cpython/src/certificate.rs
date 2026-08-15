use efct_language_python::{
    ExceptionBinding, RuntimeCallableKind as PlanCallableKind, RuntimePlan,
    RuntimeType as PlanRuntimeType,
};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyModule, PyType};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallableKind {
    InferredPure,
    BoundedPure,
    InferredEffect,
    BoundedEffect,
}

pub enum RuntimeType {
    Scalar(ScalarKind),
    TupleFixed(Vec<RuntimeType>),
    TupleVariadic(Box<RuntimeType>),
    FrozenSet(Box<RuntimeType>),
    FrozenMap {
        key: Box<RuntimeType>,
        value: Box<RuntimeType>,
    },
    Option(Box<RuntimeType>),
    Result {
        value: Box<RuntimeType>,
        error: Box<RuntimeType>,
    },
    Record {
        record: Py<PyAny>,
        fields: Vec<(String, RuntimeType)>,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScalarKind {
    None,
    Bool,
    Int,
    Str,
    Bytes,
}

pub struct Certificate {
    pub object: Py<PyAny>,
    pub module_name: String,
    pub function_name: String,
    pub callable_kind: CallableKind,
    pub declared_effects: Vec<String>,
    pub parameter_names: Vec<String>,
    pub parameter_types: Vec<RuntimeType>,
    pub return_type: RuntimeType,
    pub dependency_names: Vec<String>,
    pub constant_types: Vec<(String, RuntimeType)>,
    pub source_sha256: String,
    pub dependency_sources: Vec<(String, String)>,
    pub imported_functions: Vec<(String, String, String)>,
    pub imported_modules: Vec<(String, String, Vec<String>)>,
    pub exception_bindings: Vec<ExceptionBinding>,
    pub external_functions: Vec<ExternalFunction>,
    pub external_modules: Vec<ExternalModule>,
}

pub struct CertificateMetadata {
    pub module_name: String,
    pub function_name: String,
    pub dependency_names: Vec<String>,
    pub source_sha256: String,
    pub dependency_sources: Vec<(String, String)>,
    pub code_fingerprint: String,
    pub python_version: (u8, u8, u8),
    pub protocol_version: u32,
    pub core_version: String,
    pub registry_version: u32,
    pub boundaries: Vec<BoundaryEvidence>,
}

pub(crate) struct CertificateViewMetadata {
    pub(crate) code_fingerprint: String,
    pub(crate) python_version: (u8, u8, u8),
    pub(crate) protocol_version: u32,
    pub(crate) core_version: String,
    pub(crate) registry_version: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BoundaryEvidence {
    Audited(AuditedBoundary),
    Unsafe { path: String, reason: String },
}

impl BoundaryEvidence {
    pub fn path(&self) -> &str {
        match self {
            Self::Audited(boundary) => &boundary.path,
            Self::Unsafe { path, .. } => path,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditedBoundary {
    pub path: String,
    pub owner: String,
    pub public_module: String,
    pub public_artifact_path: String,
    pub implementation: AuditedImplementation,
    pub boundary_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuditedImplementation {
    Python {
        module: String,
        qualname: String,
        artifact_path: String,
        artifact_sha256: String,
    },
    Native {
        module: String,
        qualname: String,
        artifact_path: String,
    },
}

pub struct ExternalFunction {
    pub binding: String,
    pub module: String,
    pub name: String,
    pub boundary: BoundaryEvidence,
}

pub struct ExternalModule {
    pub binding: String,
    pub module: String,
    pub members: Vec<ExternalMember>,
}

pub struct ExternalMember {
    pub name: String,
    pub boundary: BoundaryEvidence,
}

impl Certificate {
    pub fn from_plan(
        py: Python<'_>,
        module: &Bound<'_, PyModule>,
        plan: RuntimePlan,
        expected_kind: CallableKind,
        expected_effects: &[String],
        metadata: CertificateMetadata,
    ) -> PyResult<Self> {
        let callable_kind = match plan.callable_kind {
            PlanCallableKind::InferredPure => CallableKind::InferredPure,
            PlanCallableKind::BoundedPure => CallableKind::BoundedPure,
            PlanCallableKind::InferredEffect => CallableKind::InferredEffect,
            PlanCallableKind::BoundedEffect => CallableKind::BoundedEffect,
        };
        if callable_kind != expected_kind
            || !declarations_match(&plan.declared_effects, expected_effects)
        {
            return Err(PyValueError::new_err(format!(
                "The live Efct marker for function {} does not match the source; source kind {callable_kind:?} with declarations {:?}, live kind {expected_kind:?} with declarations {expected_effects:?}",
                metadata.function_name, plan.declared_effects,
            )));
        }
        let parameter_types = convert_types(py, module, &plan.parameter_types)?;
        let return_type = RuntimeType::from_plan(py, module, &plan.return_type)?;
        let constant_types = plan
            .constant_types
            .iter()
            .map(|item| {
                Ok((
                    item.name.clone(),
                    RuntimeType::from_plan(py, module, &item.value_type)?,
                ))
            })
            .collect::<PyResult<Vec<_>>>()?;
        let exception_symbols = plan
            .exception_bindings
            .iter()
            .filter(|item| !item.binding.contains('.'))
            .map(|item| item.binding.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let imported_functions = plan
            .symbol_imports
            .iter()
            .filter(|item| metadata.dependency_names.contains(&item.binding))
            .filter(|item| !exception_symbols.contains(item.binding.as_str()))
            .map(|item| (item.binding.clone(), item.module.clone(), item.name.clone()))
            .collect::<Vec<_>>();
        let imported_modules = plan
            .module_members
            .iter()
            .map(|item| {
                (
                    item.binding.clone(),
                    item.module.clone(),
                    item.members.clone(),
                )
            })
            .collect::<Vec<_>>();
        let external_functions = plan
            .symbol_imports
            .iter()
            .filter(|item| metadata.dependency_names.contains(&item.binding))
            .filter_map(|item| {
                boundary(
                    &metadata.boundaries,
                    &format!("{}.{}", item.module, item.name),
                )
                .map(|boundary| ExternalFunction {
                    binding: item.binding.clone(),
                    module: item.module.clone(),
                    name: item.name.clone(),
                    boundary: boundary.clone(),
                })
            })
            .collect::<Vec<_>>();
        let external_modules = plan
            .module_members
            .iter()
            .filter_map(|item| {
                let members =
                    item.members
                        .iter()
                        .filter_map(|name| {
                            boundary(&metadata.boundaries, &format!("{}.{}", item.module, name))
                                .map(|boundary| ExternalMember {
                                    name: name.clone(),
                                    boundary: boundary.clone(),
                                })
                        })
                        .collect::<Vec<_>>();
                (!members.is_empty()).then(|| ExternalModule {
                    binding: item.binding.clone(),
                    module: item.module.clone(),
                    members,
                })
            })
            .collect::<Vec<_>>();
        let view_metadata = CertificateViewMetadata {
            code_fingerprint: metadata.code_fingerprint.clone(),
            python_version: metadata.python_version,
            protocol_version: metadata.protocol_version,
            core_version: metadata.core_version.clone(),
            registry_version: metadata.registry_version,
        };
        let mut certificate = Self {
            object: py.None(),
            module_name: metadata.module_name,
            function_name: metadata.function_name,
            callable_kind,
            declared_effects: plan.declared_effects,
            parameter_names: plan.parameter_names,
            parameter_types,
            return_type,
            dependency_names: metadata.dependency_names,
            constant_types,
            source_sha256: metadata.source_sha256,
            dependency_sources: metadata.dependency_sources,
            imported_functions,
            imported_modules,
            exception_bindings: plan.exception_bindings,
            external_functions,
            external_modules,
        };
        certificate.object = certificate.python_view(py, &view_metadata)?;
        Ok(certificate)
    }

    pub fn dependency_source(&self, module: &str) -> Option<&str> {
        self.dependency_sources
            .iter()
            .find_map(|(name, digest)| (name == module).then_some(digest.as_str()))
    }

    pub fn constant_type(&self, name: &str) -> Option<&RuntimeType> {
        self.constant_types
            .iter()
            .find_map(|(binding, value_type)| (binding == name).then_some(value_type))
    }
}

fn declarations_match(source: &[String], live: &[String]) -> bool {
    let mut source = source.to_vec();
    let mut live = live.to_vec();
    source.sort_unstable();
    live.sort_unstable();
    source == live
}

impl RuntimeType {
    fn from_plan(
        py: Python<'_>,
        module: &Bound<'_, PyModule>,
        value: &PlanRuntimeType,
    ) -> PyResult<Self> {
        Ok(match value {
            PlanRuntimeType::Scalar { name } => Self::Scalar(match name.as_str() {
                "None" => ScalarKind::None,
                "bool" => ScalarKind::Bool,
                "int" => ScalarKind::Int,
                "str" => ScalarKind::Str,
                "bytes" => ScalarKind::Bytes,
                other => {
                    return Err(PyValueError::new_err(format!(
                        "The runtime plan contains unknown scalar type {other}"
                    )));
                }
            }),
            PlanRuntimeType::TupleFixed { elements } => {
                Self::TupleFixed(convert_types(py, module, elements)?)
            }
            PlanRuntimeType::TupleVariadic { element } => {
                Self::TupleVariadic(Box::new(Self::from_plan(py, module, element)?))
            }
            PlanRuntimeType::FrozenSet { element } => {
                Self::FrozenSet(Box::new(Self::from_plan(py, module, element)?))
            }
            PlanRuntimeType::FrozenMap { key, value } => Self::FrozenMap {
                key: Box::new(Self::from_plan(py, module, key)?),
                value: Box::new(Self::from_plan(py, module, value)?),
            },
            PlanRuntimeType::Option { element } => {
                Self::Option(Box::new(Self::from_plan(py, module, element)?))
            }
            PlanRuntimeType::Result { value, error } => Self::Result {
                value: Box::new(Self::from_plan(py, module, value)?),
                error: Box::new(Self::from_plan(py, module, error)?),
            },
            PlanRuntimeType::Record { name, fields } => {
                let Some(record) = module.dict().get_item(name)? else {
                    return Err(PyValueError::new_err(format!(
                        "Pure record {name} cannot be located in the module"
                    )));
                };
                if !record.get_type().is(py.get_type::<PyType>()) {
                    return Err(PyValueError::new_err(format!(
                        "Pure record {name} cannot be located in the module"
                    )));
                }
                let registered = py
                    .import("efct.values")?
                    .getattr("_pure_record_fields")?
                    .call1((&record,))?;
                if registered.is_none() {
                    return Err(PyValueError::new_err(format!(
                        "Pure record {name} has not been verified"
                    )));
                }
                let expected = fields
                    .iter()
                    .map(|field| field.name.as_str())
                    .collect::<Vec<_>>();
                if registered.extract::<Vec<String>>()? != expected {
                    return Err(PyValueError::new_err(format!(
                        "Pure record {name} does not match the Rust runtime plan"
                    )));
                }
                Self::Record {
                    record: record.unbind(),
                    fields: fields
                        .iter()
                        .map(|field| {
                            Ok((
                                field.name.clone(),
                                Self::from_plan(py, module, &field.value_type)?,
                            ))
                        })
                        .collect::<PyResult<Vec<_>>>()?,
                }
            }
            PlanRuntimeType::PureCallable {
                parameters,
                returns,
            } => Self::PureCallable {
                parameters: convert_types(py, module, parameters)?,
                returns: Box::new(Self::from_plan(py, module, returns)?),
            },
            PlanRuntimeType::EffectCallable {
                parameters,
                returns,
                effect_variable,
            } => Self::EffectCallable {
                parameters: convert_types(py, module, parameters)?,
                returns: Box::new(Self::from_plan(py, module, returns)?),
                effect_variable: effect_variable.clone(),
            },
        })
    }

    pub fn format(&self, py: Python<'_>) -> PyResult<String> {
        Ok(match self {
            Self::Scalar(kind) => scalar_name(*kind).to_owned(),
            Self::TupleFixed(elements) => {
                format!("tuple[{}]", format_types(elements, py)?.join(", "))
            }
            Self::TupleVariadic(element) => format!("tuple[{}, ...]", element.format(py)?),
            Self::FrozenSet(element) => format!("frozenset[{}]", element.format(py)?),
            Self::FrozenMap { key, value } => {
                format!("efct.FrozenMap[{}, {}]", key.format(py)?, value.format(py)?)
            }
            Self::Option(element) => format!("{} | None", element.format(py)?),
            Self::Result { value, error } => {
                format!("efct.Result[{}, {}]", value.format(py)?, error.format(py)?)
            }
            Self::Record { record, .. } => {
                let record = record.bind(py);
                format!(
                    "{}.{}",
                    record.getattr("__module__")?.extract::<String>()?,
                    record.getattr("__name__")?.extract::<String>()?
                )
            }
            Self::PureCallable {
                parameters,
                returns,
            } => format!(
                "efct.PureCallable[[{}], {}]",
                format_types(parameters, py)?.join(", "),
                returns.format(py)?
            ),
            Self::EffectCallable {
                parameters,
                returns,
                effect_variable,
            } => format!(
                "efct.EffectCallable[[{}], {}, {}]",
                format_types(parameters, py)?.join(", "),
                returns.format(py)?,
                effect_variable
            ),
        })
    }
}

fn boundary<'a>(values: &'a [BoundaryEvidence], path: &str) -> Option<&'a BoundaryEvidence> {
    values.iter().find(|value| value.path() == path)
}

fn convert_types(
    py: Python<'_>,
    module: &Bound<'_, PyModule>,
    values: &[PlanRuntimeType],
) -> PyResult<Vec<RuntimeType>> {
    values
        .iter()
        .map(|value| RuntimeType::from_plan(py, module, value))
        .collect()
}

fn format_types(values: &[RuntimeType], py: Python<'_>) -> PyResult<Vec<String>> {
    values.iter().map(|value| value.format(py)).collect()
}

fn scalar_name(kind: ScalarKind) -> &'static str {
    match kind {
        ScalarKind::None => "None",
        ScalarKind::Bool => "bool",
        ScalarKind::Int => "int",
        ScalarKind::Str => "str",
        ScalarKind::Bytes => "bytes",
    }
}

#[cfg(test)]
mod tests {
    use super::declarations_match;

    #[test]
    fn declaration_comparison_is_order_independent() {
        let source = vec!["clock".to_owned(), "raise:builtins.ValueError".to_owned()];
        let live = vec!["raise:builtins.ValueError".to_owned(), "clock".to_owned()];

        assert!(declarations_match(&source, &live));
    }

    #[test]
    fn declaration_comparison_preserves_multiplicity() {
        let source = vec!["clock".to_owned(), "raise:builtins.ValueError".to_owned()];
        let live = vec!["clock".to_owned(), "clock".to_owned()];

        assert!(!declarations_match(&source, &live));
    }
}
