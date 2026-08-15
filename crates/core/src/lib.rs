pub use efct_model::{Diagnostic, LanguageIdentity, Severity};
use efct_protocol::{ProjectEnvelope, ProtocolEnvelope, SourceLanguage};

pub fn check(envelope: ProtocolEnvelope) -> Vec<Diagnostic> {
    match &envelope.language {
        SourceLanguage::Python { .. } => efct_language_python::check(envelope),
        SourceLanguage::TypeScript { .. } => vec![Diagnostic::error(
            "P0002",
            envelope.filename,
            None,
            None,
            "The current build does not include the TypeScript analyzer",
        )],
    }
}

pub fn check_project(envelope: ProjectEnvelope) -> Vec<Diagnostic> {
    match &envelope.language {
        LanguageIdentity::Python { .. } => efct_language_python::check_project(envelope),
        LanguageIdentity::TypeScript { .. } => vec![Diagnostic::error(
            "P0002",
            envelope.root,
            None,
            None,
            "The current build does not include the TypeScript analyzer",
        )],
    }
}

#[cfg(test)]
mod tests {
    use efct_model::PythonImplementation;
    use efct_protocol::{ModuleKind, ModuleNode};

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
    fn rejects_typescript_explicitly_when_disabled() {
        let diagnostics = check(envelope(SourceLanguage::TypeScript {
            compiler_version: "6.0.0".to_owned(),
        }));
        assert_eq!(diagnostics[0].code, "P0002");
    }
}
