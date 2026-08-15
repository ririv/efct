mod distribution;
mod manifest;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use distribution::{ArtifactKind, DistributionSnapshot, ModuleArtifact, normalize_name};
use efct_protocol::{ExternalSymbol, ExternalTrust};
use efct_runtime_contract_cpython::{AuditedBoundary, AuditedImplementation, BoundaryEvidence};
use manifest::{ImplementationSpec, Manifest, PythonSpec, SymbolSpec};
use pyo3::prelude::*;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Clone, Default)]
pub struct TrustSet {
    pub symbols: Vec<ExternalSymbol>,
    pub boundaries: Vec<BoundaryEvidence>,
    pub identity: Vec<(String, String)>,
    pub reports: Vec<BoundaryReport>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "trust", rename_all = "snake_case")]
pub enum BoundaryReport {
    Audited {
        path: String,
        owner: String,
        boundary_id: String,
    },
    Unsafe {
        path: String,
        reason: String,
    },
}

impl BoundaryReport {
    fn path(&self) -> &str {
        match self {
            Self::Audited { path, .. } | Self::Unsafe { path, .. } => path,
        }
    }
}

impl TrustSet {
    pub fn covers_module(&self, module: &str) -> bool {
        let prefix = format!("{module}.");
        self.boundaries
            .iter()
            .any(|boundary| boundary.path().starts_with(&prefix))
    }
}

pub fn find(py: Python<'_>, source: &Path) -> PyResult<TrustSet> {
    let Some(path) = find_manifest(py, source)? else {
        return Ok(TrustSet::default());
    };
    load_manifest(py, &path)
}

pub fn load_at(py: Python<'_>, root: &Path) -> PyResult<Option<TrustSet>> {
    let path = root.join("efct-trust.toml");
    if !path.exists() {
        return Ok(None);
    }
    validate_manifest_file(py, &path)?;
    load_manifest(py, &path).map(Some)
}

#[pyfunction]
pub fn fingerprint_distribution(py: Python<'_>, name: &str) -> PyResult<(String, String)> {
    distribution::fingerprint(py, name)
}

fn load_manifest(py: Python<'_>, path: &Path) -> PyResult<TrustSet> {
    let raw = fs::read_to_string(path).map_err(|error| {
        crate::error::startup_error(
            py,
            format!("Cannot parse trust manifest {}: {error}", path.display()),
        )
        .expect("EfctStartupError must be available")
    })?;
    let parsed = py
        .import("tomllib")?
        .getattr("loads")?
        .call1((raw,))
        .map_err(|error| {
            crate::error::startup_error(
                py,
                format!("Cannot parse trust manifest {}: {error}", path.display()),
            )
            .expect("EfctStartupError must be available")
        })?;
    let json = py
        .import("json")?
        .getattr("dumps")?
        .call1((parsed,))?
        .extract::<String>()?;
    let document: Value = serde_json::from_str(&json).map_err(|error| {
        crate::error::startup_error(
            py,
            format!("Cannot parse trust manifest {}: {error}", path.display()),
        )
        .expect("EfctStartupError must be available")
    })?;
    let manifest = manifest::parse(py, document)?;
    resolve(py, manifest)
}

fn resolve(py: Python<'_>, manifest: Manifest) -> PyResult<TrustSet> {
    let audited = manifest
        .symbols
        .iter()
        .any(|symbol| matches!(symbol, SymbolSpec::Audited { .. }));
    let python = match (&manifest.python, audited) {
        (Some(spec), true) => {
            validate_python(py, spec)?;
            Some(spec)
        }
        (None, true) => {
            return crate::error::fail(
                py,
                "A trust manifest containing audited symbols requires a python table",
            );
        }
        (Some(_), false) => {
            return crate::error::fail(
                py,
                "A trust manifest without audited symbols cannot contain a python table",
            );
        }
        (None, false) => None,
    };
    if !audited && !manifest.distributions.is_empty() {
        return crate::error::fail(
            py,
            "A trust manifest without audited symbols cannot contain distributions",
        );
    }
    let distributions = distribution::load_all(py, &manifest.distributions)?;
    let mut result = TrustSet::default();
    let mut seen_symbols = BTreeSet::new();
    let mut used_distributions = BTreeSet::new();
    for (index, symbol) in manifest.symbols.iter().enumerate() {
        if !seen_symbols.insert(symbol.path().to_owned()) {
            return crate::error::fail(py, format!("Trust symbol {} is duplicated", symbol.path()));
        }
        let boundary = match symbol {
            SymbolSpec::Audited {
                path,
                owner,
                implementation,
                signature,
                effects,
                partials,
            } => audited_boundary(
                py,
                index,
                python.expect("audited manifests have a Python specification"),
                &distributions,
                &mut used_distributions,
                path,
                owner,
                implementation,
                signature,
                effects,
                partials,
            )?,
            SymbolSpec::Unsafe {
                path,
                signature,
                effects,
                partials,
                reason,
            } => unsafe_boundary(py, index, path, signature, effects, partials, reason)?,
        };
        result
            .identity
            .push((boundary.symbol.path.clone(), boundary.identity.clone()));
        result.boundaries.push(boundary.evidence);
        result.symbols.push(boundary.symbol);
        result.reports.push(boundary.report);
    }
    if let Some(unused) = distributions
        .keys()
        .find(|name| !used_distributions.contains(*name))
    {
        return crate::error::fail(
            py,
            format!("Distribution {unused} is not used by an audited symbol"),
        );
    }
    result.identity.sort();
    result
        .boundaries
        .sort_by(|left, right| left.path().cmp(right.path()));
    result
        .symbols
        .sort_by(|left, right| left.path.cmp(&right.path));
    result
        .reports
        .sort_by(|left, right| left.path().cmp(right.path()));
    Ok(result)
}

struct ParsedBoundary {
    symbol: ExternalSymbol,
    evidence: BoundaryEvidence,
    report: BoundaryReport,
    identity: String,
}

#[allow(clippy::too_many_arguments)]
fn audited_boundary(
    py: Python<'_>,
    index: usize,
    python: &PythonSpec,
    distributions: &BTreeMap<String, DistributionSnapshot>,
    used_distributions: &mut BTreeSet<String>,
    path: &str,
    owner: &str,
    implementation: &ImplementationSpec,
    signature: &str,
    effects: &[String],
    partials: &[String],
) -> PyResult<ParsedBoundary> {
    let (parameters, returns, combined) =
        manifest::validate_contract(py, index, path, signature, effects, partials)?;
    if !manifest::qualified_identifier(py, implementation.path())?
        || !implementation.path().contains('.')
    {
        return crate::error::fail(
            py,
            format!("The implementation path in trust entry {index} must be qualified"),
        );
    }
    let Some(owner) = normalize_name(owner) else {
        return crate::error::fail(py, format!("Trust entry {index} has an invalid owner"));
    };
    let Some(owner_snapshot) = distributions.get(&owner) else {
        return crate::error::fail(
            py,
            format!("Audited symbol {path} references missing owner {owner}"),
        );
    };
    let closure = dependency_closure(py, &owner, distributions)?;
    used_distributions.extend(closure.iter().cloned());
    let (public_module, _, public_artifact) = resolve_in_distribution(py, path, owner_snapshot)?;
    let (implementation_module, implementation_qualname, implementation_artifact) =
        resolve_in_closure(py, implementation.path(), &closure, distributions)?;
    let resolved_implementation = match (implementation, implementation_artifact.kind) {
        (ImplementationSpec::Python { .. }, ArtifactKind::Python) => {
            AuditedImplementation::Python {
                module: implementation_module,
                qualname: implementation_qualname,
                artifact_path: implementation_artifact.path.to_string_lossy().into_owned(),
                artifact_sha256: implementation_artifact.sha256,
            }
        }
        (ImplementationSpec::Native { .. }, ArtifactKind::Native) => {
            AuditedImplementation::Native {
                module: implementation_module,
                qualname: implementation_qualname,
                artifact_path: implementation_artifact.path.to_string_lossy().into_owned(),
            }
        }
        (ImplementationSpec::Python { .. }, ArtifactKind::Native) => {
            return crate::error::fail(
                py,
                format!(
                    "Audited implementation {} is not a Python module",
                    implementation.path()
                ),
            );
        }
        (ImplementationSpec::Native { .. }, ArtifactKind::Python) => {
            return crate::error::fail(
                py,
                format!(
                    "Audited implementation {} is not a native extension",
                    implementation.path()
                ),
            );
        }
    };
    let boundary_id = audited_identity(
        AuditedIdentityInput {
            python,
            path,
            owner: &owner,
            implementation,
            parameters: &parameters,
            returns: &returns,
            effects,
            partials,
        },
        &closure,
        distributions,
    );
    Ok(ParsedBoundary {
        symbol: ExternalSymbol {
            path: path.to_owned(),
            parameters,
            returns,
            effects: combined,
            trust: ExternalTrust::Audited {
                evidence: boundary_id.clone(),
            },
        },
        evidence: BoundaryEvidence::Audited(AuditedBoundary {
            path: path.to_owned(),
            owner: owner.clone(),
            public_module,
            public_artifact_path: public_artifact.path.to_string_lossy().into_owned(),
            implementation: resolved_implementation,
            boundary_id: boundary_id.clone(),
        }),
        report: BoundaryReport::Audited {
            path: path.to_owned(),
            owner,
            boundary_id: boundary_id.clone(),
        },
        identity: boundary_id,
    })
}

fn unsafe_boundary(
    py: Python<'_>,
    index: usize,
    path: &str,
    signature: &str,
    effects: &[String],
    partials: &[String],
    reason: &str,
) -> PyResult<ParsedBoundary> {
    manifest::validate_non_empty(py, reason, "symbol.reason")?;
    let (parameters, returns, combined) =
        manifest::validate_contract(py, index, path, signature, effects, partials)?;
    let identity = unsafe_identity(path, &parameters, &returns, effects, partials, reason);
    Ok(ParsedBoundary {
        symbol: ExternalSymbol {
            path: path.to_owned(),
            parameters,
            returns,
            effects: combined,
            trust: ExternalTrust::Unsafe {
                reason: reason.to_owned(),
            },
        },
        evidence: BoundaryEvidence::Unsafe {
            path: path.to_owned(),
            reason: reason.to_owned(),
        },
        report: BoundaryReport::Unsafe {
            path: path.to_owned(),
            reason: reason.to_owned(),
        },
        identity,
    })
}

fn dependency_closure(
    py: Python<'_>,
    owner: &str,
    distributions: &BTreeMap<String, DistributionSnapshot>,
) -> PyResult<Vec<String>> {
    let mut pending = vec![owner.to_owned()];
    let mut closure = BTreeSet::new();
    while let Some(name) = pending.pop() {
        if !closure.insert(name.clone()) {
            continue;
        }
        let Some(distribution) = distributions.get(&name) else {
            return crate::error::fail(py, format!("Distribution {name} is missing"));
        };
        pending.extend(distribution.dependencies.iter().cloned());
    }
    Ok(closure.into_iter().collect())
}

fn resolve_in_distribution(
    py: Python<'_>,
    symbol: &str,
    distribution: &DistributionSnapshot,
) -> PyResult<(String, String, ModuleArtifact)> {
    resolve_module(symbol, distribution).ok_or_else(|| {
        crate::error::startup_error(
            py,
            format!(
                "Symbol {symbol} does not resolve to a module owned by distribution {}",
                distribution.name
            ),
        )
        .expect("EfctStartupError must be available")
    })
}

fn resolve_module(
    symbol: &str,
    distribution: &DistributionSnapshot,
) -> Option<(String, String, ModuleArtifact)> {
    let segments = symbol.split('.').collect::<Vec<_>>();
    for end in (1..segments.len()).rev() {
        let module = segments[..end].join(".");
        if let Some(artifact) = distribution.module(&module) {
            return Some((module, segments[end..].join("."), artifact.clone()));
        }
    }
    None
}

fn resolve_in_closure(
    py: Python<'_>,
    symbol: &str,
    closure: &[String],
    distributions: &BTreeMap<String, DistributionSnapshot>,
) -> PyResult<(String, String, ModuleArtifact)> {
    let mut matches = Vec::new();
    for name in closure {
        let distribution = distributions
            .get(name)
            .expect("dependency closures contain existing distributions");
        if let Some(value) = resolve_module(symbol, distribution) {
            matches.push((name, value));
        }
    }
    match matches.as_slice() {
        [(_, value)] => Ok(value.clone()),
        [] => crate::error::fail(
            py,
            format!("Implementation {symbol} is not owned by the audited distribution closure"),
        ),
        _ => crate::error::fail(
            py,
            format!("Implementation {symbol} is owned by multiple audited distributions"),
        ),
    }
}

struct AuditedIdentityInput<'a> {
    python: &'a PythonSpec,
    path: &'a str,
    owner: &'a str,
    implementation: &'a ImplementationSpec,
    parameters: &'a [String],
    returns: &'a str,
    effects: &'a [String],
    partials: &'a [String],
}

fn audited_identity(
    input: AuditedIdentityInput<'_>,
    closure: &[String],
    distributions: &BTreeMap<String, DistributionSnapshot>,
) -> String {
    let implementation = match input.implementation {
        ImplementationSpec::Python { path } => ("python", path),
        ImplementationSpec::Native { path } => ("native", path),
    };
    let mut effects = input.effects.to_vec();
    effects.sort();
    let mut partials = input.partials.to_vec();
    partials.sort();
    let identities = closure
        .iter()
        .map(|name| {
            let distribution = distributions
                .get(name)
                .expect("dependency closures contain existing distributions");
            (
                &distribution.name,
                &distribution.version,
                &distribution.installation_sha256,
                &distribution.dependencies,
            )
        })
        .collect::<Vec<_>>();
    digest_serializable(&(
        "efct-audited-boundary-v1",
        input.python,
        input.path,
        input.owner,
        implementation,
        input.parameters,
        input.returns,
        effects,
        partials,
        identities,
    ))
}

fn unsafe_identity(
    path: &str,
    parameters: &[String],
    returns: &str,
    effects: &[String],
    partials: &[String],
    reason: &str,
) -> String {
    let mut effects = effects.to_vec();
    effects.sort();
    let mut partials = partials.to_vec();
    partials.sort();
    digest_serializable(&(
        "efct-unsafe-boundary-v1",
        path,
        parameters,
        returns,
        effects,
        partials,
        reason,
    ))
}

fn digest_serializable(value: &impl Serialize) -> String {
    let bytes = serde_json::to_vec(value).expect("trust identities are serializable");
    format!("{:x}", Sha256::digest(bytes))
}

fn validate_python(py: Python<'_>, expected: &PythonSpec) -> PyResult<()> {
    if expected.implementation != "cpython" {
        return crate::error::fail(py, "Audited boundaries require CPython");
    }
    let actual_version = py
        .import("platform")?
        .call_method0("python_version")?
        .extract::<String>()?;
    if expected.version != actual_version {
        return crate::error::fail(
            py,
            format!(
                "Audited boundaries are bound to Python {}; the current version is {actual_version}",
                expected.version
            ),
        );
    }
    let implementation = py.import("sys")?.getattr("implementation")?;
    let actual_implementation = implementation.getattr("name")?.extract::<String>()?;
    if actual_implementation != expected.implementation {
        return crate::error::fail(
            py,
            format!(
                "Audited boundaries require {}; the current implementation is {actual_implementation}",
                expected.implementation
            ),
        );
    }
    let actual_cache_tag = implementation.getattr("cache_tag")?.extract::<String>()?;
    if actual_cache_tag != expected.cache_tag {
        return crate::error::fail(
            py,
            format!(
                "Audited boundaries are bound to Python cache tag {}; the current tag is {actual_cache_tag}",
                expected.cache_tag
            ),
        );
    }
    Ok(())
}

fn find_manifest(py: Python<'_>, source: &Path) -> PyResult<Option<PathBuf>> {
    let Some(mut current) = source.parent() else {
        return Ok(None);
    };
    loop {
        let candidate = current.join("efct-trust.toml");
        if candidate.exists() {
            validate_manifest_file(py, &candidate)?;
            return Ok(Some(candidate));
        }
        let Some(parent) = current.parent() else {
            return Ok(None);
        };
        current = parent;
    }
}

fn validate_manifest_file(py: Python<'_>, path: &Path) -> PyResult<()> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        crate::error::startup_error(
            py,
            format!("Cannot parse trust manifest {}: {error}", path.display()),
        )
        .expect("EfctStartupError must be available")
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return crate::error::fail(
            py,
            format!(
                "The trust manifest must be a regular file: {}",
                path.display()
            ),
        );
    }
    Ok(())
}
