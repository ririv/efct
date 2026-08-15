use efct_runtime_contract_cpython::{
    CallableKind, Certificate, CertificateMetadata, EffectFunction, PureFunction,
    create_effect_function, create_pure_function,
};
use pyo3::prelude::*;

enum Declaration {
    InferredPure,
    BoundedPure(Vec<String>),
    InferredEffects,
    BoundedEffects(Vec<String>),
}

impl Declaration {
    fn certificate_kind(&self) -> CallableKind {
        match self {
            Self::InferredPure => CallableKind::InferredPure,
            Self::BoundedPure(_) => CallableKind::BoundedPure,
            Self::InferredEffects => CallableKind::InferredEffect,
            Self::BoundedEffects(_) => CallableKind::BoundedEffect,
        }
    }

    fn effects(&self) -> &[String] {
        match self {
            Self::InferredPure | Self::InferredEffects => &[],
            Self::BoundedPure(partials) | Self::BoundedEffects(partials) => partials,
        }
    }
}

#[pyfunction]
pub fn verify_pure(
    py: Python<'_>,
    function: &Bound<'_, PyAny>,
    declared_partials: Option<Vec<String>>,
) -> PyResult<Py<PureFunction>> {
    let declaration = match declared_partials {
        Some(partials) => Declaration::BoundedPure(partials),
        None => Declaration::InferredPure,
    };
    let verified = verify(py, function, declaration)?;
    Py::new(
        py,
        create_pure_function(verified.code, verified.module, verified.certificate)?,
    )
}

#[pyfunction]
pub fn verify_effect(
    py: Python<'_>,
    function: &Bound<'_, PyAny>,
    declared_effects: Option<Vec<String>>,
) -> PyResult<Py<EffectFunction>> {
    let declaration = match declared_effects {
        Some(effects) => Declaration::BoundedEffects(effects),
        None => Declaration::InferredEffects,
    };
    let verified = verify(py, function, declaration)?;
    Py::new(
        py,
        create_effect_function(verified.code, verified.module, verified.certificate)?,
    )
}

struct VerifiedFunction {
    code: Py<PyAny>,
    module: Py<PyModule>,
    certificate: Certificate,
}

fn verify(
    py: Python<'_>,
    function: &Bound<'_, PyAny>,
    declaration: Declaration,
) -> PyResult<VerifiedFunction> {
    require_supported_function(py, function)?;
    let module = crate::source::module_for(py, function, "function")?;
    let source = crate::source::function_source(py, function, &module)?;
    let trust = crate::trust::find(py, &source.path)?;
    let module_name = function.getattr("__module__")?.extract::<String>()?;
    let accepted = crate::analysis::validate(py, &module_name, source, trust)?;
    let live = crate::bytecode::validate(py, function, &accepted.source)?;
    let function_name = function.getattr("__name__")?.extract::<String>()?;
    let Some(plan) = accepted.plans.get(&function_name).cloned() else {
        return crate::error::fail(
            py,
            format!("Function {function_name} cannot be uniquely located in the module source"),
        );
    };
    let version = py.version_info();
    let dependency_sources = accepted.dependency_sources();
    let metadata = CertificateMetadata {
        module_name,
        function_name,
        dependency_names: live.loaded_names,
        source_sha256: accepted.source_sha256,
        dependency_sources,
        code_fingerprint: live.fingerprint,
        python_version: (version.major, version.minor, version.patch),
        protocol_version: efct_protocol::PROTOCOL_VERSION,
        core_version: env!("CARGO_PKG_VERSION").to_owned(),
        registry_version: 1,
        boundaries: accepted.trust.boundaries,
    };
    let certificate = match Certificate::from_plan(
        py,
        &module,
        plan,
        declaration.certificate_kind(),
        declaration.effects(),
        metadata,
    ) {
        Ok(certificate) => certificate,
        Err(error) => {
            let message = error.value(py).str()?.to_string_lossy().into_owned();
            return crate::error::fail(py, message);
        }
    };
    Ok(VerifiedFunction {
        code: live.code,
        module: module.unbind(),
        certificate,
    })
}

fn require_supported_function(py: Python<'_>, function: &Bound<'_, PyAny>) -> PyResult<()> {
    crate::error::require_supported_runtime(py)?;
    let function_type = py.import("types")?.getattr("FunctionType")?;
    if !function.is_instance(&function_type)? {
        return crate::error::fail(py, "Efct decorators only accept Python functions");
    }
    let has_closure = !function.getattr("__closure__")?.is_none();
    let has_defaults = !function.getattr("__defaults__")?.is_none();
    let has_keyword_defaults = !function.getattr("__kwdefaults__")?.is_none();
    if has_closure || has_defaults || has_keyword_defaults {
        return crate::error::fail(
            py,
            "Closures and default arguments are not supported in the MVP",
        );
    }
    Ok(())
}
