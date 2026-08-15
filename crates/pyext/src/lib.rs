use std::collections::HashSet;
use std::sync::Mutex;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict};

#[pyclass(frozen, name = "_PreparedModule", module = "efct._core")]
struct PreparedModuleHandle {
    state: Mutex<PreparedModuleState>,
}

enum PreparedModuleState {
    Ready(Box<efct_language_python::PreparedModule>),
    Consumed,
}

impl PreparedModuleHandle {
    fn new(module: efct_language_python::PreparedModule) -> Self {
        Self {
            state: Mutex::new(PreparedModuleState::Ready(Box::new(module))),
        }
    }

    fn imports(&self) -> PyResult<Vec<String>> {
        let state = self
            .state
            .lock()
            .map_err(|_| PyValueError::new_err("The prepared module lock is poisoned"))?;
        match &*state {
            PreparedModuleState::Ready(module) => Ok(module.imports().to_vec()),
            PreparedModuleState::Consumed => Err(PyValueError::new_err(
                "The prepared module has already been consumed",
            )),
        }
    }

    fn take(&self) -> PyResult<efct_language_python::PreparedModule> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| PyValueError::new_err("The prepared module lock is poisoned"))?;
        match std::mem::replace(&mut *state, PreparedModuleState::Consumed) {
            PreparedModuleState::Ready(module) => Ok(*module),
            PreparedModuleState::Consumed => Err(PyValueError::new_err(
                "The prepared module has already been consumed",
            )),
        }
    }
}

#[pyfunction]
fn check_ast(payload: &[u8]) -> PyResult<String> {
    let envelope =
        efct_protocol::decode(payload).map_err(|error| PyValueError::new_err(error.to_string()))?;
    serde_json::to_string(&efct_core::check(envelope))
        .map_err(|error| PyValueError::new_err(format!("Diagnostic serialization failed: {error}")))
}

#[pyfunction]
fn check_source(
    py: Python<'_>,
    source: &str,
    filename: &str,
    source_sha256: String,
) -> PyResult<String> {
    let parsed = efct_frontend_cpython::parse_source(py, source, filename, source_sha256)?;
    let prepared = efct_language_python::prepare(parsed.envelope).map_err(PyValueError::new_err)?;
    serde_json::to_string(&efct_language_python::check_prepared(prepared))
        .map_err(|error| PyValueError::new_err(format!("Diagnostic serialization failed: {error}")))
}

#[pyfunction]
fn encode_source<'py>(
    py: Python<'py>,
    source: &str,
    filename: &str,
    source_sha256: String,
) -> PyResult<Bound<'py, PyBytes>> {
    let parsed = efct_frontend_cpython::parse_source(py, source, filename, source_sha256)?;
    let payload = encode_envelope(&parsed.envelope)?;
    Ok(PyBytes::new(py, &payload))
}

#[pyfunction]
fn prepare_module(
    py: Python<'_>,
    source: &str,
    filename: &str,
    source_sha256: String,
) -> PyResult<PreparedModuleHandle> {
    let parsed = efct_frontend_cpython::parse_source(py, source, filename, source_sha256)?;
    let prepared = efct_language_python::prepare(parsed.envelope).map_err(PyValueError::new_err)?;
    Ok(PreparedModuleHandle::new(prepared))
}

#[pyfunction]
fn prepared_module_imports(module: PyRef<'_, PreparedModuleHandle>) -> PyResult<String> {
    let imports = module.imports()?;
    serde_json::to_string(&imports).map_err(|error| {
        PyValueError::new_err(format!("Module import serialization failed: {error}"))
    })
}

#[pyfunction]
fn check_prepared_runtime(module: PyRef<'_, PreparedModuleHandle>) -> PyResult<String> {
    let analysis = efct_language_python::check_prepared_runtime(module.take()?)
        .map_err(PyValueError::new_err)?;
    serde_json::to_string(&analysis).map_err(|error| {
        PyValueError::new_err(format!("Runtime analysis serialization failed: {error}"))
    })
}

#[pyfunction]
fn check_prepared_runtime_project(
    modules: &Bound<'_, PyDict>,
    root: &str,
    external_symbols: &str,
) -> PyResult<String> {
    let analysis = efct_language_python::check_prepared_runtime_project(
        root.to_owned(),
        decode_external_symbols(external_symbols)?,
        take_prepared_modules(modules)?,
    )
    .map_err(PyValueError::new_err)?;
    serde_json::to_string(&analysis).map_err(|error| {
        PyValueError::new_err(format!(
            "Runtime project analysis serialization failed: {error}"
        ))
    })
}

#[pyfunction]
fn check_prepared_target(
    py: Python<'_>,
    modules: &Bound<'_, PyDict>,
    root: &str,
    target: &str,
    policy: &str,
) -> PyResult<String> {
    let target = match target {
        "file" => efct_startup_verification_cpython::CheckTarget::File,
        "project" => efct_startup_verification_cpython::CheckTarget::Project,
        _ => return Err(PyValueError::new_err("The check target is invalid")),
    };
    let policy = match policy {
        "default" => efct_protocol::TrustPolicy::Default,
        "deny_unsafe" => efct_protocol::TrustPolicy::DenyUnsafe,
        "verified_only" => efct_protocol::TrustPolicy::VerifiedOnly,
        _ => return Err(PyValueError::new_err("The trust policy is invalid")),
    };
    let result = efct_startup_verification_cpython::check_target(
        py,
        std::path::Path::new(root),
        target,
        policy,
        take_prepared_modules(modules)?,
    )?;
    serde_json::to_string(&result).map_err(|error| {
        PyValueError::new_err(format!("Target check serialization failed: {error}"))
    })
}

fn encode_envelope(envelope: &efct_protocol::ProtocolEnvelope) -> PyResult<Vec<u8>> {
    let value = serde_json::to_value(envelope).map_err(|error| {
        PyValueError::new_err(format!("AST envelope conversion failed: {error}"))
    })?;
    serde_json::to_vec(&value).map_err(|error| {
        PyValueError::new_err(format!("AST envelope serialization failed: {error}"))
    })
}

fn decode_external_symbols(value: &str) -> PyResult<Vec<efct_protocol::ExternalSymbol>> {
    serde_json::from_str(value).map_err(|error| {
        PyValueError::new_err(format!("External symbol configuration is invalid: {error}"))
    })
}

fn take_prepared_modules(
    modules: &Bound<'_, PyDict>,
) -> PyResult<Vec<(String, efct_language_python::PreparedModule)>> {
    if modules.is_empty() {
        return Err(PyValueError::new_err(
            "A prepared project requires at least one module",
        ));
    }
    let mut identities = HashSet::new();
    for (name, module) in modules.iter() {
        name.extract::<String>()?;
        module.extract::<PyRef<'_, PreparedModuleHandle>>()?;
        if !identities.insert(module.as_ptr() as usize) {
            return Err(PyValueError::new_err(
                "A prepared module cannot appear more than once in a project",
            ));
        }
    }
    modules
        .iter()
        .map(|(name, module)| {
            let name: String = name.extract()?;
            let module: PyRef<'_, PreparedModuleHandle> = module.extract()?;
            Ok((name, module.take()?))
        })
        .collect()
}

#[pyfunction]
fn parse_external_signature(signature: &str) -> PyResult<String> {
    let (parameters, returns) =
        efct_language_python::parse_external_signature(signature).map_err(PyValueError::new_err)?;
    serde_json::to_string(&serde_json::json!({
        "parameters": parameters,
        "returns": returns,
    }))
    .map_err(|error| {
        PyValueError::new_err(format!("External signature serialization failed: {error}"))
    })
}

#[pyfunction]
fn check_project(payload: &[u8]) -> PyResult<String> {
    let envelope = efct_protocol::decode_project(payload)
        .map_err(|error| PyValueError::new_err(error.to_string()))?;
    serde_json::to_string(&efct_core::check_project(envelope))
        .map_err(|error| PyValueError::new_err(format!("Diagnostic serialization failed: {error}")))
}

#[pyfunction]
fn runtime_versions() -> (u32, &'static str, u32) {
    (
        efct_protocol::PROTOCOL_VERSION,
        env!("CARGO_PKG_VERSION"),
        1,
    )
}

#[pymodule]
fn _core(module: &Bound<'_, PyModule>) -> PyResult<()> {
    efct_runtime_contract_cpython::register(module)?;
    efct_startup_verification_cpython::register(module)?;
    module.add_class::<PreparedModuleHandle>()?;
    module.add_function(wrap_pyfunction!(check_ast, module)?)?;
    module.add_function(wrap_pyfunction!(check_source, module)?)?;
    module.add_function(wrap_pyfunction!(encode_source, module)?)?;
    module.add_function(wrap_pyfunction!(prepare_module, module)?)?;
    module.add_function(wrap_pyfunction!(prepared_module_imports, module)?)?;
    module.add_function(wrap_pyfunction!(check_prepared_runtime, module)?)?;
    module.add_function(wrap_pyfunction!(check_prepared_runtime_project, module)?)?;
    module.add_function(wrap_pyfunction!(check_prepared_target, module)?)?;
    module.add_function(wrap_pyfunction!(parse_external_signature, module)?)?;
    module.add_function(wrap_pyfunction!(check_project, module)?)?;
    module.add_function(wrap_pyfunction!(runtime_versions, module)?)?;
    Ok(())
}
