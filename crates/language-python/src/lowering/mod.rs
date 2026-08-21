use efct_protocol::{
    ClassItemNode, ConstantValue, ExpressionContext, ExpressionNode, ModuleItem, ProtocolEnvelope,
};

use crate::hir::{
    ConstantDefinition, ExceptionDefinition, Expression, Function, FunctionId, FunctionKind,
    Import, MODULE_INITIALIZER_NAME, Module, RecordDefinition, RecordField,
};
use efct_model::Diagnostic;

mod callable;
mod effects;
mod module_contract;
mod statements;

use module_contract::{ClassifiedModuleStatement, module_import_statements};

pub fn lower(envelope: ProtocolEnvelope) -> Result<Module, Vec<Diagnostic>> {
    let efct_protocol::SourceLanguage::Python { root, .. } = envelope.language else {
        return Err(vec![Diagnostic::error(
            "P0002",
            envelope.filename,
            None,
            None,
            "The Python analyzer received a non-Python payload",
        )]);
    };
    if root.items.is_empty() {
        return Ok(Module {
            filename: envelope.filename,
            source_sha256: envelope.source_sha256,
            imports: Vec::new(),
            constants: Vec::new(),
            records: Vec::new(),
            exceptions: Vec::new(),
            functions: Vec::new(),
        });
    }

    let mut context = LoweringContext {
        filename: envelope.filename.clone(),
        diagnostics: Vec::new(),
    };
    let mut module = Module {
        filename: envelope.filename,
        source_sha256: envelope.source_sha256,
        imports: Vec::new(),
        constants: Vec::new(),
        records: Vec::new(),
        exceptions: Vec::new(),
        functions: Vec::new(),
    };

    let mut module_declaration = None;
    let mut module_statements = Vec::new();
    for item in root.items {
        if let ModuleItem::Statement { statement } = item {
            module_statements.push(statement);
            continue;
        }
        context.lower_item(item, &mut module);
    }

    let mut initializer_statements = Vec::new();
    for statement in module_statements {
        match context.classify_module_statement(statement, &module.imports) {
            ClassifiedModuleStatement::Contract { declaration, span } => {
                if module_declaration.replace((declaration, span)).is_some() {
                    context.error(
                        "P1006",
                        Some(span),
                        None,
                        "A module may declare `_efct` only once",
                    );
                }
            }
            ClassifiedModuleStatement::Initializer(statement) => {
                initializer_statements.push(*statement);
            }
            ClassifiedModuleStatement::InvalidContract => {}
        }
    }

    if let Some((declaration, span)) = module_declaration {
        let mut initializer_body = Vec::new();
        for statement in initializer_statements {
            context.lower_module_initializer_statement(statement, &mut initializer_body);
        }
        let mut body = module
            .imports
            .iter()
            .flat_map(module_import_statements)
            .collect::<Vec<_>>();
        body.append(&mut initializer_body);
        module.functions.push(Function {
            id: FunctionId(module.functions.len()),
            kind: FunctionKind::ModuleInitializer,
            name: MODULE_INITIALIZER_NAME.to_owned(),
            declaration,
            effect_parameters: Vec::new(),
            parameters: Vec::new(),
            returns: Some(Expression::Constant {
                value: ConstantValue::None,
                span,
            }),
            body,
            span,
        });
    }

    if context.diagnostics.is_empty() {
        Ok(module)
    } else {
        Err(context.diagnostics)
    }
}

struct LoweringContext {
    filename: String,
    diagnostics: Vec<Diagnostic>,
}

impl LoweringContext {
    fn lower_item(&mut self, item: ModuleItem, module: &mut Module) {
        match item {
            ModuleItem::Import { names, span } => {
                for name in names {
                    let binding = name.alias.unwrap_or_else(|| {
                        name.name.split('.').next().unwrap_or(&name.name).to_owned()
                    });
                    module.imports.push(Import::Module {
                        path: name.name,
                        binding,
                        span,
                    });
                }
            }
            ModuleItem::ImportFrom {
                module: source,
                names,
                level,
                span,
            } => {
                let Some(source) = source else {
                    self.error(
                        "P1005",
                        Some(span),
                        None,
                        "A relative import without a module name is not allowed",
                    );
                    return;
                };
                if level != 0 {
                    self.error(
                        "P1005",
                        Some(span),
                        None,
                        "Relative imports are not supported in the current version",
                    );
                    return;
                }
                for name in names {
                    if name.name == "*" {
                        self.error("P1005", Some(span), None, "Star imports are not allowed");
                    } else {
                        let binding = name.alias.unwrap_or_else(|| name.name.clone());
                        module.imports.push(Import::Symbol {
                            module: source.clone(),
                            name: name.name,
                            binding,
                            span,
                        });
                    }
                }
            }
            ModuleItem::AnnotatedAssignment {
                target,
                annotation,
                value,
                simple,
                span,
            } => {
                if !simple {
                    self.error(
                        "P1401",
                        Some(span),
                        None,
                        "A module constant must be assigned to a simple name",
                    );
                    return;
                }
                let Some(value) = value else {
                    self.error(
                        "P1501",
                        Some(span),
                        None,
                        "A module constant must have an initializer",
                    );
                    return;
                };
                let name = match &target {
                    ExpressionNode::Name { identifier, .. } => identifier.clone(),
                    _ => {
                        self.error(
                            "P1401",
                            Some(span),
                            None,
                            "A module constant target must be a name",
                        );
                        return;
                    }
                };
                let Some(annotation) = self.lower_expression(annotation, None) else {
                    return;
                };
                let Some(value) = self.lower_expression(value, None) else {
                    return;
                };
                module.constants.push(ConstantDefinition {
                    name,
                    annotation,
                    value,
                    span,
                });
            }
            ModuleItem::Function {
                name,
                type_parameters,
                parameters,
                returns,
                decorators,
                body,
                type_comment,
                span,
            } => {
                let function_name = Some(name.as_str());
                if type_comment.is_some() {
                    self.error(
                        "P1502",
                        Some(span),
                        function_name,
                        "Function type comments are not allowed",
                    );
                    return;
                }
                let Some(declaration) =
                    self.lower_declaration(decorators, &name, span, &module.imports)
                else {
                    return;
                };
                let Some(effect_parameters) =
                    self.lower_effect_parameters(type_parameters, &name, &module.imports)
                else {
                    return;
                };
                let Some(parameters) = self.lower_parameters(*parameters, &name, span) else {
                    return;
                };
                let returns = match returns {
                    Some(expression) => self.lower_expression(expression, function_name),
                    None => None,
                };
                let Some(body) = self.lower_statements(body, &name) else {
                    return;
                };
                let id = FunctionId(module.functions.len());
                module.functions.push(Function {
                    id,
                    kind: FunctionKind::Declared,
                    name,
                    declaration,
                    effect_parameters,
                    parameters,
                    returns,
                    body,
                    span,
                });
            }
            ModuleItem::Class {
                name,
                bases,
                keywords,
                decorators,
                body,
                span,
            } => {
                if !bases.is_empty() {
                    if bases.len() != 1 || !keywords.is_empty() || !decorators.is_empty() {
                        self.error(
                            "P1201",
                            Some(span),
                            None,
                            "An exception class requires one base class and no decorators or metaclass arguments",
                        );
                        return;
                    }
                    let mut valid_body = true;
                    for (index, item) in body.iter().enumerate() {
                        match item {
                            ClassItemNode::Pass { .. } => {}
                            ClassItemNode::Docstring { .. } if index == 0 => {}
                            ClassItemNode::Docstring { span } => {
                                self.error(
                                    "P1201",
                                    Some(*span),
                                    None,
                                    "An exception class docstring must be its first body item",
                                );
                                valid_body = false;
                            }
                            ClassItemNode::Field { span, .. }
                            | ClassItemNode::Unsupported { span, .. } => {
                                self.error(
                                    "P1201",
                                    Some(*span),
                                    None,
                                    "An exception class body may only contain a docstring and pass",
                                );
                                valid_body = false;
                            }
                        }
                    }
                    let Some(base) = self.lower_expression(bases.into_iter().next().unwrap(), None)
                    else {
                        return;
                    };
                    if valid_body {
                        module
                            .exceptions
                            .push(ExceptionDefinition { name, base, span });
                    }
                    return;
                }
                if !keywords.is_empty() {
                    self.error(
                        "P1201",
                        Some(span),
                        None,
                        "Pure records cannot have base classes or metaclasses",
                    );
                    return;
                }
                if !is_pure_record_decorators(&decorators, &module.imports) {
                    self.error(
                        "P1201",
                        Some(span),
                        None,
                        "A pure record class may only use @efct.pure and @dataclass(frozen=True, slots=True)",
                    );
                    return;
                }
                let mut fields = Vec::new();
                for item in body {
                    match item {
                        ClassItemNode::Field {
                            name,
                            annotation,
                            has_value,
                            span,
                        } => {
                            if has_value {
                                self.error(
                                    "P1201",
                                    Some(span),
                                    None,
                                    "Pure record fields cannot have default values",
                                );
                                continue;
                            }
                            if let Some(annotation) = self.lower_expression(annotation, None) {
                                fields.push(RecordField {
                                    name,
                                    annotation,
                                    span,
                                });
                            }
                        }
                        ClassItemNode::Unsupported { node, span } => self.error(
                            "P1201",
                            Some(span),
                            None,
                            format!("Pure records do not allow class body node {node}"),
                        ),
                        ClassItemNode::Docstring { span } | ClassItemNode::Pass { span } => self
                            .error(
                                "P1201",
                                Some(span),
                                None,
                                "Pure records only allow annotated fields",
                            ),
                    }
                }
                if fields.is_empty() {
                    self.error(
                        "P1201",
                        Some(span),
                        None,
                        "A pure record requires at least one field",
                    );
                    return;
                }
                module.records.push(RecordDefinition { name, fields, span });
            }
            ModuleItem::TypeIgnore { span, .. } => {
                self.error("P1502", Some(span), None, "# type: ignore is not allowed");
            }
            ModuleItem::Statement { .. } => {
                unreachable!("module statements are lowered before declaration items")
            }
            ModuleItem::Unsupported { node, span } => {
                self.error(
                    "P1401",
                    Some(span),
                    None,
                    format!("Python syntax node {node} is not supported"),
                );
            }
        }
    }

    fn lower_expression(
        &mut self,
        expression: ExpressionNode,
        function: Option<&str>,
    ) -> Option<Expression> {
        match expression {
            ExpressionNode::Name {
                identifier,
                context,
                span,
            } => {
                if context == ExpressionContext::Unknown {
                    self.error("P1401", Some(span), function, "Unknown name context");
                    return None;
                }
                Some(Expression::Name { identifier, span })
            }
            ExpressionNode::Constant { value, span } => {
                if let ConstantValue::Unsupported(kind) = &value {
                    self.error(
                        "P1401",
                        Some(span),
                        function,
                        format!("Constant type {kind} is not supported"),
                    );
                    return None;
                }
                Some(Expression::Constant { value, span })
            }
            ExpressionNode::Tuple { elements, span, .. } => Some(Expression::Tuple {
                elements: elements
                    .into_iter()
                    .map(|element| self.lower_expression(element, function))
                    .collect::<Option<Vec<_>>>()?,
                span,
            }),
            ExpressionNode::List { elements, span, .. } => Some(Expression::List {
                elements: elements
                    .into_iter()
                    .map(|element| self.lower_expression(element, function))
                    .collect::<Option<Vec<_>>>()?,
                span,
            }),
            ExpressionNode::Unary {
                operator,
                operand,
                span,
            } => Some(Expression::Unary {
                operator,
                operand: Box::new(self.lower_expression(*operand, function)?),
                span,
            }),
            ExpressionNode::Binary {
                operator,
                left,
                right,
                span,
            } => Some(Expression::Binary {
                operator,
                left: Box::new(self.lower_expression(*left, function)?),
                right: Box::new(self.lower_expression(*right, function)?),
                span,
            }),
            ExpressionNode::Boolean {
                operator,
                values,
                span,
            } => Some(Expression::Boolean {
                operator,
                values: values
                    .into_iter()
                    .map(|value| self.lower_expression(value, function))
                    .collect::<Option<Vec<_>>>()?,
                span,
            }),
            ExpressionNode::Compare {
                left,
                operators,
                comparators,
                span,
            } => Some(Expression::Compare {
                left: Box::new(self.lower_expression(*left, function)?),
                operators,
                comparators: comparators
                    .into_iter()
                    .map(|value| self.lower_expression(value, function))
                    .collect::<Option<Vec<_>>>()?,
                span,
            }),
            ExpressionNode::Conditional {
                condition,
                then_value,
                else_value,
                span,
            } => Some(Expression::Conditional {
                condition: Box::new(self.lower_expression(*condition, function)?),
                then_value: Box::new(self.lower_expression(*then_value, function)?),
                else_value: Box::new(self.lower_expression(*else_value, function)?),
                span,
            }),
            ExpressionNode::Call {
                callee,
                arguments,
                keywords,
                span,
            } => {
                if !keywords.is_empty() {
                    self.error(
                        "P1106",
                        Some(span),
                        function,
                        "Calls in the MVP only allow positional arguments",
                    );
                    return None;
                }
                Some(Expression::Call {
                    callee: Box::new(self.lower_expression(*callee, function)?),
                    arguments: arguments
                        .into_iter()
                        .map(|argument| self.lower_expression(argument, function))
                        .collect::<Option<Vec<_>>>()?,
                    span,
                })
            }
            ExpressionNode::Attribute {
                value, name, span, ..
            } => Some(Expression::Attribute {
                value: Box::new(self.lower_expression(*value, function)?),
                name,
                span,
            }),
            ExpressionNode::Subscript {
                value, slice, span, ..
            } => Some(Expression::Subscript {
                value: Box::new(self.lower_expression(*value, function)?),
                slice: Box::new(self.lower_expression(*slice, function)?),
                span,
            }),
            ExpressionNode::Unsupported { node, span } => {
                self.error(
                    "P1401",
                    Some(span),
                    function,
                    format!("Python expression {node} is not supported"),
                );
                None
            }
        }
    }

    fn error(
        &mut self,
        code: &'static str,
        span: Option<efct_protocol::SourceSpan>,
        function: Option<&str>,
        message: impl Into<String>,
    ) {
        self.diagnostics.push(Diagnostic::error(
            code,
            self.filename.clone(),
            span,
            function.map(str::to_owned),
            message,
        ));
    }
}

fn is_pure_record_decorators(decorators: &[ExpressionNode], imports: &[Import]) -> bool {
    if decorators.len() != 2 || !is_efct_name(&decorators[0], "pure", imports) {
        return false;
    }
    let ExpressionNode::Call {
        callee,
        arguments,
        keywords,
        ..
    } = &decorators[1]
    else {
        return false;
    };
    if !is_qualified_name(callee, &["dataclass"]) || !arguments.is_empty() || keywords.len() != 2 {
        return false;
    }
    let names: std::collections::BTreeSet<&str> = keywords
        .iter()
        .filter_map(|keyword| match (&keyword.name, &keyword.value) {
            (
                Some(name),
                ExpressionNode::Constant {
                    value: ConstantValue::Bool(true),
                    ..
                },
            ) => Some(name.as_str()),
            _ => None,
        })
        .collect();
    names == std::collections::BTreeSet::from(["frozen", "slots"])
}

fn is_qualified_name(expression: &ExpressionNode, segments: &[&str]) -> bool {
    match segments {
        [name] => matches!(
            expression,
            ExpressionNode::Name { identifier, .. } if identifier == name
        ),
        [prefix @ .., name] => matches!(
            expression,
            ExpressionNode::Attribute { value, name: attribute, .. }
                if attribute == name && is_qualified_name(value, prefix)
        ),
        [] => false,
    }
}

fn is_efct_name(expression: &ExpressionNode, name: &str, imports: &[Import]) -> bool {
    canonical_efct_name(expression, imports).as_deref() == Some(name)
}

fn canonical_efct_name(expression: &ExpressionNode, imports: &[Import]) -> Option<String> {
    let lexical = expression_qualified_name(expression)?;
    let (root, suffix) = lexical
        .split_once('.')
        .map_or((lexical.as_str(), ""), |(root, suffix)| (root, suffix));
    imports.iter().find_map(|import| match import {
        Import::Module { path, binding, .. } if path == "efct" && binding == root => {
            Some(suffix.to_owned())
        }
        Import::Symbol {
            module,
            name,
            binding,
            ..
        } if module == "efct" && binding == root => Some(if suffix.is_empty() {
            name.clone()
        } else {
            format!("{name}.{suffix}")
        }),
        _ => None,
    })
}

fn expression_qualified_name(expression: &ExpressionNode) -> Option<String> {
    match expression {
        ExpressionNode::Name { identifier, .. } => Some(identifier.clone()),
        ExpressionNode::Attribute { value, name, .. } => {
            Some(format!("{}.{}", expression_qualified_name(value)?, name))
        }
        _ => None,
    }
}
