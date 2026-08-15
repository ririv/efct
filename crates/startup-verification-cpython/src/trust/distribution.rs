use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use pyo3::prelude::*;
use sha2::{Digest, Sha256};

use super::manifest::{DistributionSpec, validate_hash, validate_non_empty};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtifactKind {
    Python,
    Native,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleArtifact {
    pub kind: ArtifactKind,
    pub path: PathBuf,
    pub sha256: String,
}

#[derive(Clone, Debug)]
pub struct DistributionSnapshot {
    pub name: String,
    pub version: String,
    pub installation_sha256: String,
    pub dependencies: Vec<String>,
    modules: BTreeMap<String, ModuleArtifact>,
}

impl DistributionSnapshot {
    pub fn module(&self, name: &str) -> Option<&ModuleArtifact> {
        self.modules.get(name)
    }
}

pub fn normalize_name(value: &str) -> Option<String> {
    if value.is_empty()
        || !value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        || !value
            .bytes()
            .last()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return None;
    }
    let mut normalized = String::new();
    let mut separator = false;
    for byte in value.bytes() {
        if matches!(byte, b'-' | b'_' | b'.') {
            separator = true;
        } else {
            if separator && !normalized.is_empty() {
                normalized.push('-');
            }
            separator = false;
            normalized.push(char::from(byte.to_ascii_lowercase()));
        }
    }
    Some(normalized)
}

pub fn load_all(
    py: Python<'_>,
    specs: &[DistributionSpec],
) -> PyResult<BTreeMap<String, DistributionSnapshot>> {
    let suffixes = extension_suffixes(py)?;
    let mut snapshots = BTreeMap::new();
    for (index, spec) in specs.iter().enumerate() {
        validate_non_empty(py, &spec.name, "distribution.name")?;
        validate_non_empty(py, &spec.version, "distribution.version")?;
        validate_hash(
            py,
            &spec.installation_sha256,
            "distribution.installation_sha256",
        )?;
        let Some(name) = normalize_name(&spec.name) else {
            return crate::error::fail(
                py,
                format!("Distribution entry {index} has an invalid name"),
            );
        };
        if snapshots.contains_key(&name) {
            return crate::error::fail(py, format!("Distribution {name} is duplicated"));
        }
        let mut dependencies = Vec::with_capacity(spec.dependencies.len());
        let mut seen_dependencies = BTreeSet::new();
        for dependency in &spec.dependencies {
            let Some(dependency) = normalize_name(dependency) else {
                return crate::error::fail(
                    py,
                    format!("Distribution {name} has an invalid dependency name"),
                );
            };
            if !seen_dependencies.insert(dependency.clone()) {
                return crate::error::fail(
                    py,
                    format!("Distribution {name} contains dependency {dependency} more than once"),
                );
            }
            dependencies.push(dependency);
        }
        dependencies.sort();
        let snapshot = load(py, spec, name.clone(), dependencies, &suffixes)?;
        snapshots.insert(name, snapshot);
    }
    for snapshot in snapshots.values() {
        for dependency in &snapshot.dependencies {
            if !snapshots.contains_key(dependency) {
                return crate::error::fail(
                    py,
                    format!(
                        "Distribution {} references missing dependency {dependency}",
                        snapshot.name
                    ),
                );
            }
        }
    }
    Ok(snapshots)
}

pub fn fingerprint(py: Python<'_>, requested: &str) -> PyResult<(String, String)> {
    let Some(name) = normalize_name(requested) else {
        return crate::error::fail(py, "The distribution name is invalid");
    };
    let suffixes = extension_suffixes(py)?;
    let snapshot = inspect(py, requested, name, Vec::new(), &suffixes)?;
    Ok((snapshot.version, snapshot.installation_sha256))
}

fn load(
    py: Python<'_>,
    spec: &DistributionSpec,
    name: String,
    dependencies: Vec<String>,
    extension_suffixes: &[String],
) -> PyResult<DistributionSnapshot> {
    let snapshot = inspect(
        py,
        &spec.name,
        name.clone(),
        dependencies,
        extension_suffixes,
    )?;
    if snapshot.version != spec.version {
        return crate::error::fail(
            py,
            format!(
                "Distribution {name} is bound to version {}; the current version is {}",
                spec.version, snapshot.version
            ),
        );
    }
    if snapshot.installation_sha256 != spec.installation_sha256 {
        return crate::error::fail(
            py,
            format!("The installation digest for distribution {name} is no longer valid"),
        );
    }
    Ok(snapshot)
}

fn inspect(
    py: Python<'_>,
    requested: &str,
    name: String,
    dependencies: Vec<String>,
    extension_suffixes: &[String],
) -> PyResult<DistributionSnapshot> {
    let metadata = py.import("importlib.metadata")?;
    let distribution = metadata
        .getattr("distribution")?
        .call1((requested,))
        .map_err(|_| {
            crate::error::startup_error(py, format!("Distribution {requested} is not installed"))
                .expect("EfctStartupError must be available")
        })?;
    reject_editable(py, &distribution, &name)?;
    let actual_version = distribution.getattr("version")?.extract::<String>()?;
    let metadata_name = distribution
        .getattr("metadata")?
        .call_method1("__getitem__", ("Name",))?
        .extract::<String>()?;
    if normalize_name(&metadata_name).as_deref() != Some(name.as_str()) {
        return crate::error::fail(
            py,
            format!("Distribution {name} has inconsistent installed metadata"),
        );
    }
    let files = distribution.getattr("files")?;
    if files.is_none() {
        return crate::error::fail(
            py,
            format!("Distribution {name} has no installed file record"),
        );
    }
    let mut installed = Vec::new();
    let mut logical_paths = BTreeSet::new();
    let mut canonical_paths = BTreeSet::new();
    for item in files.try_iter()? {
        let item = item?;
        let logical = item.str()?.to_string_lossy().replace('\\', "/");
        if logical.is_empty() || logical.as_bytes().contains(&0) || logical.ends_with(".pyc") {
            continue;
        }
        if !logical_paths.insert(logical.clone()) {
            return crate::error::fail(
                py,
                format!("Distribution {name} contains duplicate installed path {logical}"),
            );
        }
        let located = item
            .call_method0("locate")?
            .str()?
            .to_string_lossy()
            .into_owned();
        let path = PathBuf::from(located);
        let file_type = fs::symlink_metadata(&path)
            .map_err(|error| {
                crate::error::startup_error(
                    py,
                    format!(
                        "Cannot inspect distribution file {}: {error}",
                        path.display()
                    ),
                )
                .expect("EfctStartupError must be available")
            })?
            .file_type();
        if file_type.is_symlink() || !file_type.is_file() {
            return crate::error::fail(
                py,
                format!(
                    "Distribution file {} must be a regular file and cannot be a symbolic link",
                    path.display()
                ),
            );
        }
        let canonical = fs::canonicalize(&path)?;
        if !canonical_paths.insert(canonical.clone()) {
            return crate::error::fail(
                py,
                format!(
                    "Distribution {name} maps multiple installed paths to {}",
                    canonical.display()
                ),
            );
        }
        let bytes = fs::read(&canonical).map_err(|error| {
            crate::error::startup_error(
                py,
                format!(
                    "Cannot read distribution file {}: {error}",
                    canonical.display()
                ),
            )
            .expect("EfctStartupError must be available")
        })?;
        installed.push((logical, canonical, bytes));
    }
    installed.sort_by(|left, right| left.0.cmp(&right.0));
    let actual_digest = installation_digest(&installed);
    let modules = module_index(py, &name, &installed, extension_suffixes)?;
    Ok(DistributionSnapshot {
        name,
        version: actual_version,
        installation_sha256: actual_digest,
        dependencies,
        modules,
    })
}

fn reject_editable(py: Python<'_>, distribution: &Bound<'_, PyAny>, name: &str) -> PyResult<()> {
    let origin = distribution.getattr("origin")?;
    if origin.is_none() {
        return Ok(());
    }
    let dir_info = origin.getattr("dir_info")?;
    if !dir_info.is_none() && dir_info.getattr("editable")?.extract::<bool>()? {
        return crate::error::fail(
            py,
            format!("Audited distribution {name} cannot be installed in editable mode"),
        );
    }
    Ok(())
}

fn installation_digest(installed: &[(String, PathBuf, Vec<u8>)]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"efct-installation-v1\0");
    for (logical, _, bytes) in installed {
        update_length(&mut hasher, logical.len());
        hasher.update(logical.as_bytes());
        update_length(&mut hasher, bytes.len());
        hasher.update(bytes);
    }
    format!("{:x}", hasher.finalize())
}

fn update_length(hasher: &mut Sha256, length: usize) {
    hasher.update(
        u64::try_from(length)
            .expect("file lengths fit in u64")
            .to_be_bytes(),
    );
}

fn module_index(
    py: Python<'_>,
    distribution: &str,
    installed: &[(String, PathBuf, Vec<u8>)],
    extension_suffixes: &[String],
) -> PyResult<BTreeMap<String, ModuleArtifact>> {
    let mut modules = BTreeMap::new();
    for (logical, path, bytes) in installed {
        let candidate = if let Some(value) = logical.strip_suffix("/__init__.py") {
            Some((value, ArtifactKind::Python))
        } else if let Some(value) = logical.strip_suffix(".py") {
            Some((value, ArtifactKind::Python))
        } else {
            extension_suffixes.iter().find_map(|suffix| {
                logical
                    .strip_suffix(suffix)
                    .map(|value| (value, ArtifactKind::Native))
            })
        };
        let Some((module_path, kind)) = candidate else {
            continue;
        };
        let module = module_path.replace('/', ".");
        if module.is_empty()
            || module
                .split('.')
                .any(|segment| segment.is_empty() || segment == "..")
            || !super::manifest::qualified_identifier(py, &module)?
        {
            continue;
        }
        let artifact = ModuleArtifact {
            kind,
            path: path.clone(),
            sha256: format!("{:x}", Sha256::digest(bytes)),
        };
        if let Some(existing) = modules.insert(module.clone(), artifact.clone())
            && existing != artifact
        {
            return crate::error::fail(
                py,
                format!("Distribution {distribution} defines module {module} more than once"),
            );
        }
    }
    Ok(modules)
}

fn extension_suffixes(py: Python<'_>) -> PyResult<Vec<String>> {
    let mut suffixes = py
        .import("importlib.machinery")?
        .getattr("EXTENSION_SUFFIXES")?
        .extract::<Vec<String>>()?;
    suffixes.sort_by_key(|suffix| std::cmp::Reverse(suffix.len()));
    Ok(suffixes)
}
