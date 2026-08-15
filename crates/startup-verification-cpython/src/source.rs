use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use efct_language_python::PreparedModule;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule};
use sha2::{Digest, Sha256};

use crate::trust::TrustSet;

pub struct SourceFile {
    pub path: PathBuf,
    pub source: String,
    pub sha256: String,
}

pub struct ProjectSources {
    pub modules: Vec<(String, PreparedModule)>,
    pub root_source: String,
    pub root_sha256: String,
    pub dependency_files: Vec<DependencyFile>,
}

#[derive(Clone)]
pub struct DependencyFile {
    pub module: String,
    pub path: PathBuf,
    pub sha256: String,
}

pub fn function_source(
    py: Python<'_>,
    function: &Bound<'_, PyAny>,
    module: &Bound<'_, PyModule>,
) -> PyResult<SourceFile> {
    let filename = function
        .getattr("__code__")?
        .getattr("co_filename")?
        .extract::<String>()?;
    let module_filename = module.getattr("__file__")?.extract::<String>()?;
    if filename.is_empty() || module_filename.is_empty() {
        return crate::error::fail(py, "Library mode requires a readable .py source file");
    }
    let path = canonical_source(py, Path::new(&filename), "source")?;
    let module_path = canonical_source(py, Path::new(&module_filename), "module source")?;
    if path != module_path {
        return crate::error::fail(
            py,
            "The function code path does not match the module source path",
        );
    }
    read_source(py, path)
}

pub fn record_source(py: Python<'_>, module: &Bound<'_, PyModule>) -> PyResult<SourceFile> {
    let filename = module
        .getattr("__file__")
        .and_then(|value| value.extract::<String>())
        .map_err(|_| {
            crate::error::startup_error(py, "A pure record requires a readable .py source file")
                .expect("EfctStartupError must be available")
        })?;
    let path = canonical_source(py, Path::new(&filename), "module source")?;
    read_source(py, path)
}

pub fn module_for<'py>(
    py: Python<'py>,
    owner: &Bound<'py, PyAny>,
    subject: &str,
) -> PyResult<Bound<'py, PyModule>> {
    let name = owner
        .getattr("__module__")
        .and_then(|value| value.extract::<String>())
        .map_err(|_| {
            crate::error::startup_error(
                py,
                format!("The module containing the {subject} cannot be located"),
            )
            .expect("EfctStartupError must be available")
        })?;
    let modules = py
        .import("sys")?
        .getattr("modules")?
        .cast_into::<PyDict>()?;
    let Some(module) = modules.get_item(&name)? else {
        return crate::error::fail(
            py,
            format!("The module containing the {subject} cannot be located"),
        );
    };
    module.cast_into::<PyModule>().map_err(|_| {
        crate::error::startup_error(
            py,
            format!("The module containing the {subject} cannot be located"),
        )
        .expect("EfctStartupError must be available")
    })
}

pub fn discover_project(
    py: Python<'_>,
    root_name: &str,
    root: SourceFile,
    trust: &TrustSet,
) -> PyResult<ProjectSources> {
    let mut paths = BTreeMap::from([(root_name.to_owned(), root.path.clone())]);
    let mut pending = vec![(root_name.to_owned(), root)];
    let mut modules = Vec::new();
    let mut dependencies = Vec::new();
    let mut root_source = String::new();
    let mut root_sha256 = String::new();
    while let Some((name, file)) = pending.pop() {
        let parsed = efct_frontend_cpython::parse_source(
            py,
            &file.source,
            file.path.to_string_lossy().as_ref(),
            file.sha256.clone(),
        )?;
        let imports =
            crate::error::map_message(py, efct_language_python::module_imports(&parsed.envelope))?;
        let prepared =
            crate::error::map_message(py, efct_language_python::prepare(parsed.envelope))?;
        if name == root_name {
            root_source = file.source.clone();
            root_sha256 = file.sha256.clone();
        } else {
            dependencies.push(DependencyFile {
                module: name.clone(),
                path: file.path.clone(),
                sha256: file.sha256.clone(),
            });
        }
        modules.push((name, prepared));
        for imported_name in imports {
            if efct_language_python::python_import_role(&imported_name).is_some()
                || trust.covers_module(&imported_name)
            {
                continue;
            }
            let imported = imported_module(py, &imported_name)?;
            let imported_path = imported_path(py, &imported_name, &imported)?;
            if let Some(existing) = paths.get(&imported_name) {
                if existing != &imported_path {
                    return crate::error::fail(
                        py,
                        format!("Dependency module {imported_name} does not have a unique path"),
                    );
                }
                continue;
            }
            let imported_source = read_source(py, imported_path.clone())?;
            paths.insert(imported_name.clone(), imported_path);
            pending.push((imported_name, imported_source));
        }
    }
    modules.sort_by(|left, right| left.0.cmp(&right.0));
    dependencies.sort_by(|left, right| left.module.cmp(&right.module));
    Ok(ProjectSources {
        modules,
        root_source,
        root_sha256,
        dependency_files: dependencies,
    })
}

pub fn common_root(modules: &[(String, PreparedModule)]) -> PathBuf {
    let mut paths = modules.iter().map(|(_, module)| {
        Path::new(module.filename())
            .parent()
            .unwrap_or(Path::new(""))
    });
    let Some(first) = paths.next() else {
        return PathBuf::new();
    };
    let mut common = first.to_path_buf();
    for path in paths {
        while !path.starts_with(&common) {
            if !common.pop() {
                return PathBuf::new();
            }
        }
    }
    common
}

pub fn dependencies_match(files: &[DependencyFile]) -> bool {
    files.iter().all(|file| {
        fs::read(&file.path)
            .map(|raw| digest(&raw) == file.sha256)
            .unwrap_or(false)
    })
}

fn imported_module<'py>(py: Python<'py>, name: &str) -> PyResult<Bound<'py, PyModule>> {
    let modules = py
        .import("sys")?
        .getattr("modules")?
        .cast_into::<PyDict>()?;
    let Some(module) = modules.get_item(name)? else {
        return crate::error::fail(
            py,
            format!("Declared dependency module {name} has not been imported"),
        );
    };
    module.cast_into::<PyModule>().map_err(|_| {
        crate::error::startup_error(
            py,
            format!("Declared dependency module {name} has not been imported"),
        )
        .expect("EfctStartupError must be available")
    })
}

fn imported_path(py: Python<'_>, name: &str, module: &Bound<'_, PyModule>) -> PyResult<PathBuf> {
    let filename = module
        .getattr("__file__")
        .and_then(|value| value.extract::<String>())
        .map_err(|_| {
            crate::error::startup_error(
                py,
                format!("Dependency module {name} has no verifiable source"),
            )
            .expect("EfctStartupError must be available")
        })?;
    let path = fs::canonicalize(&filename).map_err(|error| {
        crate::error::startup_error(py, format!("Cannot read dependency module {name}: {error}"))
            .expect("EfctStartupError must be available")
    })?;
    if path.extension().and_then(|value| value.to_str()) != Some("py") || !path.is_file() {
        return crate::error::fail(
            py,
            format!("Dependency module {name} is not a verifiable .py source file"),
        );
    }
    Ok(path)
}

fn canonical_source(py: Python<'_>, path: &Path, subject: &str) -> PyResult<PathBuf> {
    let resolved = fs::canonicalize(path).map_err(|error| {
        crate::error::startup_error(
            py,
            format!("Cannot read {subject} {}: {error}", path.display()),
        )
        .expect("EfctStartupError must be available")
    })?;
    if resolved.extension().and_then(|value| value.to_str()) != Some("py") || !resolved.is_file() {
        return crate::error::fail(py, "Library mode only accepts readable .py source files");
    }
    Ok(resolved)
}

fn read_source(py: Python<'_>, path: PathBuf) -> PyResult<SourceFile> {
    let raw = fs::read(&path).map_err(|error| {
        crate::error::startup_error(
            py,
            format!("Cannot read source {}: {error}", path.display()),
        )
        .expect("EfctStartupError must be available")
    })?;
    let source = decode_source(py, &raw)?;
    Ok(SourceFile {
        path,
        sha256: digest(&raw),
        source,
    })
}

pub(crate) fn decode_source(py: Python<'_>, raw: &[u8]) -> PyResult<String> {
    efct_runtime_contract_cpython::decode_python_source(py, raw)
}

pub(crate) fn digest(raw: &[u8]) -> String {
    format!("{:x}", Sha256::digest(raw))
}
