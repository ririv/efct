use pyo3::prelude::*;

pub enum RuntimeFailure {
    Integrity(String),
    Python(PyErr),
}

impl From<PyErr> for RuntimeFailure {
    fn from(value: PyErr) -> Self {
        Self::Python(value)
    }
}

pub fn contract_error(py: Python<'_>, message: impl Into<String>) -> PyErr {
    project_error(py, "EfctContractError", message.into())
}

pub fn integrity_error(py: Python<'_>, message: impl Into<String>) -> PyErr {
    project_error(py, "EfctIntegrityError", message.into())
}

fn project_error(py: Python<'_>, name: &str, message: String) -> PyErr {
    let result = py
        .import("efct.errors")
        .and_then(|module| module.getattr(name))
        .and_then(|error_type| error_type.call1((message,)));
    match result {
        Ok(error) => PyErr::from_value(error),
        Err(import_error) => import_error,
    }
}
