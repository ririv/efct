mod certificate;
mod certificate_view;
mod contract;
mod dependency;
mod dependency_verifier;
mod error;
mod function;
mod python_code;

use pyo3::prelude::*;

pub use certificate::{
    AuditedBoundary, AuditedImplementation, BoundaryEvidence, CallableKind, Certificate,
    CertificateMetadata,
};
pub use function::{EffectFunction, PureFunction};
pub use function::{create_effect_function, create_pure_function};
pub use python_code::{
    PythonCodeSourceMatch, decode_python_source, validate_python_code_source,
    validate_python_code_text,
};

pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    function::initialize(module.py())?;
    module.add_class::<PureFunction>()?;
    module.add_class::<EffectFunction>()?;
    Ok(())
}
