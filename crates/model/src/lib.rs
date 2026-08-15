mod diagnostics;
mod effects;

pub use diagnostics::{Diagnostic, EffectTraceFrame, Severity, SourceSpan};
pub use effects::{
    Effect, EffectFormula, EffectParseError, EffectSet, EffectTerm, EffectVariable, ExceptionId,
    ExceptionIdError, ExternalEffect, PartialBehavior,
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustPolicy {
    Default,
    DenyUnsafe,
    VerifiedOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LanguageIdentity {
    Python {
        implementation: PythonImplementation,
        version: [u8; 3],
    },
    TypeScript {
        compiler_version: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PythonImplementation {
    Cpython,
}

pub const SUPPORTED_CPYTHON_MESSAGE: &str =
    "The current version only supports CPython 3.13 and 3.14";

pub fn supports_cpython_version(version: [u8; 3]) -> bool {
    matches!((version[0], version[1]), (3, 13) | (3, 14))
}
