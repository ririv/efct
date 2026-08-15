use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use efct_language_python::PreparedModule;
use efct_model::{Severity, TrustPolicy};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyList;

use crate::execution::VerifiedSource;
use crate::project::CheckTarget;
use crate::trust::TrustSet;

#[pyclass(name = "_RunTarget", module = "efct._core")]
pub struct RunTarget {
    state: Mutex<RunTargetState>,
}

enum RunTargetState {
    Prepared(PreparedRunTarget),
    Verified(VerifiedRunTarget),
    Rejected,
    Consumed,
}

struct PreparedRunTarget {
    root: PathBuf,
    entry_module: String,
    modules: Vec<(String, PreparedModule)>,
    sources: BTreeMap<String, VerifiedSource>,
    trust: Option<TrustSet>,
}

struct VerifiedRunTarget {
    entry_module: String,
    sources: BTreeMap<String, VerifiedSource>,
}

#[pyfunction]
pub fn prepare_run_target(py: Python<'_>, target: &str) -> PyResult<RunTarget> {
    crate::error::require_supported_runtime(py)?;
    let entry = canonical_entry(py, Path::new(target))?;
    let root = entry
        .parent()
        .expect("a canonical file has a parent")
        .to_path_buf();
    let trust = crate::trust::load_at(py, &root)?;
    let prepared = capture_project(py, root, entry, trust)?;
    Ok(RunTarget {
        state: Mutex::new(RunTargetState::Prepared(prepared)),
    })
}

#[pyfunction]
pub fn verify_run_target(py: Python<'_>, target: PyRef<'_, RunTarget>) -> PyResult<String> {
    let mut state = target
        .state
        .lock()
        .map_err(|_| PyValueError::new_err("The run target lock is poisoned"))?;
    let prepared = match std::mem::replace(&mut *state, RunTargetState::Consumed) {
        RunTargetState::Prepared(prepared) => prepared,
        RunTargetState::Verified(verified) => {
            *state = RunTargetState::Verified(verified);
            return Err(PyValueError::new_err(
                "The run target has already been verified",
            ));
        }
        RunTargetState::Rejected => {
            *state = RunTargetState::Rejected;
            return Err(PyValueError::new_err("The run target has been rejected"));
        }
        RunTargetState::Consumed => {
            return Err(PyValueError::new_err(
                "The run target has already been consumed",
            ));
        }
    };
    let current_trust = match crate::trust::load_at(py, &prepared.root) {
        Ok(trust) => trust,
        Err(error) => {
            *state = RunTargetState::Rejected;
            return Err(error);
        }
    };
    if prepared.trust.as_ref().map(|value| &value.identity)
        != current_trust.as_ref().map(|value| &value.identity)
    {
        *state = RunTargetState::Rejected;
        return crate::error::fail(
            py,
            "The trust manifest changed after run target preparation",
        );
    }
    let result = match crate::project::check_with_trust(
        py,
        &prepared.root,
        CheckTarget::Project,
        TrustPolicy::Default,
        prepared.modules,
        current_trust,
    ) {
        Ok(result) => result,
        Err(error) => {
            *state = RunTargetState::Rejected;
            return Err(error);
        }
    };
    let accepted = !result
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error);
    *state = if accepted {
        RunTargetState::Verified(VerifiedRunTarget {
            entry_module: prepared.entry_module,
            sources: prepared.sources,
        })
    } else {
        RunTargetState::Rejected
    };
    serde_json::to_string(&result).map_err(|error| {
        PyValueError::new_err(format!("Run target check serialization failed: {error}"))
    })
}

#[pyfunction]
pub fn run_verified_target(
    py: Python<'_>,
    target: PyRef<'_, RunTarget>,
    arguments: &Bound<'_, PyList>,
) -> PyResult<()> {
    let verified = {
        let mut state = target
            .state
            .lock()
            .map_err(|_| PyValueError::new_err("The run target lock is poisoned"))?;
        match std::mem::replace(&mut *state, RunTargetState::Consumed) {
            RunTargetState::Verified(verified) => verified,
            RunTargetState::Prepared(prepared) => {
                *state = RunTargetState::Prepared(prepared);
                return Err(PyValueError::new_err(
                    "The run target must be verified before execution",
                ));
            }
            RunTargetState::Rejected => {
                *state = RunTargetState::Rejected;
                return Err(PyValueError::new_err(
                    "A rejected run target cannot be executed",
                ));
            }
            RunTargetState::Consumed => {
                return Err(PyValueError::new_err(
                    "The run target has already been consumed",
                ));
            }
        }
    };
    crate::execution::execute(py, &verified.entry_module, verified.sources, arguments)
}

fn capture_project(
    py: Python<'_>,
    root: PathBuf,
    entry: PathBuf,
    trust: Option<TrustSet>,
) -> PyResult<PreparedRunTarget> {
    let entry_module = module_name(py, &root, &entry)?;
    let mut pending = vec![entry];
    let mut visited = BTreeSet::new();
    let mut paths = BTreeMap::new();
    let mut modules = Vec::new();
    let mut sources = BTreeMap::new();
    while let Some(path) = pending.pop() {
        if !visited.insert(path.clone()) {
            continue;
        }
        let name = module_name(py, &root, &path)?;
        if let Some(existing) = paths.insert(name.clone(), path.clone()) {
            if existing != path {
                return crate::error::fail(
                    py,
                    format!("Module name {name} is defined by multiple source files"),
                );
            }
        }
        let raw = fs::read(&path).map_err(|error| {
            crate::error::startup_error(
                py,
                format!("Cannot read source {}: {error}", path.display()),
            )
            .expect("EfctStartupError must be available")
        })?;
        let source = crate::source::decode_source(py, &raw)?;
        let parsed = efct_frontend_cpython::parse_source(
            py,
            &source,
            path.to_string_lossy().as_ref(),
            crate::source::digest(&raw),
        )?;
        let imports =
            crate::error::map_message(py, efct_language_python::module_imports(&parsed.envelope))?;
        let prepared =
            crate::error::map_message(py, efct_language_python::prepare(parsed.envelope))?;
        let is_package = path.file_name().and_then(|value| value.to_str()) == Some("__init__.py");
        modules.push((name.clone(), prepared));
        sources.insert(
            name,
            VerifiedSource::new(raw, path.to_string_lossy().into_owned(), is_package),
        );
        for imported in imports {
            if efct_language_python::python_import_role(&imported).is_some()
                || trust
                    .as_ref()
                    .is_some_and(|boundaries| boundaries.covers_module(&imported))
            {
                continue;
            }
            pending.extend(local_dependency_paths(py, &root, &imported)?);
        }
    }
    modules.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(PreparedRunTarget {
        root,
        entry_module,
        modules,
        sources,
        trust,
    })
}

fn canonical_entry(py: Python<'_>, target: &Path) -> PyResult<PathBuf> {
    if !target.exists() {
        return crate::error::fail(py, format!("Path does not exist: {}", target.display()));
    }
    let path = fs::canonicalize(target).map_err(|error| {
        crate::error::startup_error(
            py,
            format!("Cannot read run target {}: {error}", target.display()),
        )
        .expect("EfctStartupError must be available")
    })?;
    if path.extension().and_then(|value| value.to_str()) != Some("py") || !path.is_file() {
        return crate::error::fail(
            py,
            format!(
                "The run target must be one Python file: {}",
                target.display()
            ),
        );
    }
    Ok(path)
}

fn module_name(py: Python<'_>, root: &Path, path: &Path) -> PyResult<String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| PyValueError::new_err("A run target module is outside the project root"))?;
    let without_extension = relative.with_extension("");
    let mut parts = without_extension
        .iter()
        .map(|part| part.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    if parts.last().is_some_and(|part| part == "__init__") {
        parts.pop();
    }
    if parts.is_empty() {
        return crate::error::fail(py, "A project root cannot contain only __init__.py");
    }
    for part in &parts {
        if !part
            .into_pyobject(py)?
            .call_method0("isidentifier")?
            .is_truthy()?
        {
            return crate::error::fail(
                py,
                format!(
                    "The module path is not a valid Python qualified name: {}",
                    relative.display()
                ),
            );
        }
    }
    Ok(parts.join("."))
}

fn local_dependency_paths(py: Python<'_>, root: &Path, module: &str) -> PyResult<Vec<PathBuf>> {
    let parts = module.split('.').collect::<Vec<_>>();
    let mut discovered = Vec::new();
    for length in 1..parts.len() {
        let package = root
            .join(parts[..length].iter().collect::<PathBuf>())
            .join("__init__.py");
        if package.is_file() && !package.is_symlink() {
            discovered.push(fs::canonicalize(package)?);
        }
    }
    let module_path = root.join(parts.iter().collect::<PathBuf>());
    let candidates = [
        module_path.with_extension("py"),
        module_path.join("__init__.py"),
    ]
    .into_iter()
    .filter(|path| path.is_file() && !path.is_symlink())
    .map(fs::canonicalize)
    .collect::<Result<Vec<_>, _>>()?;
    if candidates.len() > 1 {
        return crate::error::fail(
            py,
            format!("Module name {module} is defined by multiple source files"),
        );
    }
    discovered.extend(candidates);
    Ok(discovered)
}
