mod analyzer;
mod api_model;
mod exceptions;
mod external;
pub mod hir;
mod lowering;
mod project;
mod runtime_plan;
mod types;

use efct_model::Diagnostic;
use efct_protocol::{ProjectEnvelope, ProtocolEnvelope};
use serde::Serialize;
use std::collections::BTreeMap;

pub use runtime_plan::{
    ExceptionBinding, ModuleMembers, NamedField, NamedRuntimeType, RuntimeCallableKind,
    RuntimePlan, RuntimeType, SymbolImport, module_imports,
};

pub fn registered_builtin_exception_names() -> impl Iterator<Item = &'static str> {
    exceptions::registered_builtin_exception_names()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PythonImportRole {
    LanguageSupport,
    ModeledApi,
}

pub fn python_import_role(module: &str) -> Option<PythonImportRole> {
    let root = module.split('.').next()?;
    if matches!(root, "__future__" | "dataclasses" | "efct" | "typing") {
        return Some(PythonImportRole::LanguageSupport);
    }
    api_model::is_module(root).then_some(PythonImportRole::ModeledApi)
}

pub fn registered_api_members() -> impl Iterator<Item = (&'static str, &'static str)> {
    api_model::operations()
        .map(|operation| {
            operation
                .name
                .split_once('.')
                .expect("registered Python API names must be qualified")
        })
        .chain(api_model::context_manager_members())
}

#[cfg(test)]
mod import_role_tests {
    use super::{PythonImportRole, python_import_role, registered_api_members};

    #[test]
    fn classifies_language_support_and_modeled_api_modules() {
        for module in ["__future__", "dataclasses", "efct", "typing"] {
            assert_eq!(
                python_import_role(module),
                Some(PythonImportRole::LanguageSupport)
            );
        }
        for (module, _) in registered_api_members() {
            assert_eq!(
                python_import_role(module),
                Some(PythonImportRole::ModeledApi)
            );
        }
        assert_eq!(
            python_import_role("os.path"),
            Some(PythonImportRole::ModeledApi)
        );
        assert_eq!(python_import_role("vendor"), None);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuntimeAnalysis {
    pub diagnostics: Vec<Diagnostic>,
    pub plans: BTreeMap<String, RuntimePlan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectRuntimeAnalysis {
    pub diagnostics: Vec<Diagnostic>,
    pub modules: BTreeMap<String, BTreeMap<String, RuntimePlan>>,
}

pub struct PreparedModule {
    filename: String,
    imports: Vec<String>,
    state: PreparedModuleState,
}

enum PreparedModuleState {
    Ready(hir::Module),
    Rejected(Vec<Diagnostic>),
}

impl PreparedModule {
    pub fn filename(&self) -> &str {
        &self.filename
    }

    pub fn imports(&self) -> &[String] {
        &self.imports
    }

    pub fn has_exception_definitions(&self) -> bool {
        matches!(&self.state, PreparedModuleState::Ready(module) if !module.exceptions.is_empty())
    }

    fn into_result(self) -> Result<hir::Module, Vec<Diagnostic>> {
        match self.state {
            PreparedModuleState::Ready(module) => Ok(module),
            PreparedModuleState::Rejected(diagnostics) => Err(diagnostics),
        }
    }
}

pub fn parse_external_signature(value: &str) -> Result<(Vec<String>, String), String> {
    external::parse_signature(value)
}

pub fn check(envelope: ProtocolEnvelope) -> Vec<Diagnostic> {
    let filename = envelope.filename.clone();
    match prepare(envelope) {
        Ok(module) => check_prepared(module),
        Err(message) => vec![Diagnostic::error("P0002", filename, None, None, message)],
    }
}

pub fn check_runtime(envelope: ProtocolEnvelope) -> Result<RuntimeAnalysis, String> {
    check_prepared_runtime(prepare(envelope)?)
}

pub fn prepare(envelope: ProtocolEnvelope) -> Result<PreparedModule, String> {
    match &envelope.language {
        efct_protocol::SourceLanguage::Python {
            implementation: efct_protocol::PythonImplementation::Cpython,
            version,
            ..
        } if efct_protocol::supports_cpython_version(*version) => {}
        efct_protocol::SourceLanguage::Python { .. } => {
            return Err(efct_protocol::SUPPORTED_CPYTHON_MESSAGE.to_owned());
        }
        efct_protocol::SourceLanguage::TypeScript { .. } => {
            return Err("The Python analyzer requires Python source".to_owned());
        }
        efct_protocol::SourceLanguage::JavaScript { .. } => {
            return Err("The Python analyzer requires Python source".to_owned());
        }
    }
    let filename = envelope.filename.clone();
    let imports = module_imports(&envelope)?;
    let state = match lowering::lower(envelope) {
        Ok(module) => PreparedModuleState::Ready(module),
        Err(diagnostics) => PreparedModuleState::Rejected(diagnostics),
    };
    Ok(PreparedModule {
        filename,
        imports,
        state,
    })
}

pub fn check_prepared(module: PreparedModule) -> Vec<Diagnostic> {
    match module.into_result() {
        Ok(module) => analyzer::analyze(&module),
        Err(diagnostics) => diagnostics,
    }
}

pub fn check_prepared_runtime(module: PreparedModule) -> Result<RuntimeAnalysis, String> {
    let module = match module.into_result() {
        Ok(module) => module,
        Err(diagnostics) => {
            return Ok(RuntimeAnalysis {
                diagnostics,
                plans: BTreeMap::new(),
            });
        }
    };
    match analyzer::analyze_runtime(&module) {
        analyzer::AnalysisResult::Accepted(runtime_types) => Ok(RuntimeAnalysis {
            diagnostics: Vec::new(),
            plans: runtime_plan::build_module_with_types(&module, &runtime_types)?,
        }),
        analyzer::AnalysisResult::Rejected(diagnostics) => Ok(RuntimeAnalysis {
            diagnostics,
            plans: BTreeMap::new(),
        }),
    }
}

pub fn check_project(envelope: ProjectEnvelope) -> Vec<Diagnostic> {
    project::check(envelope)
}

pub fn check_prepared_project(
    root: String,
    policy: efct_model::TrustPolicy,
    external_symbols: Vec<efct_protocol::ExternalSymbol>,
    modules: Vec<(String, PreparedModule)>,
) -> Vec<Diagnostic> {
    project::check_prepared(root, policy, external_symbols, modules)
}

pub fn check_runtime_project(envelope: ProjectEnvelope) -> Result<ProjectRuntimeAnalysis, String> {
    project::check_runtime(envelope)
}

pub fn check_prepared_runtime_project(
    root: String,
    external_symbols: Vec<efct_protocol::ExternalSymbol>,
    modules: Vec<(String, PreparedModule)>,
) -> Result<ProjectRuntimeAnalysis, String> {
    project::check_prepared_runtime(root, external_symbols, modules)
}
