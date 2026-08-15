use std::path::Path;

use efct_language_python::PreparedModule;
use efct_model::{Diagnostic, TrustPolicy};
use pyo3::prelude::*;
use serde::Serialize;

use crate::BoundaryReport;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckTarget {
    File,
    Project,
}

#[derive(Debug, Serialize)]
pub struct CheckResult {
    pub diagnostics: Vec<Diagnostic>,
    pub trusted_boundaries: Vec<BoundaryReport>,
}

pub fn check_target(
    py: Python<'_>,
    root: &Path,
    target: CheckTarget,
    policy: TrustPolicy,
    modules: Vec<(String, PreparedModule)>,
) -> PyResult<CheckResult> {
    let trust = crate::trust::load_at(py, root)?;
    check_with_trust(py, root, target, policy, modules, trust)
}

pub(crate) fn check_with_trust(
    py: Python<'_>,
    root: &Path,
    target: CheckTarget,
    policy: TrustPolicy,
    mut modules: Vec<(String, PreparedModule)>,
    trust: Option<crate::trust::TrustSet>,
) -> PyResult<CheckResult> {
    let diagnostics = match (target, trust.as_ref()) {
        (CheckTarget::File, None)
            if modules
                .first()
                .is_some_and(|(_, module)| !module.has_exception_definitions()) =>
        {
            if modules.len() != 1 {
                return crate::error::fail(py, "A file check requires exactly one prepared module");
            }
            let (_, module) = modules.pop().expect("one module was checked");
            efct_language_python::check_prepared(module)
        }
        _ => efct_language_python::check_prepared_project(
            root.to_string_lossy().into_owned(),
            policy,
            trust
                .as_ref()
                .map(|value| value.symbols.clone())
                .unwrap_or_default(),
            modules,
        ),
    };
    Ok(CheckResult {
        diagnostics,
        trusted_boundaries: trust.map(|value| value.reports).unwrap_or_default(),
    })
}
