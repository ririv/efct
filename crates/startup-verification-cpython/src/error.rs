use std::path::Path;

use efct_model::{Diagnostic, Severity};
use pyo3::prelude::*;
use pyo3::types::PyType;

pub fn startup_error(py: Python<'_>, message: impl Into<String>) -> PyResult<PyErr> {
    let class = py
        .import("efct.errors")?
        .getattr("EfctStartupError")?
        .cast_into::<PyType>()?;
    Ok(PyErr::from_type(class, (message.into(),)))
}

pub fn fail<T>(py: Python<'_>, message: impl Into<String>) -> PyResult<T> {
    Err(startup_error(py, message)?)
}

pub fn require_supported_runtime(py: Python<'_>) -> PyResult<()> {
    if !efct_frontend_cpython::supports_runtime(py)? {
        return fail(py, efct_model::SUPPORTED_CPYTHON_MESSAGE);
    }
    Ok(())
}

pub fn map_message<T>(py: Python<'_>, result: Result<T, String>) -> PyResult<T> {
    match result {
        Ok(value) => Ok(value),
        Err(message) => Err(startup_error(py, message)?),
    }
}

pub fn diagnostics_error(
    py: Python<'_>,
    path: &Path,
    diagnostics: &[Diagnostic],
) -> PyResult<PyErr> {
    let payload = match serde_json::to_string(diagnostics) {
        Ok(payload) => payload,
        Err(error) => {
            return startup_error(py, format!("Diagnostic serialization failed: {error}"));
        }
    };
    let values = py.import("json")?.call_method1("loads", (payload,))?;
    let i18n = py.import("efct.i18n")?;
    let language = i18n.getattr("system_language")?.call0()?;
    i18n.getattr("localize_diagnostics")?
        .call1((&values, language))?;
    let errors = values
        .try_iter()?
        .filter_map(Result::ok)
        .zip(diagnostics)
        .filter_map(|(value, diagnostic)| (diagnostic.severity == Severity::Error).then_some(value))
        .collect::<Vec<_>>();
    let errors = pyo3::types::PyList::new(py, errors)?;
    let path_object = py
        .import("pathlib")?
        .getattr("Path")?
        .call1((path.to_string_lossy().as_ref(),))?;
    let message = py
        .import("efct.startup_diagnostics")?
        .getattr("format_startup_diagnostics")?
        .call1((path_object, errors))?;
    let instance = py
        .import("efct.errors")?
        .getattr("EfctStartupError")?
        .call1((message,))?;
    Ok(PyErr::from_value(instance))
}
