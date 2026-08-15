use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use efct_language_python::RuntimePlan;
use efct_model::Severity;
use pyo3::prelude::*;

use crate::source::{DependencyFile, SourceFile};
use crate::trust::TrustSet;

#[derive(Clone)]
pub struct AcceptedModule {
    pub source: String,
    pub source_sha256: String,
    pub plans: BTreeMap<String, RuntimePlan>,
    pub dependency_files: Vec<DependencyFile>,
    pub trust: TrustSet,
}

impl AcceptedModule {
    pub fn dependency_sources(&self) -> Vec<(String, String)> {
        self.dependency_files
            .iter()
            .map(|file| (file.module.clone(), file.sha256.clone()))
            .collect()
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct CacheKey {
    path: PathBuf,
    source_sha256: String,
    trust_identity: Vec<(String, String)>,
    python_version: (u8, u8, u8),
}

enum CacheEntry {
    Validating,
    Accepted(AcceptedModule),
}

static CACHE: OnceLock<Mutex<HashMap<CacheKey, CacheEntry>>> = OnceLock::new();

pub fn validate(
    py: Python<'_>,
    module_name: &str,
    root: SourceFile,
    trust: TrustSet,
) -> PyResult<AcceptedModule> {
    let version = py.version_info();
    let key = CacheKey {
        path: root.path.clone(),
        source_sha256: root.sha256.clone(),
        trust_identity: trust.identity.clone(),
        python_version: (version.major, version.minor, version.patch),
    };
    if let Some(cached) = cached(py, &key)? {
        return Ok(cached);
    }
    insert_validating(py, key.clone())?;
    let result = analyze(py, module_name, root, trust);
    match result {
        Ok(accepted) => {
            cache(py)?.insert(key, CacheEntry::Accepted(accepted.clone()));
            Ok(accepted)
        }
        Err(error) => {
            cache(py)?.remove(&key);
            Err(error)
        }
    }
}

fn analyze(
    py: Python<'_>,
    module_name: &str,
    root: SourceFile,
    trust: TrustSet,
) -> PyResult<AcceptedModule> {
    let path = root.path.clone();
    let project = crate::source::discover_project(py, module_name, root, &trust)?;
    let requires_project_exceptions = project
        .modules
        .first()
        .is_some_and(|(_, module)| module.has_exception_definitions());
    let (diagnostics, plans) = if project.modules.len() == 1
        && trust.symbols.is_empty()
        && !requires_project_exceptions
    {
        let mut modules = project.modules;
        let (_, prepared) = modules
            .pop()
            .expect("a source project always contains its root module");
        let analysis =
            crate::error::map_message(py, efct_language_python::check_prepared_runtime(prepared))?;
        (analysis.diagnostics, analysis.plans)
    } else {
        let common_root = crate::source::common_root(&project.modules);
        let analysis = crate::error::map_message(
            py,
            efct_language_python::check_prepared_runtime_project(
                common_root.to_string_lossy().into_owned(),
                trust.symbols.clone(),
                project.modules,
            ),
        )?;
        let plans = analysis
            .modules
            .get(module_name)
            .cloned()
            .unwrap_or_default();
        (analysis.diagnostics, plans)
    };
    if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error)
    {
        return Err(crate::error::diagnostics_error(py, &path, &diagnostics)?);
    }
    Ok(AcceptedModule {
        source: project.root_source,
        source_sha256: project.root_sha256,
        plans,
        dependency_files: project.dependency_files,
        trust,
    })
}

fn cached(py: Python<'_>, key: &CacheKey) -> PyResult<Option<AcceptedModule>> {
    let mut cache = cache(py)?;
    match cache.get(key) {
        Some(CacheEntry::Validating) => crate::error::fail(
            py,
            format!("Validation of module {} was re-entered", key.path.display()),
        ),
        Some(CacheEntry::Accepted(value))
            if crate::source::dependencies_match(&value.dependency_files) =>
        {
            Ok(Some(value.clone()))
        }
        Some(CacheEntry::Accepted(_)) => {
            cache.remove(key);
            Ok(None)
        }
        None => Ok(None),
    }
}

fn insert_validating(py: Python<'_>, key: CacheKey) -> PyResult<()> {
    cache(py)?.insert(key, CacheEntry::Validating);
    Ok(())
}

fn cache(
    py: Python<'_>,
) -> PyResult<std::sync::MutexGuard<'static, HashMap<CacheKey, CacheEntry>>> {
    CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| {
            crate::error::startup_error(py, "The startup verification cache lock is poisoned")
                .expect("EfctStartupError must be available")
        })
}
