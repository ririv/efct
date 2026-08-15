use pyo3::prelude::*;
use pyo3::types::{PyTuple, PyType};

#[pyfunction]
pub fn verify_record(py: Python<'_>, record: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    crate::error::require_supported_runtime(py)?;
    if !record.get_type().is(py.get_type::<PyType>()) {
        return crate::error::fail(
            py,
            "Pure record verification only accepts regular Python classes",
        );
    }
    let module = crate::source::module_for(py, record, "pure record")?;
    let dataclasses = py.import("dataclasses")?;
    let parameters = record.getattr("__dataclass_params__").ok();
    let is_dataclass = dataclasses
        .getattr("is_dataclass")?
        .call1((record,))?
        .is_truthy()?;
    let frozen = match &parameters {
        Some(value) => value.getattr("frozen")?.is_truthy()?,
        None => false,
    };
    if !is_dataclass || !frozen {
        return crate::error::fail(
            py,
            "A pure record requires @dataclass(frozen=True, slots=True)",
        );
    }
    let slots = record.getattr("__slots__").ok();
    if !slots.is_some_and(|value| value.is_instance_of::<PyTuple>()) {
        return crate::error::fail(py, "A pure record requires slots=True");
    }
    let fields = dataclasses
        .getattr("fields")?
        .call1((record,))?
        .cast_into::<PyTuple>()?;
    if fields.is_empty() {
        return crate::error::fail(py, "A pure record requires at least one field");
    }
    let missing = dataclasses.getattr("MISSING")?;
    let mut names = Vec::new();
    for field in fields {
        if !field.getattr("default")?.is(&missing)
            || !field.getattr("default_factory")?.is(&missing)
        {
            return crate::error::fail(
                py,
                "Pure record fields cannot have defaults or default factories",
            );
        }
        names.push(field.getattr("name")?.extract::<String>()?);
    }
    let source = crate::source::record_source(py, &module)?;
    let trust = crate::trust::find(py, &source.path)?;
    let module_name = record.getattr("__module__")?.extract::<String>()?;
    crate::analysis::validate(py, &module_name, source, trust)?;
    py.import("efct.values")?
        .getattr("_register_pure_record")?
        .call1((record, PyTuple::new(py, names)?))?;
    Ok(record.clone().unbind())
}
