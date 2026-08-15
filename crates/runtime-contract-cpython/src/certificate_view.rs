use pyo3::IntoPyObjectExt;
use pyo3::prelude::*;
use pyo3::types::{PyModule, PyTuple};

use crate::certificate::{
    BoundaryEvidence, CallableKind, Certificate, CertificateViewMetadata, RuntimeType, ScalarKind,
};

impl Certificate {
    pub(crate) fn python_view(
        &self,
        py: Python<'_>,
        metadata: &CertificateViewMetadata,
    ) -> PyResult<Py<PyAny>> {
        let definitions = py.import("efct.certificates")?;
        let kind = definitions
            .getattr("CallableKind")?
            .getattr(match self.callable_kind {
                CallableKind::InferredPure => "INFERRED_PURE",
                CallableKind::BoundedPure => "BOUNDED_PURE",
                CallableKind::InferredEffect => "INFERRED_EFFECT",
                CallableKind::BoundedEffect => "BOUNDED_EFFECT",
            })?;
        let parameter_types = self
            .parameter_types
            .iter()
            .map(|value| value.python_view(py, &definitions))
            .collect::<PyResult<Vec<_>>>()?;
        let parameter_types = PyTuple::new(py, parameter_types)?;
        let constant_types = self
            .constant_types
            .iter()
            .map(|(name, value)| Ok((name, value.python_view(py, &definitions)?)))
            .collect::<PyResult<Vec<_>>>()?;
        let constant_types = PyTuple::new(py, constant_types)?;
        let external_functions = self
            .external_functions
            .iter()
            .map(|value| {
                definitions.getattr("ExternalFunctionBinding")?.call1((
                    &value.binding,
                    &value.module,
                    &value.name,
                    boundary_view(py, &definitions, &value.boundary)?,
                ))
            })
            .collect::<PyResult<Vec<_>>>()?;
        let external_modules = self
            .external_modules
            .iter()
            .map(|value| {
                let members = value
                    .members
                    .iter()
                    .map(|member| {
                        definitions.getattr("ExternalModuleMemberBinding")?.call1((
                            &member.name,
                            boundary_view(py, &definitions, &member.boundary)?,
                        ))
                    })
                    .collect::<PyResult<Vec<_>>>()?;
                definitions.getattr("ExternalModuleBinding")?.call1((
                    &value.binding,
                    &value.module,
                    PyTuple::new(py, members)?,
                ))
            })
            .collect::<PyResult<Vec<_>>>()?;
        let arguments = vec![
            self.module_name.as_str().into_py_any(py)?,
            self.function_name.as_str().into_py_any(py)?,
            kind.unbind(),
            PyTuple::new(py, &self.declared_effects)?
                .into_any()
                .unbind(),
            PyTuple::new(py, &self.parameter_names)?.into_any().unbind(),
            parameter_types.into_any().unbind(),
            self.return_type.python_view(py, &definitions)?,
            PyTuple::new(py, &self.dependency_names)?
                .into_any()
                .unbind(),
            constant_types.into_any().unbind(),
            self.source_sha256.as_str().into_py_any(py)?,
            PyTuple::new(py, &self.dependency_sources)?
                .into_any()
                .unbind(),
            PyTuple::new(py, &self.imported_functions)?
                .into_any()
                .unbind(),
            PyTuple::new(py, &self.imported_modules)?
                .into_any()
                .unbind(),
            PyTuple::new(py, external_functions)?.into_any().unbind(),
            PyTuple::new(py, external_modules)?.into_any().unbind(),
            metadata.code_fingerprint.as_str().into_py_any(py)?,
            metadata.python_version.into_py_any(py)?,
            metadata.protocol_version.into_py_any(py)?,
            metadata.core_version.as_str().into_py_any(py)?,
            metadata.registry_version.into_py_any(py)?,
        ];
        let arguments = PyTuple::new(py, arguments)?;
        Ok(definitions
            .getattr("VerificationCertificate")?
            .call1(arguments)?
            .unbind())
    }
}

fn boundary_view(
    _py: Python<'_>,
    definitions: &Bound<'_, PyModule>,
    boundary: &BoundaryEvidence,
) -> PyResult<Py<PyAny>> {
    match boundary {
        BoundaryEvidence::Audited(value) => Ok(definitions
            .getattr("AuditedBoundary")?
            .call1((&value.path, &value.owner, &value.boundary_id))?
            .unbind()),
        BoundaryEvidence::Unsafe { path, reason } => Ok(definitions
            .getattr("UnsafeBoundary")?
            .call1((path, reason))?
            .unbind()),
    }
}

impl RuntimeType {
    fn python_view(
        &self,
        py: Python<'_>,
        definitions: &Bound<'_, PyModule>,
    ) -> PyResult<Py<PyAny>> {
        let value = match self {
            Self::Scalar(kind) => definitions.getattr("ScalarType")?.call1((definitions
                .getattr("ScalarKind")?
                .getattr(scalar_variant(*kind))?,))?,
            Self::TupleFixed(elements) => {
                let elements = type_views(py, definitions, elements)?;
                definitions
                    .getattr("TupleFixedType")?
                    .call1((PyTuple::new(py, elements)?,))?
            }
            Self::TupleVariadic(element) => definitions
                .getattr("TupleVariadicType")?
                .call1((element.python_view(py, definitions)?,))?,
            Self::FrozenSet(element) => {
                definitions
                    .getattr("FrozenSetType")?
                    .call1((element.python_view(py, definitions)?,))?
            }
            Self::FrozenMap { key, value } => definitions.getattr("FrozenMapType")?.call1((
                key.python_view(py, definitions)?,
                value.python_view(py, definitions)?,
            ))?,
            Self::Option(element) => definitions
                .getattr("OptionalType")?
                .call1((element.python_view(py, definitions)?,))?,
            Self::Result { value, error } => definitions.getattr("ResultType")?.call1((
                value.python_view(py, definitions)?,
                error.python_view(py, definitions)?,
            ))?,
            Self::Record { record, fields } => {
                let fields = fields
                    .iter()
                    .map(|(name, value)| Ok((name, value.python_view(py, definitions)?)))
                    .collect::<PyResult<Vec<_>>>()?;
                definitions
                    .getattr("RecordType")?
                    .call1((record.bind(py), PyTuple::new(py, fields)?))?
            }
            Self::PureCallable {
                parameters,
                returns,
            } => definitions.getattr("PureCallableType")?.call1((
                PyTuple::new(py, type_views(py, definitions, parameters)?)?,
                returns.python_view(py, definitions)?,
            ))?,
            Self::EffectCallable {
                parameters,
                returns,
                effect_variable,
            } => definitions.getattr("EffectCallableType")?.call1((
                PyTuple::new(py, type_views(py, definitions, parameters)?)?,
                returns.python_view(py, definitions)?,
                effect_variable,
            ))?,
        };
        Ok(value.unbind())
    }
}

fn type_views(
    py: Python<'_>,
    definitions: &Bound<'_, PyModule>,
    values: &[RuntimeType],
) -> PyResult<Vec<Py<PyAny>>> {
    values
        .iter()
        .map(|value| value.python_view(py, definitions))
        .collect()
}

fn scalar_variant(kind: ScalarKind) -> &'static str {
    match kind {
        ScalarKind::None => "NONE",
        ScalarKind::Bool => "BOOL",
        ScalarKind::Int => "INT",
        ScalarKind::Str => "STR",
        ScalarKind::Bytes => "BYTES",
    }
}
