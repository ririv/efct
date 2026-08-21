pub use efct_model::{Diagnostic, LanguageIdentity, Severity};
use efct_protocol::{ProjectEnvelope, ProtocolEnvelope, SourceLanguage};

pub fn check(envelope: ProtocolEnvelope) -> Vec<Diagnostic> {
    match &envelope.language {
        SourceLanguage::Python { .. } => efct_language_python::check(envelope),
        SourceLanguage::TypeScript { .. } | SourceLanguage::JavaScript { .. } => {
            efct_language_typescript::check(envelope)
        }
    }
}

pub fn check_project(envelope: ProjectEnvelope) -> Vec<Diagnostic> {
    match &envelope.language {
        LanguageIdentity::Python { .. } => efct_language_python::check_project(envelope),
        LanguageIdentity::TypeScript { .. } | LanguageIdentity::JavaScript { .. } => {
            efct_language_typescript::check_project(envelope)
        }
    }
}

#[cfg(test)]
mod tests {
    use efct_model::{NodeRuntimeIdentity, PythonImplementation, TypeScriptCompilerIdentity};
    use efct_protocol::{EcmaModuleNode, ModuleKind, ModuleNode};

    use super::*;

    fn envelope(language: SourceLanguage) -> ProtocolEnvelope {
        ProtocolEnvelope {
            protocol_version: efct_protocol::PROTOCOL_VERSION,
            language,
            filename: "empty.py".to_owned(),
            source_sha256: "a".repeat(64),
        }
    }

    #[test]
    fn dispatches_an_empty_python_module_to_the_language_analyzer() {
        for version in [[3, 13, 9], [3, 14, 7]] {
            let diagnostics = check(envelope(SourceLanguage::Python {
                implementation: PythonImplementation::Cpython,
                version,
                root: ModuleNode {
                    kind: ModuleKind::Module,
                    items: Vec::new(),
                },
            }));
            assert!(diagnostics.is_empty());
        }
    }

    #[test]
    fn rejects_an_unsupported_cpython_version() {
        let diagnostics = check(envelope(SourceLanguage::Python {
            implementation: PythonImplementation::Cpython,
            version: [3, 12, 11],
            root: ModuleNode {
                kind: ModuleKind::Module,
                items: Vec::new(),
            },
        }));
        assert_eq!(diagnostics[0].code, "P0002");
        assert_eq!(
            diagnostics[0].message,
            efct_model::SUPPORTED_CPYTHON_MESSAGE
        );
    }

    #[test]
    fn dispatches_empty_typescript_to_the_language_analyzer() {
        let diagnostics = check(envelope(SourceLanguage::TypeScript {
            compiler: TypeScriptCompilerIdentity {
                version: efct_language_typescript::SUPPORTED_TYPESCRIPT_VERSION.to_owned(),
                installation_sha256:
                    efct_language_typescript::SUPPORTED_TYPESCRIPT_INSTALLATION_SHA256.to_owned(),
            },
            runtime: NodeRuntimeIdentity {
                version: [24, 19, 0],
                node_api_version: 8,
            },
            config_sha256: efct_language_typescript::SUPPORTED_TYPESCRIPT_CONFIG_SHA256.to_owned(),
            root: EcmaModuleNode { items: Vec::new() },
        }));
        assert!(diagnostics.is_empty());
    }
}
