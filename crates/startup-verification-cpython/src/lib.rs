mod analysis;
mod bytecode;
mod error;
mod execution;
mod function;
mod project;
mod record;
mod run_target;
mod source;
mod trust;

use pyo3::prelude::*;

pub use project::{CheckResult, CheckTarget, check_target};
pub use trust::BoundaryReport;

pub fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<execution::VerifiedSourceFinder>()?;
    module.add_class::<run_target::RunTarget>()?;
    module.add_function(wrap_pyfunction!(run_target::prepare_run_target, module)?)?;
    module.add_function(wrap_pyfunction!(run_target::verify_run_target, module)?)?;
    module.add_function(wrap_pyfunction!(run_target::run_verified_target, module)?)?;
    module.add_function(wrap_pyfunction!(function::verify_pure, module)?)?;
    module.add_function(wrap_pyfunction!(function::verify_effect, module)?)?;
    module.add_function(wrap_pyfunction!(record::verify_record, module)?)?;
    module.add_function(wrap_pyfunction!(trust::fingerprint_distribution, module)?)?;
    Ok(())
}
