use std::collections::BTreeSet;

use efct_runtime_contract_cpython::{PythonCodeSourceMatch, validate_python_code_text};
use pyo3::prelude::*;

pub struct LiveCode {
    pub code: Py<PyAny>,
    pub fingerprint: String,
    pub loaded_names: Vec<String>,
}

pub fn validate(py: Python<'_>, function: &Bound<'_, PyAny>, source: &str) -> PyResult<LiveCode> {
    let code = function.getattr("__code__")?;
    let qualified_name = function.getattr("__qualname__")?.extract::<String>()?;
    let live_fingerprint = match validate_python_code_text(py, &code, &qualified_name, source)? {
        PythonCodeSourceMatch::Match { fingerprint } => fingerprint,
        PythonCodeSourceMatch::NotFound => {
            return crate::error::fail(
                py,
                format!("Function {qualified_name} cannot be uniquely located in the source"),
            );
        }
        PythonCodeSourceMatch::Mismatch => {
            return crate::error::fail(
                py,
                format!("The live code for function {qualified_name} does not match the source"),
            );
        }
    };
    Ok(LiveCode {
        loaded_names: loaded_names(py, &code)?,
        code: code.unbind(),
        fingerprint: live_fingerprint,
    })
}

fn loaded_names(py: Python<'_>, code: &Bound<'_, PyAny>) -> PyResult<Vec<String>> {
    let instructions = py
        .import("dis")?
        .getattr("get_instructions")?
        .call1((code,))?;
    let mut names = BTreeSet::new();
    for instruction in instructions.try_iter()? {
        let instruction = instruction?;
        let opname = instruction.getattr("opname")?.extract::<String>()?;
        if opname != "LOAD_GLOBAL" && opname != "LOAD_NAME" {
            continue;
        }
        let value = instruction.getattr("argval")?;
        if let Ok(name) = value.extract::<String>() {
            names.insert(name);
        }
    }
    Ok(names.into_iter().collect())
}
