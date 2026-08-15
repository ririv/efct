use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyTuple};
use sha2::{Digest, Sha256};

pub enum PythonCodeSourceMatch {
    Match { fingerprint: String },
    NotFound,
    Mismatch,
}

pub fn validate_python_code_source(
    py: Python<'_>,
    code: &Bound<'_, PyAny>,
    qualified_name: &str,
    raw_source: &[u8],
) -> PyResult<PythonCodeSourceMatch> {
    let source = decode_python_source(py, raw_source)?;
    validate_python_code_text(py, code, qualified_name, &source)
}

pub fn validate_python_code_text(
    py: Python<'_>,
    code: &Bound<'_, PyAny>,
    qualified_name: &str,
    source: &str,
) -> PyResult<PythonCodeSourceMatch> {
    let filename = code.getattr("co_filename")?;
    let kwargs = PyDict::new(py);
    kwargs.set_item("dont_inherit", true)?;
    kwargs.set_item(
        "optimize",
        py.import("sys")?.getattr("flags")?.getattr("optimize")?,
    )?;
    let compiled = py
        .import("builtins")?
        .getattr("compile")?
        .call((source, filename, "exec"), Some(&kwargs))?;
    let Some(expected) = find_unique_code(py, &compiled, qualified_name)? else {
        return Ok(PythonCodeSourceMatch::NotFound);
    };
    let live_fingerprint = fingerprint(py, code)?;
    if !code.eq(expected.bind(py))? {
        return Ok(PythonCodeSourceMatch::Mismatch);
    }
    Ok(PythonCodeSourceMatch::Match {
        fingerprint: live_fingerprint,
    })
}

pub fn decode_python_source(py: Python<'_>, raw: &[u8]) -> PyResult<String> {
    let bytes = PyBytes::new(py, raw);
    let reader = py
        .import("io")?
        .getattr("BytesIO")?
        .call1((bytes.clone(),))?;
    let detected = py
        .import("tokenize")?
        .getattr("detect_encoding")?
        .call1((reader.getattr("readline")?,))?
        .cast_into::<PyTuple>()?;
    let encoding = detected.get_item(0)?;
    bytes.call_method1("decode", (encoding,))?.extract()
}

fn find_unique_code(
    py: Python<'_>,
    root: &Bound<'_, PyAny>,
    qualified_name: &str,
) -> PyResult<Option<Py<PyAny>>> {
    let code_type = py.import("types")?.getattr("CodeType")?;
    let mut pending = vec![root.clone().unbind()];
    let mut matches = Vec::new();
    while let Some(code) = pending.pop() {
        for value in code.bind(py).getattr("co_consts")?.cast::<PyTuple>()? {
            if !value.is_instance(&code_type)? {
                continue;
            }
            if value.getattr("co_qualname")?.extract::<String>()? == qualified_name {
                matches.push(value.clone().unbind());
            }
            pending.push(value.unbind());
        }
    }
    Ok((matches.len() == 1).then(|| matches.remove(0)))
}

fn fingerprint(py: Python<'_>, code: &Bound<'_, PyAny>) -> PyResult<String> {
    let payload = py
        .import("marshal")?
        .getattr("dumps")?
        .call1((code,))?
        .cast_into::<PyBytes>()?;
    Ok(format!("{:x}", Sha256::digest(payload.as_bytes())))
}
