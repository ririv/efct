use efct_model::Effect;
use pyo3::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const MANIFEST_SCHEMA: u64 = 1;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u64,
    pub python: Option<PythonSpec>,
    #[serde(default, rename = "distribution")]
    pub distributions: Vec<DistributionSpec>,
    #[serde(rename = "symbol")]
    pub symbols: Vec<SymbolSpec>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PythonSpec {
    pub implementation: String,
    pub version: String,
    pub cache_tag: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DistributionSpec {
    pub name: String,
    pub version: String,
    pub installation_sha256: String,
    pub dependencies: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "trust", rename_all = "lowercase", deny_unknown_fields)]
pub enum SymbolSpec {
    Audited {
        path: String,
        owner: String,
        implementation: ImplementationSpec,
        signature: String,
        effects: Vec<String>,
        partials: Vec<String>,
    },
    Unsafe {
        path: String,
        signature: String,
        effects: Vec<String>,
        partials: Vec<String>,
        reason: String,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum ImplementationSpec {
    Python { path: String },
    Native { path: String },
}

impl ImplementationSpec {
    pub fn path(&self) -> &str {
        match self {
            Self::Python { path } | Self::Native { path } => path,
        }
    }
}

impl SymbolSpec {
    pub fn path(&self) -> &str {
        match self {
            Self::Audited { path, .. } | Self::Unsafe { path, .. } => path,
        }
    }
}

pub fn parse(py: Python<'_>, document: Value) -> PyResult<Manifest> {
    let manifest = serde_json::from_value::<Manifest>(document).map_err(|error| {
        crate::error::startup_error(py, format!("Invalid trust manifest structure: {error}"))
            .expect("EfctStartupError must be available")
    })?;
    if manifest.schema != MANIFEST_SCHEMA {
        return crate::error::fail(
            py,
            format!(
                "Unsupported trust manifest schema {}; the current schema is {MANIFEST_SCHEMA}",
                manifest.schema
            ),
        );
    }
    if manifest.symbols.is_empty() {
        return crate::error::fail(py, "A trust manifest must contain at least one symbol");
    }
    Ok(manifest)
}

pub fn validate_contract(
    py: Python<'_>,
    index: usize,
    path: &str,
    signature: &str,
    effects: &[String],
    partials: &[String],
) -> PyResult<(Vec<String>, String, Vec<String>)> {
    if !qualified_identifier(py, path)? || !path.contains('.') {
        return crate::error::fail(
            py,
            format!("The path in trust entry {index} must be a qualified symbol name"),
        );
    }
    let (parameters, returns) = crate::error::map_message(
        py,
        efct_language_python::parse_external_signature(signature),
    )?;
    let mut combined = Vec::with_capacity(effects.len() + partials.len());
    for value in effects {
        let parsed =
            crate::error::map_message(py, Effect::parse(value).map_err(|e| e.to_string()))?;
        if parsed.is_partial() {
            return crate::error::fail(
                py,
                format!("Field effects in trust entry {index} may only contain external effects"),
            );
        }
        combined.push(value.clone());
    }
    for value in partials {
        let parsed =
            crate::error::map_message(py, Effect::parse(value).map_err(|e| e.to_string()))?;
        if !parsed.is_partial() {
            return crate::error::fail(
                py,
                format!("Field partials in trust entry {index} may only contain partial behavior"),
            );
        }
        combined.push(value.clone());
    }
    combined.sort();
    if combined.windows(2).any(|pair| pair[0] == pair[1]) {
        return crate::error::fail(
            py,
            format!("Trust entry {index} contains a duplicated effect or partial behavior"),
        );
    }
    Ok((parameters, returns, combined))
}

pub fn validate_non_empty(py: Python<'_>, value: &str, field: &str) -> PyResult<()> {
    if value.is_empty() {
        return crate::error::fail(py, format!("Field {field} must be a non-empty string"));
    }
    Ok(())
}

pub fn validate_hash(py: Python<'_>, value: &str, field: &str) -> PyResult<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return crate::error::fail(
            py,
            format!("Field {field} must be a lowercase SHA-256 digest"),
        );
    }
    Ok(())
}

pub fn qualified_identifier(py: Python<'_>, value: &str) -> PyResult<bool> {
    if value.is_empty() {
        return Ok(false);
    }
    for segment in value.split('.') {
        if !segment
            .into_pyobject(py)?
            .call_method0("isidentifier")?
            .is_truthy()?
        {
            return Ok(false);
        }
    }
    Ok(true)
}
