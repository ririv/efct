mod analysis;

use std::collections::{HashMap, HashSet};

use efct_model::{Diagnostic, LanguageIdentity, NodeRuntimeIdentity, TypeScriptCompilerIdentity};
use efct_protocol::{EcmaModuleItem, ProjectEnvelope, ProtocolEnvelope, SourceLanguage};

pub const SUPPORTED_TYPESCRIPT_VERSION: &str = "5.9.3";
pub const SUPPORTED_TYPESCRIPT_INSTALLATION_SHA256: &str =
    "dbbb9b146d378d9d62aa73396b76d5ea0c9eba8945f3a9229aad56862fc1ebd0";
pub const SUPPORTED_TYPESCRIPT_CONFIG_SHA256: &str =
    "77d10f1faef9a270bb496dfc6011e2073b8655cba4c6f4baa477fa7f79928ebf";
pub const SUPPORTED_JAVASCRIPT_CONFIG_SHA256: &str =
    "bb7339c54be75aacd81a01587860c53c32e01656f9b3c70dcf535b4f264b29c6";
pub const SUPPORTED_NODE_RUNTIME: NodeRuntimeIdentity = NodeRuntimeIdentity {
    version: [24, 19, 0],
    node_api_version: 8,
};

pub fn check(envelope: ProtocolEnvelope) -> Vec<Diagnostic> {
    let filename = envelope.filename.clone();
    match envelope.language {
        SourceLanguage::TypeScript {
            compiler,
            runtime,
            config_sha256,
            root,
        } => check_ecma_module(
            filename,
            "TypeScript",
            compiler,
            runtime,
            config_sha256,
            SUPPORTED_TYPESCRIPT_CONFIG_SHA256,
            root.items,
        ),
        SourceLanguage::JavaScript {
            checker,
            runtime,
            config_sha256,
            root,
        } => check_ecma_module(
            filename,
            "JavaScript",
            checker,
            runtime,
            config_sha256,
            SUPPORTED_JAVASCRIPT_CONFIG_SHA256,
            root.items,
        ),
        SourceLanguage::Python { .. } => vec![Diagnostic::error(
            "P0002",
            filename,
            None,
            None,
            "Expected a TypeScript or JavaScript source envelope",
        )],
    }
}

pub fn check_project(project: ProjectEnvelope) -> Vec<Diagnostic> {
    let mut diagnostics = validate_project_identity(&project.language, &project.root);
    let modules: HashMap<_, _> = project
        .modules
        .iter()
        .map(|module| (module.name.clone(), module))
        .collect();
    if modules.len() != project.modules.len() {
        diagnostics.push(project_error(
            &project.root,
            "Duplicate project module identity",
        ));
    }
    if !modules.contains_key(&project.root) {
        diagnostics.push(project_error(
            &project.root,
            "Project root is not present in modules",
        ));
    }
    let mut graph = HashMap::<String, Vec<String>>::new();
    for module in &project.modules {
        if module.name != module.envelope.filename {
            diagnostics.push(project_error(
                &module.envelope.filename,
                "Project module name must equal its canonical source filename",
            ));
        }
        let (_, items) = ecma_items(&module.envelope);
        let dependencies = items
            .iter()
            .filter_map(|item| match item {
                EcmaModuleItem::Import {
                    resolved: Some(resolved),
                    ..
                } => Some(resolved.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        for dependency in &dependencies {
            if !modules.contains_key(dependency) {
                diagnostics.push(project_error(
                    &module.name,
                    format!("Resolved local dependency is absent from project: {dependency}"),
                ));
            }
        }
        graph.insert(module.name.clone(), dependencies);
    }
    if let Some(module) = dependency_cycle(&graph) {
        diagnostics.push(project_error(
            &module,
            "Cyclic local ESM dependencies are not supported in Efct 0.1",
        ));
    }
    for module in &project.modules {
        let (language, items) = ecma_items(&module.envelope);
        let mut imports = HashMap::new();
        for item in &items {
            let EcmaModuleItem::Import {
                resolved: Some(resolved),
                names,
                ..
            } = item
            else {
                continue;
            };
            let Some(target) = modules.get(resolved) else {
                continue;
            };
            let (_, target_items) = ecma_items(&target.envelope);
            for name in names.iter().filter(|name| !name.type_only) {
                let function = target_items.iter().find_map(|item| match item {
                    EcmaModuleItem::ModuleDefinition {
                        exports, functions, ..
                    } if exports.contains(&name.imported) => functions
                        .iter()
                        .find(|function| function.name == name.imported),
                    _ => None,
                });
                let Some(function) = function else {
                    diagnostics.push(project_error(
                        &module.name,
                        format!(
                            "Local import {} is not an Efct function exported by {resolved}",
                            name.imported
                        ),
                    ));
                    continue;
                };
                match analysis::explicit_external_function(function) {
                    Ok(function) => {
                        if imports.insert(name.local.clone(), function).is_some() {
                            diagnostics.push(project_error(
                                &module.name,
                                format!("Duplicate local import binding: {}", name.local),
                            ));
                        }
                    }
                    Err(message) => diagnostics.push(project_error(
                        &module.name,
                        format!("Local import {} is not callable: {message}", name.imported),
                    )),
                }
            }
        }
        diagnostics.extend(validate_module_environment(&module.envelope));
        diagnostics.extend(analysis::check_module_with_imports(
            module.envelope.filename.clone(),
            language,
            items,
            imports,
        ));
    }
    diagnostics
}

fn ecma_items(envelope: &ProtocolEnvelope) -> (&'static str, Vec<EcmaModuleItem>) {
    match &envelope.language {
        SourceLanguage::TypeScript { root, .. } => ("TypeScript", root.items.clone()),
        SourceLanguage::JavaScript { root, .. } => ("JavaScript", root.items.clone()),
        SourceLanguage::Python { .. } => ("Python", Vec::new()),
    }
}

fn validate_module_environment(envelope: &ProtocolEnvelope) -> Vec<Diagnostic> {
    match &envelope.language {
        SourceLanguage::TypeScript { config_sha256, .. } => validate_config(
            config_sha256,
            SUPPORTED_TYPESCRIPT_CONFIG_SHA256,
            &envelope.filename,
        ),
        SourceLanguage::JavaScript { config_sha256, .. } => validate_config(
            config_sha256,
            SUPPORTED_JAVASCRIPT_CONFIG_SHA256,
            &envelope.filename,
        ),
        SourceLanguage::Python { .. } => vec![project_error(
            &envelope.filename,
            "Expected a TypeScript or JavaScript source envelope",
        )],
    }
}

fn validate_config(actual: &str, expected: &str, filename: &str) -> Vec<Diagnostic> {
    if actual == expected {
        Vec::new()
    } else {
        vec![Diagnostic::error(
            "P0002",
            filename.to_owned(),
            None,
            None,
            format!("Unsupported effective compiler configuration {actual}; expected {expected}"),
        )]
    }
}

fn dependency_cycle(graph: &HashMap<String, Vec<String>>) -> Option<String> {
    fn visit(
        module: &str,
        graph: &HashMap<String, Vec<String>>,
        visiting: &mut HashSet<String>,
        visited: &mut HashSet<String>,
    ) -> Option<String> {
        if visiting.contains(module) {
            return Some(module.to_owned());
        }
        if !visited.insert(module.to_owned()) {
            return None;
        }
        visiting.insert(module.to_owned());
        for dependency in graph.get(module).into_iter().flatten() {
            if let Some(cycle) = visit(dependency, graph, visiting, visited) {
                return Some(cycle);
            }
        }
        visiting.remove(module);
        None
    }
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    for module in graph.keys() {
        if let Some(cycle) = visit(module, graph, &mut visiting, &mut visited) {
            return Some(cycle);
        }
    }
    None
}

fn project_error(filename: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error("J0003", filename.to_owned(), None, None, message)
}

fn check_ecma_module(
    filename: String,
    language: &str,
    compiler: TypeScriptCompilerIdentity,
    runtime: NodeRuntimeIdentity,
    config_sha256: String,
    expected_config_sha256: &str,
    items: Vec<EcmaModuleItem>,
) -> Vec<Diagnostic> {
    let mut diagnostics = validate_environment(&compiler, runtime, &filename);
    diagnostics.extend(validate_config(
        &config_sha256,
        expected_config_sha256,
        &filename,
    ));
    diagnostics.extend(analysis::check_module(filename, language, items));
    diagnostics
}

fn validate_project_identity(language: &LanguageIdentity, filename: &str) -> Vec<Diagnostic> {
    match language {
        LanguageIdentity::TypeScript { compiler, runtime } => {
            validate_environment(compiler, *runtime, filename)
        }
        LanguageIdentity::JavaScript { checker, runtime } => {
            validate_environment(checker, *runtime, filename)
        }
        LanguageIdentity::Python { .. } => vec![Diagnostic::error(
            "P0002",
            filename.to_owned(),
            None,
            None,
            "Expected a TypeScript or JavaScript project envelope",
        )],
    }
}

fn validate_environment(
    compiler: &TypeScriptCompilerIdentity,
    runtime: NodeRuntimeIdentity,
    filename: &str,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    if compiler.version != SUPPORTED_TYPESCRIPT_VERSION {
        diagnostics.push(Diagnostic::error(
            "P0002",
            filename.to_owned(),
            None,
            None,
            format!(
                "Unsupported TypeScript compiler version {}; expected {SUPPORTED_TYPESCRIPT_VERSION}",
                compiler.version
            ),
        ));
    }
    if compiler.installation_sha256 != SUPPORTED_TYPESCRIPT_INSTALLATION_SHA256 {
        diagnostics.push(Diagnostic::error(
            "P0002",
            filename.to_owned(),
            None,
            None,
            format!(
                "Unsupported TypeScript installation digest {}; expected {SUPPORTED_TYPESCRIPT_INSTALLATION_SHA256}",
                compiler.installation_sha256
            ),
        ));
    }
    if runtime != SUPPORTED_NODE_RUNTIME {
        diagnostics.push(Diagnostic::error(
            "P0002",
            filename.to_owned(),
            None,
            None,
            format!(
                "Unsupported Node.js runtime identity {:?} with Node-API {}; expected {:?} with Node-API {}",
                runtime.version,
                runtime.node_api_version,
                SUPPORTED_NODE_RUNTIME.version,
                SUPPORTED_NODE_RUNTIME.node_api_version
            ),
        ));
    }
    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use efct_model::TrustPolicy;
    use efct_protocol::{
        EcmaBinaryOperator, EcmaEffectContract, EcmaErrorConstructor, EcmaExpressionNode,
        EcmaExternalEffect, EcmaFunctionContract, EcmaFunctionNode, EcmaImportName, EcmaModuleNode,
        EcmaOptionalAbsence, EcmaParameterNode, EcmaPartialBehavior, EcmaPartialContract,
        EcmaStatementNode, EcmaTypeNode, Utf16SourceSpan,
    };

    const HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn compiler() -> TypeScriptCompilerIdentity {
        TypeScriptCompilerIdentity {
            version: SUPPORTED_TYPESCRIPT_VERSION.to_owned(),
            installation_sha256: SUPPORTED_TYPESCRIPT_INSTALLATION_SHA256.to_owned(),
        }
    }

    fn envelope(language: SourceLanguage, filename: &str) -> ProtocolEnvelope {
        ProtocolEnvelope {
            protocol_version: efct_protocol::PROTOCOL_VERSION,
            filename: filename.to_owned(),
            source_sha256: HASH.to_owned(),
            language,
        }
    }

    fn span() -> Utf16SourceSpan {
        Utf16SourceSpan { start: 0, end: 1 }
    }

    fn pure_add(return_type: EcmaTypeNode) -> EcmaFunctionNode {
        EcmaFunctionNode {
            name: "add".to_owned(),
            contract: EcmaFunctionContract::Pure {
                partial: EcmaPartialContract::ExplicitEmpty,
            },
            parameters: vec![
                EcmaParameterNode {
                    name: "left".to_owned(),
                    annotation: EcmaTypeNode::Number,
                    span: span(),
                },
                EcmaParameterNode {
                    name: "right".to_owned(),
                    annotation: EcmaTypeNode::Number,
                    span: span(),
                },
            ],
            returns: return_type,
            body: vec![EcmaStatementNode::Return {
                value: Some(EcmaExpressionNode::Binary {
                    left: Box::new(EcmaExpressionNode::Identifier {
                        name: "left".to_owned(),
                        span: span(),
                    }),
                    operator: EcmaBinaryOperator::Add,
                    right: Box::new(EcmaExpressionNode::Identifier {
                        name: "right".to_owned(),
                        span: span(),
                    }),
                    span: span(),
                }),
                span: span(),
            }],
            span: span(),
        }
    }

    fn pure_module(function: EcmaFunctionNode) -> SourceLanguage {
        SourceLanguage::TypeScript {
            compiler: compiler(),
            runtime: SUPPORTED_NODE_RUNTIME,
            config_sha256: SUPPORTED_TYPESCRIPT_CONFIG_SHA256.to_owned(),
            root: EcmaModuleNode {
                items: vec![
                    EcmaModuleItem::Import {
                        module: "efct".to_owned(),
                        resolved: None,
                        names: vec![
                            EcmaImportName {
                                imported: "defineModule".to_owned(),
                                local: "defineModule".to_owned(),
                                type_only: false,
                            },
                            EcmaImportName {
                                imported: "pure".to_owned(),
                                local: "pure".to_owned(),
                                type_only: false,
                            },
                        ],
                        span: span(),
                    },
                    EcmaModuleItem::ModuleDefinition {
                        exports: vec!["add".to_owned()],
                        functions: vec![function],
                        span: span(),
                    },
                ],
            },
        }
    }

    fn add_declaration_import(language: &mut SourceLanguage, imported: &str) {
        let SourceLanguage::TypeScript { root, .. } = language else {
            unreachable!();
        };
        let EcmaModuleItem::Import { names, .. } = &mut root.items[0] else {
            unreachable!();
        };
        names.push(EcmaImportName {
            imported: imported.to_owned(),
            local: imported.to_owned(),
            type_only: false,
        });
    }

    #[test]
    fn accepts_empty_typescript_and_checked_javascript_modules() {
        let root = EcmaModuleNode { items: Vec::new() };
        let typescript = SourceLanguage::TypeScript {
            compiler: compiler(),
            runtime: SUPPORTED_NODE_RUNTIME,
            config_sha256: SUPPORTED_TYPESCRIPT_CONFIG_SHA256.to_owned(),
            root: root.clone(),
        };
        let javascript = SourceLanguage::JavaScript {
            checker: compiler(),
            runtime: SUPPORTED_NODE_RUNTIME,
            config_sha256: SUPPORTED_JAVASCRIPT_CONFIG_SHA256.to_owned(),
            root,
        };

        assert!(check(envelope(typescript, "empty.ts")).is_empty());
        assert!(check(envelope(javascript, "empty.js")).is_empty());
    }

    #[test]
    fn rejects_unknown_syntax_instead_of_ignoring_it() {
        let language = SourceLanguage::TypeScript {
            compiler: compiler(),
            runtime: SUPPORTED_NODE_RUNTIME,
            config_sha256: SUPPORTED_TYPESCRIPT_CONFIG_SHA256.to_owned(),
            root: EcmaModuleNode {
                items: vec![EcmaModuleItem::Unsupported {
                    node: "ClassDeclaration".to_owned(),
                    span: Utf16SourceSpan { start: 0, end: 5 },
                }],
            },
        };

        let diagnostics = check(envelope(language, "unsupported.ts"));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "J0001");
    }

    #[test]
    fn accepts_a_typed_pure_arithmetic_function() {
        let diagnostics = check(envelope(
            pure_module(pure_add(EcmaTypeNode::Number)),
            "add.ts",
        ));

        assert!(diagnostics.is_empty());
    }

    #[test]
    fn rejects_a_pure_function_with_an_incorrect_return_contract() {
        let diagnostics = check(envelope(
            pure_module(pure_add(EcmaTypeNode::String)),
            "add.ts",
        ));

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "J0002");
        assert!(diagnostics[0].message.contains("returns number"));
    }

    #[test]
    fn rejects_module_exports_that_do_not_match_the_declarations() {
        let mut language = pure_module(pure_add(EcmaTypeNode::Number));
        let SourceLanguage::TypeScript { root, .. } = &mut language else {
            unreachable!();
        };
        let EcmaModuleItem::ModuleDefinition { exports, .. } = &mut root.items[1] else {
            unreachable!();
        };
        *exports = vec!["subtract".to_owned()];

        let diagnostics = check(envelope(language, "add.ts"));

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "J0003");
    }

    #[test]
    fn rejects_an_unknown_binding_in_a_pure_expression() {
        let mut function = pure_add(EcmaTypeNode::Number);
        let EcmaStatementNode::Return { value, .. } = &mut function.body[0] else {
            unreachable!();
        };
        *value = Some(EcmaExpressionNode::Identifier {
            name: "ambient".to_owned(),
            span: span(),
        });

        let diagnostics = check(envelope(pure_module(function), "add.ts"));

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "J0002");
        assert!(
            diagnostics[0]
                .message
                .contains("Unknown pure function binding")
        );
    }

    #[test]
    fn rejects_throw_outside_an_explicit_empty_partial_whitelist() {
        let mut function = pure_add(EcmaTypeNode::Number);
        function.body = vec![EcmaStatementNode::Throw {
            value: EcmaExpressionNode::String {
                value: "failure".to_owned(),
                span: span(),
            },
            span: span(),
        }];

        let diagnostics = check(envelope(pure_module(function), "failure.ts"));

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "J0004");
        assert!(diagnostics[0].message.contains("Throw"));
    }

    #[test]
    fn accepts_throw_inside_its_explicit_partial_whitelist() {
        let mut function = pure_add(EcmaTypeNode::Number);
        function.contract = EcmaFunctionContract::Pure {
            partial: EcmaPartialContract::Explicit {
                behaviors: vec![EcmaPartialBehavior::Throw],
            },
        };
        function.body = vec![EcmaStatementNode::Throw {
            value: EcmaExpressionNode::String {
                value: "failure".to_owned(),
                span: span(),
            },
            span: span(),
        }];

        assert!(check(envelope(pure_module(function), "failure.ts")).is_empty());
    }

    #[test]
    fn rejects_a_potentially_infinite_loop_without_diverge_permission() {
        let mut function = pure_add(EcmaTypeNode::Void);
        function.parameters.clear();
        function.body = vec![EcmaStatementNode::While {
            condition: EcmaExpressionNode::Boolean {
                value: true,
                span: span(),
            },
            body: Vec::new(),
            span: span(),
        }];

        let diagnostics = check(envelope(pure_module(function), "spin.ts"));

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "J0004");
        assert!(diagnostics[0].message.contains("Diverge"));
    }

    #[test]
    fn accepts_a_potentially_infinite_loop_with_diverge_permission() {
        let mut function = pure_add(EcmaTypeNode::Void);
        function.parameters.clear();
        function.contract = EcmaFunctionContract::Pure {
            partial: EcmaPartialContract::Explicit {
                behaviors: vec![EcmaPartialBehavior::Diverge],
            },
        };
        function.body = vec![EcmaStatementNode::While {
            condition: EcmaExpressionNode::Boolean {
                value: true,
                span: span(),
            },
            body: Vec::new(),
            span: span(),
        }];

        assert!(check(envelope(pure_module(function), "spin.ts")).is_empty());
    }

    #[test]
    fn rejects_a_clock_call_from_a_pure_function() {
        let mut function = pure_add(EcmaTypeNode::Number);
        function.parameters.clear();
        function.body = vec![EcmaStatementNode::Return {
            value: Some(EcmaExpressionNode::Call {
                target: vec!["Date".to_owned(), "now".to_owned()],
                arguments: Vec::new(),
                span: span(),
            }),
            span: span(),
        }];

        let diagnostics = check(envelope(pure_module(function), "clock.ts"));

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "J0005");
        assert!(diagnostics[0].message.contains("Clock"));
    }

    #[test]
    fn accepts_a_clock_call_with_an_explicit_effect_whitelist() {
        let mut function = pure_add(EcmaTypeNode::Number);
        function.parameters.clear();
        function.contract = EcmaFunctionContract::Effects {
            effects: EcmaEffectContract::Explicit {
                effects: vec![EcmaExternalEffect::Clock],
            },
            partial: EcmaPartialContract::ExplicitEmpty,
        };
        function.body = vec![EcmaStatementNode::Return {
            value: Some(EcmaExpressionNode::Call {
                target: vec!["Date".to_owned(), "now".to_owned()],
                arguments: Vec::new(),
                span: span(),
            }),
            span: span(),
        }];
        let mut language = pure_module(function);
        add_declaration_import(&mut language, "effects");

        assert!(check(envelope(language, "clock.ts")).is_empty());
    }

    #[test]
    fn console_calls_require_both_console_and_throw_permissions() {
        let mut function = pure_add(EcmaTypeNode::Void);
        function.parameters.truncate(1);
        function.contract = EcmaFunctionContract::Effects {
            effects: EcmaEffectContract::Explicit {
                effects: vec![EcmaExternalEffect::Console],
            },
            partial: EcmaPartialContract::ExplicitEmpty,
        };
        function.body = vec![EcmaStatementNode::Expression {
            expression: EcmaExpressionNode::Call {
                target: vec!["console".to_owned(), "log".to_owned()],
                arguments: vec![EcmaExpressionNode::Identifier {
                    name: "left".to_owned(),
                    span: span(),
                }],
                span: span(),
            },
            span: span(),
        }];
        let mut missing_throw = pure_module(function.clone());
        add_declaration_import(&mut missing_throw, "effects");

        let diagnostics = check(envelope(missing_throw, "console.ts"));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "J0004");

        function.contract = EcmaFunctionContract::Effects {
            effects: EcmaEffectContract::Explicit {
                effects: vec![EcmaExternalEffect::Console],
            },
            partial: EcmaPartialContract::Explicit {
                behaviors: vec![EcmaPartialBehavior::Throw],
            },
        };
        let mut complete = pure_module(function);
        add_declaration_import(&mut complete, "effects");

        assert!(check(envelope(complete, "console.ts")).is_empty());
    }

    #[test]
    fn accepts_a_nullable_primitive_without_unifying_it_with_undefined() {
        let optional = EcmaTypeNode::Optional {
            value: Box::new(EcmaTypeNode::Number),
            absence: EcmaOptionalAbsence::Null,
        };
        let mut function = pure_add(optional.clone());
        function.parameters = vec![EcmaParameterNode {
            name: "value".to_owned(),
            annotation: optional,
            span: span(),
        }];
        function.body = vec![EcmaStatementNode::Return {
            value: Some(EcmaExpressionNode::Identifier {
                name: "value".to_owned(),
                span: span(),
            }),
            span: span(),
        }];

        assert!(check(envelope(pure_module(function), "optional.ts")).is_empty());
    }

    #[test]
    fn narrows_nullable_parameters_across_explicit_if_branches() {
        let optional = EcmaTypeNode::Optional {
            value: Box::new(EcmaTypeNode::Number),
            absence: EcmaOptionalAbsence::Null,
        };
        let mut function = pure_add(EcmaTypeNode::Number);
        function.parameters = vec![EcmaParameterNode {
            name: "value".to_owned(),
            annotation: optional,
            span: span(),
        }];
        function.body = vec![EcmaStatementNode::If {
            condition: EcmaExpressionNode::Binary {
                left: Box::new(EcmaExpressionNode::Identifier {
                    name: "value".to_owned(),
                    span: span(),
                }),
                operator: EcmaBinaryOperator::StrictEqual,
                right: Box::new(EcmaExpressionNode::Null { span: span() }),
                span: span(),
            },
            then_body: vec![EcmaStatementNode::Return {
                value: Some(EcmaExpressionNode::Number {
                    text: "0".to_owned(),
                    span: span(),
                }),
                span: span(),
            }],
            else_body: vec![EcmaStatementNode::Return {
                value: Some(EcmaExpressionNode::Identifier {
                    name: "value".to_owned(),
                    span: span(),
                }),
                span: span(),
            }],
            span: span(),
        }];

        assert!(check(envelope(pure_module(function), "narrow.ts")).is_empty());
    }

    #[test]
    fn accepts_a_pure_function_that_reads_a_static_primitive_constant() {
        let mut function = pure_add(EcmaTypeNode::Number);
        let EcmaStatementNode::Return { value, .. } = &mut function.body[0] else {
            unreachable!();
        };
        let Some(EcmaExpressionNode::Binary { right, .. }) = value else {
            unreachable!();
        };
        **right = EcmaExpressionNode::Identifier {
            name: "INCREMENT".to_owned(),
            span: span(),
        };
        let mut language = pure_module(function);
        let SourceLanguage::TypeScript { root, .. } = &mut language else {
            unreachable!();
        };
        root.items.insert(
            1,
            EcmaModuleItem::Constant {
                name: "INCREMENT".to_owned(),
                annotation: Some(EcmaTypeNode::Number),
                value: EcmaExpressionNode::Number {
                    text: "1".to_owned(),
                    span: span(),
                },
                span: span(),
            },
        );

        assert!(check(envelope(language, "constant.ts")).is_empty());
    }

    #[test]
    fn propagates_effects_through_same_module_calls() {
        let clock_contract = EcmaFunctionContract::Effects {
            effects: EcmaEffectContract::Explicit {
                effects: vec![EcmaExternalEffect::Clock],
            },
            partial: EcmaPartialContract::ExplicitEmpty,
        };
        let clock = EcmaFunctionNode {
            name: "clock".to_owned(),
            contract: clock_contract.clone(),
            parameters: Vec::new(),
            returns: EcmaTypeNode::Number,
            body: vec![EcmaStatementNode::Return {
                value: Some(EcmaExpressionNode::Call {
                    target: vec!["Date".to_owned(), "now".to_owned()],
                    arguments: Vec::new(),
                    span: span(),
                }),
                span: span(),
            }],
            span: span(),
        };
        let caller = EcmaFunctionNode {
            name: "caller".to_owned(),
            contract: EcmaFunctionContract::Pure {
                partial: EcmaPartialContract::ExplicitEmpty,
            },
            parameters: Vec::new(),
            returns: EcmaTypeNode::Number,
            body: vec![EcmaStatementNode::Return {
                value: Some(EcmaExpressionNode::Call {
                    target: vec!["clock".to_owned()],
                    arguments: Vec::new(),
                    span: span(),
                }),
                span: span(),
            }],
            span: span(),
        };
        let mut rejected = pure_module(clock.clone());
        add_declaration_import(&mut rejected, "effects");
        let SourceLanguage::TypeScript { root, .. } = &mut rejected else {
            unreachable!();
        };
        let EcmaModuleItem::ModuleDefinition {
            exports, functions, ..
        } = &mut root.items[1]
        else {
            unreachable!();
        };
        *exports = vec!["clock".to_owned(), "caller".to_owned()];
        *functions = vec![clock.clone(), caller.clone()];

        let diagnostics = check(envelope(rejected, "calls.ts"));
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "J0005")
        );

        let mut accepted_caller = caller;
        accepted_caller.contract = clock_contract;
        let mut accepted = pure_module(clock.clone());
        add_declaration_import(&mut accepted, "effects");
        let SourceLanguage::TypeScript { root, .. } = &mut accepted else {
            unreachable!();
        };
        let EcmaModuleItem::ModuleDefinition {
            exports, functions, ..
        } = &mut root.items[1]
        else {
            unreachable!();
        };
        *exports = vec!["clock".to_owned(), "caller".to_owned()];
        *functions = vec![clock, accepted_caller];

        assert!(check(envelope(accepted, "calls.ts")).is_empty());
    }

    #[test]
    fn recursive_calls_require_diverge_permission() {
        let function = EcmaFunctionNode {
            name: "recurse".to_owned(),
            contract: EcmaFunctionContract::Pure {
                partial: EcmaPartialContract::ExplicitEmpty,
            },
            parameters: vec![EcmaParameterNode {
                name: "value".to_owned(),
                annotation: EcmaTypeNode::Number,
                span: span(),
            }],
            returns: EcmaTypeNode::Number,
            body: vec![EcmaStatementNode::Return {
                value: Some(EcmaExpressionNode::Call {
                    target: vec!["recurse".to_owned()],
                    arguments: vec![EcmaExpressionNode::Identifier {
                        name: "value".to_owned(),
                        span: span(),
                    }],
                    span: span(),
                }),
                span: span(),
            }],
            span: span(),
        };
        let mut language = pure_module(function);
        let SourceLanguage::TypeScript { root, .. } = &mut language else {
            unreachable!();
        };
        let EcmaModuleItem::ModuleDefinition { exports, .. } = &mut root.items[1] else {
            unreachable!();
        };
        *exports = vec!["recurse".to_owned()];

        let diagnostics = check(envelope(language, "recursive.ts"));

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "J0004");
        assert!(diagnostics[0].message.contains("Diverge"));
    }

    #[test]
    fn an_unbound_catch_eliminates_throw_from_the_partial_row() {
        let function = EcmaFunctionNode {
            name: "recover".to_owned(),
            contract: EcmaFunctionContract::Pure {
                partial: EcmaPartialContract::ExplicitEmpty,
            },
            parameters: Vec::new(),
            returns: EcmaTypeNode::String,
            body: vec![EcmaStatementNode::Try {
                body: vec![EcmaStatementNode::Throw {
                    value: EcmaExpressionNode::Error {
                        constructor: EcmaErrorConstructor::Error,
                        message: Some(Box::new(EcmaExpressionNode::String {
                            value: "failure".to_owned(),
                            span: span(),
                        })),
                        span: span(),
                    },
                    span: span(),
                }],
                catch_body: Some(vec![EcmaStatementNode::Return {
                    value: Some(EcmaExpressionNode::String {
                        value: "fallback".to_owned(),
                        span: span(),
                    }),
                    span: span(),
                }]),
                finally_body: None,
                span: span(),
            }],
            span: span(),
        };
        let mut language = pure_module(function);
        let SourceLanguage::TypeScript { root, .. } = &mut language else {
            unreachable!();
        };
        let EcmaModuleItem::ModuleDefinition { exports, .. } = &mut root.items[1] else {
            unreachable!();
        };
        *exports = vec!["recover".to_owned()];

        assert!(check(envelope(language, "catch.ts")).is_empty());
    }

    #[test]
    fn a_terminating_finally_overrides_a_protected_throw() {
        let function = EcmaFunctionNode {
            name: "recover".to_owned(),
            contract: EcmaFunctionContract::Pure {
                partial: EcmaPartialContract::ExplicitEmpty,
            },
            parameters: Vec::new(),
            returns: EcmaTypeNode::Number,
            body: vec![EcmaStatementNode::Try {
                body: vec![EcmaStatementNode::Throw {
                    value: EcmaExpressionNode::String {
                        value: "failure".to_owned(),
                        span: span(),
                    },
                    span: span(),
                }],
                catch_body: None,
                finally_body: Some(vec![EcmaStatementNode::Return {
                    value: Some(EcmaExpressionNode::Number {
                        text: "1".to_owned(),
                        span: span(),
                    }),
                    span: span(),
                }]),
                span: span(),
            }],
            span: span(),
        };
        let mut language = pure_module(function);
        let SourceLanguage::TypeScript { root, .. } = &mut language else {
            unreachable!();
        };
        let EcmaModuleItem::ModuleDefinition { exports, .. } = &mut root.items[1] else {
            unreachable!();
        };
        *exports = vec!["recover".to_owned()];

        assert!(check(envelope(language, "finally.ts")).is_empty());
    }

    #[test]
    fn validates_node_file_import_identity_signature_and_effects() {
        let function = EcmaFunctionNode {
            name: "add".to_owned(),
            contract: EcmaFunctionContract::Effects {
                effects: EcmaEffectContract::Explicit {
                    effects: vec![EcmaExternalEffect::FileRead],
                },
                partial: EcmaPartialContract::Explicit {
                    behaviors: vec![EcmaPartialBehavior::Throw],
                },
            },
            parameters: vec![EcmaParameterNode {
                name: "path".to_owned(),
                annotation: EcmaTypeNode::String,
                span: span(),
            }],
            returns: EcmaTypeNode::String,
            body: vec![EcmaStatementNode::Return {
                value: Some(EcmaExpressionNode::Call {
                    target: vec!["readFileSync".to_owned()],
                    arguments: vec![
                        EcmaExpressionNode::Identifier {
                            name: "path".to_owned(),
                            span: span(),
                        },
                        EcmaExpressionNode::String {
                            value: "utf8".to_owned(),
                            span: span(),
                        },
                    ],
                    span: span(),
                }),
                span: span(),
            }],
            span: span(),
        };
        let mut language = pure_module(function);
        add_declaration_import(&mut language, "effects");
        let SourceLanguage::TypeScript { root, .. } = &mut language else {
            unreachable!();
        };
        root.items.insert(
            0,
            EcmaModuleItem::Import {
                module: "node:fs".to_owned(),
                resolved: None,
                names: vec![EcmaImportName {
                    imported: "readFileSync".to_owned(),
                    local: "readFileSync".to_owned(),
                    type_only: false,
                }],
                span: span(),
            },
        );

        assert!(check(envelope(language, "file.ts")).is_empty());
    }

    #[test]
    fn environment_properties_cannot_be_hidden_in_a_pure_function() {
        let mut function = pure_add(EcmaTypeNode::Optional {
            value: Box::new(EcmaTypeNode::String),
            absence: EcmaOptionalAbsence::Undefined,
        });
        function.parameters.clear();
        function.body = vec![EcmaStatementNode::Return {
            value: Some(EcmaExpressionNode::Property {
                target: vec!["process".to_owned(), "env".to_owned(), "HOME".to_owned()],
                span: span(),
            }),
            span: span(),
        }];

        let diagnostics = check(envelope(pure_module(function), "environment.ts"));

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "J0005");
        assert!(diagnostics[0].message.contains("Environment"));
    }

    #[test]
    fn rejects_unpinned_compiler_and_runtime_identities() {
        let language = SourceLanguage::JavaScript {
            checker: TypeScriptCompilerIdentity {
                version: "5.9.2".to_owned(),
                installation_sha256: HASH.to_owned(),
            },
            runtime: NodeRuntimeIdentity {
                version: [24, 18, 0],
                node_api_version: 8,
            },
            config_sha256: SUPPORTED_JAVASCRIPT_CONFIG_SHA256.to_owned(),
            root: EcmaModuleNode { items: Vec::new() },
        };

        let diagnostics = check(envelope(language, "empty.js"));
        assert_eq!(diagnostics.len(), 3);
        assert!(diagnostics.iter().all(|item| item.code == "P0002"));
    }

    #[test]
    fn rejects_an_unpinned_effective_compiler_configuration() {
        let language = SourceLanguage::TypeScript {
            compiler: compiler(),
            runtime: SUPPORTED_NODE_RUNTIME,
            config_sha256: HASH.to_owned(),
            root: EcmaModuleNode { items: Vec::new() },
        };

        let diagnostics = check(envelope(language, "empty.ts"));
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("configuration"));
    }

    #[test]
    fn checks_all_modules_in_a_javascript_project() {
        let identity = LanguageIdentity::JavaScript {
            checker: compiler(),
            runtime: SUPPORTED_NODE_RUNTIME,
        };
        let module = envelope(
            SourceLanguage::JavaScript {
                checker: compiler(),
                runtime: SUPPORTED_NODE_RUNTIME,
                config_sha256: SUPPORTED_JAVASCRIPT_CONFIG_SHA256.to_owned(),
                root: EcmaModuleNode { items: Vec::new() },
            },
            "empty.js",
        );
        let project = ProjectEnvelope {
            protocol_version: efct_protocol::PROTOCOL_VERSION,
            language: identity,
            root: "empty.js".to_owned(),
            modules: vec![efct_protocol::ProjectModule {
                name: "empty.js".to_owned(),
                envelope: module,
            }],
            policy: TrustPolicy::Default,
            external_symbols: Vec::new(),
        };

        assert!(check_project(project).is_empty());
    }
}
