use std::collections::{HashMap, HashSet};

use efct_model::Diagnostic;
use efct_protocol::{
    EcmaBinaryOperator, EcmaEffectContract, EcmaExpressionNode, EcmaExternalEffect,
    EcmaFunctionContract, EcmaFunctionNode, EcmaModuleItem, EcmaOptionalAbsence,
    EcmaPartialBehavior, EcmaPartialContract, EcmaStatementNode, EcmaTypeNode, EcmaUnaryOperator,
};

pub(crate) fn check_module(
    filename: String,
    language: &str,
    items: Vec<EcmaModuleItem>,
) -> Vec<Diagnostic> {
    check_module_with_imports(filename, language, items, HashMap::new())
}

pub(crate) fn check_module_with_imports(
    filename: String,
    language: &str,
    items: Vec<EcmaModuleItem>,
    external_functions: HashMap<String, ExternalFunction>,
) -> Vec<Diagnostic> {
    let mut analyzer = Analyzer::new(filename, language, external_functions);
    analyzer.check_items(items);
    analyzer.diagnostics
}

struct Analyzer<'a> {
    filename: String,
    language: &'a str,
    diagnostics: Vec<Diagnostic>,
    declaration_imports: HashSet<String>,
    constants: HashMap<String, ValueType>,
    functions: HashMap<String, FunctionSignature>,
    function_behaviors: HashMap<String, FunctionBehavior>,
    external_functions: HashMap<String, ExternalFunction>,
    builtin_imports: HashMap<String, String>,
}

impl<'a> Analyzer<'a> {
    fn new(
        filename: String,
        language: &'a str,
        external_functions: HashMap<String, ExternalFunction>,
    ) -> Self {
        Self {
            filename,
            language,
            diagnostics: Vec::new(),
            declaration_imports: HashSet::new(),
            constants: HashMap::new(),
            functions: HashMap::new(),
            function_behaviors: HashMap::new(),
            external_functions,
            builtin_imports: HashMap::new(),
        }
    }

    fn check_items(&mut self, items: Vec<EcmaModuleItem>) {
        self.declaration_imports = items
            .iter()
            .filter_map(|item| match item {
                EcmaModuleItem::Import { module, names, .. } if module == "efct" => Some(names),
                _ => None,
            })
            .flatten()
            .filter(|name| !name.type_only && name.imported == name.local)
            .map(|name| name.imported.clone())
            .collect();
        self.builtin_imports = collect_builtin_imports(&items);
        for item in &items {
            if let EcmaModuleItem::Constant {
                name,
                annotation,
                value,
                ..
            } = item
            {
                self.check_constant(name, annotation.as_ref(), value);
            }
        }
        self.register_functions(&items);
        let mut callable_signatures = self.functions.clone();
        callable_signatures.extend(
            self.external_functions
                .iter()
                .map(|(name, function)| (name.clone(), function.signature.clone())),
        );
        let external_behaviors = self
            .external_functions
            .iter()
            .map(|(name, function)| (name.clone(), function.behavior.clone()))
            .collect();
        self.function_behaviors = solve_function_behaviors(
            &items,
            &callable_signatures,
            &self.builtin_imports,
            external_behaviors,
        );
        let mut module_definition_count = 0_u32;
        for item in items {
            match item {
                EcmaModuleItem::Import {
                    module,
                    resolved,
                    names,
                    ..
                } => {
                    self.check_import(&module, resolved.as_deref(), names);
                }
                EcmaModuleItem::Constant { .. } => {}
                EcmaModuleItem::ModuleDefinition {
                    exports, functions, ..
                } => {
                    module_definition_count += 1;
                    self.check_module_definition(exports, functions);
                }
                EcmaModuleItem::Unsupported { node, .. } => {
                    self.unsupported(format!("Unsupported {} syntax node: {node}", self.language))
                }
            }
        }
        if module_definition_count > 1 {
            self.invalid_declaration("A source file may contain only one defineModule declaration");
        }
    }

    fn register_functions(&mut self, items: &[EcmaModuleItem]) {
        for function in items
            .iter()
            .filter_map(|item| match item {
                EcmaModuleItem::ModuleDefinition { functions, .. } => Some(functions.as_slice()),
                _ => None,
            })
            .flatten()
        {
            let Some(parameters) = function
                .parameters
                .iter()
                .map(|parameter| value_type_from_node(&parameter.annotation))
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            let Some(returns) = value_type_from_node(&function.returns) else {
                continue;
            };
            self.functions.insert(
                function.name.clone(),
                FunctionSignature {
                    parameters,
                    returns,
                },
            );
        }
    }

    fn check_constant(
        &mut self,
        name: &str,
        annotation: Option<&EcmaTypeNode>,
        value: &EcmaExpressionNode,
    ) {
        let Some(summary) = self.infer_expression(value, &HashMap::new()) else {
            return;
        };
        if !summary.effects.is_empty() || !summary.partials.is_empty() {
            self.invalid_declaration(format!(
                "Module constant {name} must use a closed pure initializer"
            ));
            return;
        }
        if let Some(annotation) = annotation {
            let Some(declared) = self.check_type(annotation) else {
                return;
            };
            if declared != summary.value_type {
                self.type_error(format!(
                    "Module constant {name} has type {} but declares {}",
                    summary.value_type.name(),
                    declared.name()
                ));
                return;
            }
        }
        if self
            .constants
            .insert(name.to_owned(), summary.value_type)
            .is_some()
        {
            self.invalid_declaration(format!("Duplicate module constant: {name}"));
        }
    }

    fn check_import(
        &mut self,
        module: &str,
        resolved: Option<&str>,
        names: Vec<efct_protocol::EcmaImportName>,
    ) {
        if module != "efct" {
            if resolved.is_some() {
                for name in names {
                    if name.type_only {
                        self.unsupported(format!(
                            "Type-only local import {} is not supported in Efct 0.1",
                            name.imported
                        ));
                    } else if !self.external_functions.contains_key(&name.local) {
                        self.invalid_declaration(format!(
                            "Local import {} from {module} does not resolve to an explicit Efct function contract",
                            name.imported
                        ));
                    }
                }
                return;
            }
            self.check_builtin_import(module, names);
            return;
        }
        let mut local_names = HashSet::new();
        for name in names {
            if name.type_only {
                self.invalid_declaration(format!(
                    "Efct declaration import {} cannot be type-only",
                    name.imported
                ));
            }
            if !matches!(
                name.imported.as_str(),
                "defineModule" | "effect" | "effects" | "partial" | "pure"
            ) {
                self.unsupported(format!("Unsupported efct import: {}", name.imported));
            }
            if name.local != name.imported {
                self.invalid_declaration(format!(
                    "Efct declaration import {} cannot be aliased as {}",
                    name.imported, name.local
                ));
            }
            if !local_names.insert(name.local.clone()) {
                self.invalid_declaration(format!("Duplicate import binding: {}", name.local));
            }
        }
    }

    fn check_builtin_import(&mut self, module: &str, names: Vec<efct_protocol::EcmaImportName>) {
        let supported: &[&str] = match module {
            "node:fs" => &["readFileSync", "writeFileSync"],
            "node:child_process" => &["spawnSync"],
            _ => {
                self.unsupported(format!(
                    "Imports from {module:?} require project-level module verification"
                ));
                return;
            }
        };
        let mut locals = HashSet::new();
        for name in names {
            if name.type_only || !supported.contains(&name.imported.as_str()) {
                self.unsupported(format!(
                    "Unsupported import {} from {module}",
                    name.imported
                ));
            }
            if !locals.insert(name.local.clone()) {
                self.invalid_declaration(format!("Duplicate import binding: {}", name.local));
            }
        }
    }

    fn check_module_definition(&mut self, exports: Vec<String>, functions: Vec<EcmaFunctionNode>) {
        let export_names = self.unique_names("export", &exports);
        let function_names: Vec<_> = functions
            .iter()
            .map(|function| function.name.clone())
            .collect();
        let declared_names = self.unique_names("function", &function_names);
        if export_names != declared_names {
            self.invalid_declaration(
                "The defineModule destructuring exports must exactly match its function declarations",
            );
        }
        for function in functions {
            self.check_function(function);
        }
    }

    fn unique_names(&mut self, kind: &str, names: &[String]) -> HashSet<String> {
        let mut unique = HashSet::new();
        for name in names {
            if !unique.insert(name.clone()) {
                self.invalid_declaration(format!("Duplicate {kind} name: {name}"));
            }
        }
        unique
    }

    fn check_function(&mut self, function: EcmaFunctionNode) {
        let EcmaFunctionNode {
            name,
            contract,
            parameters: parameter_nodes,
            returns,
            body,
            ..
        } = function;
        let mut parameters = HashMap::new();
        for parameter in parameter_nodes {
            let Some(annotation) = self.check_type(&parameter.annotation) else {
                continue;
            };
            if annotation == ValueType::Void {
                self.type_error(format!(
                    "Parameter {} in {} cannot have type void",
                    parameter.name, name
                ));
            }
            if parameters
                .insert(parameter.name.clone(), annotation)
                .is_some()
            {
                self.invalid_declaration(format!(
                    "Duplicate parameter {} in {}",
                    parameter.name, name
                ));
            }
        }
        let Some(expected_return) = self.check_type(&returns) else {
            return;
        };
        let flow = self.check_statements(&name, expected_return, &body, &mut parameters);
        if flow.may_fallthrough && expected_return != ValueType::Void {
            self.type_error(format!(
                "Function {} may complete without returning its declared type {}",
                name,
                expected_return.name()
            ));
        }
        match contract {
            EcmaFunctionContract::Pure { partial } => {
                self.require_declaration_import("pure", &name);
                self.check_effect_contract(&name, None, &flow.effects);
                self.check_partial_contract(&name, partial, &flow.partials);
            }
            EcmaFunctionContract::Effects { effects, partial } => {
                self.require_declaration_import("effects", &name);
                self.check_effect_contract(&name, Some(effects), &flow.effects);
                self.check_partial_contract(&name, partial, &flow.partials);
            }
        }
    }

    fn require_declaration_import(&mut self, imported: &str, function_name: &str) {
        if !self.declaration_imports.contains(imported) {
            self.invalid_declaration(format!(
                "Function {function_name} uses {imported} without importing it directly from efct"
            ));
        }
        if !self.declaration_imports.contains("defineModule") {
            self.invalid_declaration(
                "defineModule must be imported directly from efct for a module definition",
            );
        }
    }

    fn check_statements(
        &mut self,
        function_name: &str,
        expected_return: ValueType,
        statements: &[EcmaStatementNode],
        bindings: &mut HashMap<String, ValueType>,
    ) -> FlowSummary {
        let mut summary = FlowSummary::fallthrough();
        for statement in statements {
            if summary.may_fallthrough {
                let statement_summary =
                    self.check_statement(function_name, expected_return, statement, bindings);
                summary.partials.extend(statement_summary.partials);
                summary.effects.extend(statement_summary.effects);
                summary.may_fallthrough = statement_summary.may_fallthrough;
            }
        }
        summary
    }

    fn check_statement(
        &mut self,
        function_name: &str,
        expected_return: ValueType,
        statement: &EcmaStatementNode,
        bindings: &mut HashMap<String, ValueType>,
    ) -> FlowSummary {
        match statement {
            EcmaStatementNode::Variable {
                name,
                annotation,
                value,
                ..
            } => {
                let Some(summary) = self.infer_expression(value, bindings) else {
                    return FlowSummary::fallthrough();
                };
                let value_type = if let Some(annotation) = annotation {
                    let Some(declared) = self.check_type(annotation) else {
                        return FlowSummary::from_expression(summary);
                    };
                    if declared != summary.value_type {
                        self.type_error(format!(
                            "Local variable {name} has type {} but declares {}",
                            summary.value_type.name(),
                            declared.name()
                        ));
                    }
                    declared
                } else {
                    summary.value_type
                };
                if value_type == ValueType::Void {
                    self.type_error(format!("Local variable {name} cannot have type void"));
                }
                if bindings.insert(name.clone(), value_type).is_some() {
                    self.invalid_declaration(format!("Duplicate local binding: {name}"));
                }
                FlowSummary::from_expression(summary)
            }
            EcmaStatementNode::Assignment { name, value, .. } => {
                let Some(expected) = bindings.get(name).copied() else {
                    self.type_error(format!(
                        "Assignment targets an unknown local binding: {name}"
                    ));
                    return FlowSummary::fallthrough();
                };
                let Some(summary) = self.infer_expression(value, bindings) else {
                    return FlowSummary::fallthrough();
                };
                if summary.value_type != expected {
                    self.type_error(format!(
                        "Assignment to {name} has type {} but requires {}",
                        summary.value_type.name(),
                        expected.name()
                    ));
                }
                FlowSummary::from_expression(summary)
            }
            EcmaStatementNode::Expression { expression, .. } => self
                .infer_expression(expression, bindings)
                .map_or_else(FlowSummary::fallthrough, FlowSummary::from_expression),
            EcmaStatementNode::Return { value, .. } => {
                self.check_return(function_name, expected_return, value.as_ref(), bindings)
            }
            EcmaStatementNode::If {
                condition,
                then_body,
                else_body,
                ..
            } => {
                let condition_flow = self.require_boolean(condition, bindings);
                let (mut then_parameters, mut else_parameters) =
                    narrowed_parameters(condition, bindings);
                let then_flow = self.check_statements(
                    function_name,
                    expected_return,
                    then_body,
                    &mut then_parameters,
                );
                let else_flow = self.check_statements(
                    function_name,
                    expected_return,
                    else_body,
                    &mut else_parameters,
                );
                condition_flow.then(FlowSummary::branches(then_flow, else_flow))
            }
            EcmaStatementNode::While {
                condition, body, ..
            } => {
                let condition_flow = self.require_boolean(condition, bindings);
                let mut body_bindings = bindings.clone();
                let body_flow =
                    self.check_statements(function_name, expected_return, body, &mut body_bindings);
                let loop_flow = match boolean_literal(condition) {
                    Some(false) => FlowSummary::fallthrough(),
                    Some(true) if !body_flow.may_fallthrough => body_flow,
                    Some(true) => body_flow.with_diverge(false),
                    None if body_flow.may_fallthrough => body_flow.with_diverge(true),
                    None => FlowSummary {
                        may_fallthrough: true,
                        partials: body_flow.partials,
                        effects: body_flow.effects,
                    },
                };
                condition_flow.then(loop_flow)
            }
            EcmaStatementNode::Throw { value, .. } => {
                let mut flow = self.check_thrown_value(value, bindings);
                flow.partials.insert(EcmaPartialBehavior::Throw);
                flow.may_fallthrough = false;
                flow
            }
            EcmaStatementNode::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                let mut body_bindings = bindings.clone();
                let body_flow =
                    self.check_statements(function_name, expected_return, body, &mut body_bindings);
                let mut combined = if let Some(catch_body) = catch_body {
                    let catches_throw = body_flow.partials.contains(&EcmaPartialBehavior::Throw);
                    let mut protected = body_flow;
                    protected.partials.remove(&EcmaPartialBehavior::Throw);
                    let mut catch_bindings = bindings.clone();
                    let catch_flow = self.check_statements(
                        function_name,
                        expected_return,
                        catch_body,
                        &mut catch_bindings,
                    );
                    if catches_throw {
                        FlowSummary::branches(protected, catch_flow)
                    } else {
                        protected
                    }
                } else {
                    body_flow
                };
                if let Some(finally_body) = finally_body {
                    let mut finally_bindings = bindings.clone();
                    let finally_flow = self.check_statements(
                        function_name,
                        expected_return,
                        finally_body,
                        &mut finally_bindings,
                    );
                    combined.effects.extend(finally_flow.effects);
                    if finally_flow.may_fallthrough {
                        combined.partials.extend(finally_flow.partials);
                    } else {
                        combined.partials = finally_flow.partials;
                        combined.may_fallthrough = false;
                    }
                }
                combined
            }
            EcmaStatementNode::Unsupported { node, .. } => {
                self.unsupported(format!("Unsupported statement in {function_name}: {node}"));
                FlowSummary::fallthrough()
            }
        }
    }

    fn check_thrown_value(
        &mut self,
        value: &EcmaExpressionNode,
        parameters: &HashMap<String, ValueType>,
    ) -> FlowSummary {
        if let EcmaExpressionNode::Error { message, .. } = value {
            let Some(message) = message else {
                return FlowSummary::fallthrough();
            };
            let Some(summary) = self.infer_expression(message, parameters) else {
                return FlowSummary::fallthrough();
            };
            if summary.value_type != ValueType::String {
                self.type_error(format!(
                    "Error constructor message must be string, not {}",
                    summary.value_type.name()
                ));
            }
            return FlowSummary::from_expression(summary);
        }
        self.infer_expression(value, parameters)
            .map_or_else(FlowSummary::fallthrough, FlowSummary::from_expression)
    }

    fn require_boolean(
        &mut self,
        condition: &EcmaExpressionNode,
        parameters: &HashMap<String, ValueType>,
    ) -> FlowSummary {
        let Some(summary) = self.infer_expression(condition, parameters) else {
            return FlowSummary::fallthrough();
        };
        if summary.value_type != ValueType::Boolean {
            self.type_error(format!(
                "Control-flow condition must be boolean, not {}",
                summary.value_type.name()
            ));
        }
        FlowSummary::from_expression(summary)
    }

    fn check_partial_contract(
        &mut self,
        function_name: &str,
        partial: EcmaPartialContract,
        actual: &HashSet<EcmaPartialBehavior>,
    ) {
        let allowed = match partial {
            EcmaPartialContract::Inferred => return,
            EcmaPartialContract::ExplicitEmpty => HashSet::new(),
            EcmaPartialContract::Explicit { behaviors } => {
                let allowed: HashSet<_> = behaviors.iter().copied().collect();
                if allowed.len() != behaviors.len() {
                    self.invalid_declaration(format!(
                        "Function {function_name} declares a duplicate partial behavior"
                    ));
                }
                allowed
            }
        };
        let mut undeclared: Vec<_> = actual.difference(&allowed).copied().collect();
        undeclared.sort_by_key(|behavior| format!("{behavior:?}"));
        if !undeclared.is_empty() {
            self.error(
                "J0004",
                format!(
                    "Function {function_name} produces partial behaviors outside its whitelist: {undeclared:?}"
                ),
            );
        }
    }

    fn check_effect_contract(
        &mut self,
        function_name: &str,
        contract: Option<EcmaEffectContract>,
        actual: &HashSet<EcmaExternalEffect>,
    ) {
        let allowed = match contract {
            None => HashSet::new(),
            Some(EcmaEffectContract::Inferred) => return,
            Some(EcmaEffectContract::Explicit { effects }) => {
                let allowed: HashSet<_> = effects.iter().copied().collect();
                if allowed.len() != effects.len() {
                    self.invalid_declaration(format!(
                        "Function {function_name} declares a duplicate external effect"
                    ));
                }
                allowed
            }
        };
        let mut undeclared: Vec<_> = actual.difference(&allowed).copied().collect();
        undeclared.sort_by_key(|effect| format!("{effect:?}"));
        if !undeclared.is_empty() {
            self.error(
                "J0005",
                format!(
                    "Function {function_name} produces external effects outside its whitelist: {undeclared:?}"
                ),
            );
        }
    }

    fn check_return(
        &mut self,
        function_name: &str,
        expected: ValueType,
        value: Option<&EcmaExpressionNode>,
        parameters: &HashMap<String, ValueType>,
    ) -> FlowSummary {
        let summary = match value {
            Some(expression) => self.infer_expression(expression, parameters),
            None => Some(ExpressionSummary::pure(ValueType::Undefined)),
        };
        let Some(summary) = summary else {
            return FlowSummary::terminated();
        };
        let actual = summary.value_type;
        let matches =
            expected == actual || expected == ValueType::Void && actual == ValueType::Undefined;
        if !matches {
            self.type_error(format!(
                "Function {function_name} returns {} but declares {}",
                actual.name(),
                expected.name()
            ));
        }
        let mut flow = FlowSummary::from_expression(summary);
        flow.may_fallthrough = false;
        flow
    }

    fn check_type(&mut self, node: &EcmaTypeNode) -> Option<ValueType> {
        match node {
            EcmaTypeNode::Undefined => Some(ValueType::Undefined),
            EcmaTypeNode::Null => Some(ValueType::Null),
            EcmaTypeNode::Boolean => Some(ValueType::Boolean),
            EcmaTypeNode::Number => Some(ValueType::Number),
            EcmaTypeNode::BigInt => Some(ValueType::BigInt),
            EcmaTypeNode::String => Some(ValueType::String),
            EcmaTypeNode::Void => Some(ValueType::Void),
            EcmaTypeNode::Optional { value, absence } => {
                let value = self.check_type(value)?;
                let base = match value {
                    ValueType::Boolean => PrimitiveValueType::Boolean,
                    ValueType::Number => PrimitiveValueType::Number,
                    ValueType::BigInt => PrimitiveValueType::BigInt,
                    ValueType::String => PrimitiveValueType::String,
                    _ => {
                        self.type_error("Optional values require one non-nullish primitive member");
                        return None;
                    }
                };
                Some(match absence {
                    EcmaOptionalAbsence::Null => ValueType::OptionalNull(base),
                    EcmaOptionalAbsence::Undefined => ValueType::OptionalUndefined(base),
                })
            }
            EcmaTypeNode::Unsupported { node, .. } => {
                self.unsupported(format!("Unsupported type syntax: {node}"));
                None
            }
        }
    }

    fn infer_expression(
        &mut self,
        expression: &EcmaExpressionNode,
        parameters: &HashMap<String, ValueType>,
    ) -> Option<ExpressionSummary> {
        match expression {
            EcmaExpressionNode::Identifier { name, .. } => parameters
                .get(name)
                .copied()
                .or_else(|| self.constants.get(name).copied())
                .map(ExpressionSummary::pure)
                .or_else(|| {
                    self.type_error(format!("Unknown pure function binding: {name}"));
                    None
                }),
            EcmaExpressionNode::Undefined { .. } => {
                Some(ExpressionSummary::pure(ValueType::Undefined))
            }
            EcmaExpressionNode::Null { .. } => Some(ExpressionSummary::pure(ValueType::Null)),
            EcmaExpressionNode::Boolean { .. } => Some(ExpressionSummary::pure(ValueType::Boolean)),
            EcmaExpressionNode::Number { .. } => Some(ExpressionSummary::pure(ValueType::Number)),
            EcmaExpressionNode::BigInt { .. } => Some(ExpressionSummary::pure(ValueType::BigInt)),
            EcmaExpressionNode::String { value, .. } => {
                Some(ExpressionSummary::string_literal(value.clone()))
            }
            EcmaExpressionNode::Unary {
                operator, operand, ..
            } => {
                let mut operand = self.infer_expression(operand, parameters)?;
                operand.value_type = self.infer_unary(*operator, operand.value_type)?;
                Some(operand)
            }
            EcmaExpressionNode::Binary {
                left,
                operator,
                right,
                ..
            } => {
                let left = self.infer_expression(left, parameters)?;
                let right = self.infer_expression(right, parameters)?;
                let result = self.infer_binary(*operator, left.value_type, right.value_type)?;
                Some(ExpressionSummary::merge(left, right, result))
            }
            EcmaExpressionNode::Conditional {
                condition,
                when_true,
                when_false,
                ..
            } => {
                let condition = self.infer_expression(condition, parameters)?;
                if condition.value_type != ValueType::Boolean {
                    self.type_error(format!(
                        "Conditional expression requires boolean, not {}",
                        condition.value_type.name()
                    ));
                }
                let when_true = self.infer_expression(when_true, parameters)?;
                let when_false = self.infer_expression(when_false, parameters)?;
                if when_true.value_type != when_false.value_type {
                    self.type_error(format!(
                        "Conditional branches have incompatible types {} and {}",
                        when_true.value_type.name(),
                        when_false.value_type.name()
                    ));
                    return None;
                }
                let result_type = when_true.value_type;
                let branches = ExpressionSummary::merge(when_true, when_false, result_type);
                Some(ExpressionSummary::merge(condition, branches, result_type))
            }
            EcmaExpressionNode::Call {
                target, arguments, ..
            } => {
                let mut argument_summaries = Vec::with_capacity(arguments.len());
                for argument in arguments {
                    argument_summaries.push(self.infer_expression(argument, parameters)?);
                }
                self.infer_call(target, argument_summaries)
            }
            EcmaExpressionNode::Error { .. } => {
                self.unsupported("Error objects are only supported as immediate operands of throw");
                None
            }
            EcmaExpressionNode::Property { target, .. } => self.infer_property(target),
            EcmaExpressionNode::Unsupported { node, .. } => {
                self.unsupported(format!("Unsupported expression syntax: {node}"));
                None
            }
        }
    }

    fn infer_call(
        &mut self,
        target: &[String],
        argument_summaries: Vec<ExpressionSummary>,
    ) -> Option<ExpressionSummary> {
        if let [name] = target
            && let Some(signature) = self.functions.get(name).cloned().or_else(|| {
                self.external_functions
                    .get(name)
                    .map(|function| function.signature.clone())
            })
        {
            if signature.parameters.len() != argument_summaries.len() {
                self.type_error(format!(
                    "Function {name} expects {} arguments but received {}",
                    signature.parameters.len(),
                    argument_summaries.len()
                ));
                return None;
            }
            for (index, (expected, actual)) in signature
                .parameters
                .iter()
                .zip(&argument_summaries)
                .enumerate()
            {
                if *expected != actual.value_type {
                    self.type_error(format!(
                        "Argument {} to {name} has type {} but requires {}",
                        index + 1,
                        actual.value_type.name(),
                        expected.name()
                    ));
                }
            }
            let mut summary = ExpressionSummary::pure(signature.returns);
            for argument in argument_summaries {
                summary.extend(argument);
            }
            if let Some(behavior) = self.function_behaviors.get(name) {
                summary.effects.extend(behavior.effects.iter().copied());
                summary.partials.extend(behavior.partials.iter().copied());
            }
            return Some(summary);
        }
        let argument_count = argument_summaries.len();
        let argument_types: Vec<_> = argument_summaries
            .iter()
            .map(|summary| summary.value_type)
            .collect();
        let string_literals: Vec<_> = argument_summaries
            .iter()
            .map(|summary| summary.string_literal.clone())
            .collect();
        let mut summary = ExpressionSummary::pure(ValueType::Undefined);
        for argument in argument_summaries {
            summary.extend(argument);
        }
        let path = canonical_call_path(target, &self.builtin_imports);
        self.infer_builtin_call(
            &path,
            argument_count,
            &argument_types,
            &string_literals,
            summary,
        )
    }

    fn infer_builtin_call(
        &mut self,
        path: &str,
        argument_count: usize,
        argument_types: &[ValueType],
        string_literals: &[Option<String>],
        mut summary: ExpressionSummary,
    ) -> Option<ExpressionSummary> {
        match (path, argument_count) {
            ("Date.now" | "performance.now", 0) => {
                summary.value_type = ValueType::Number;
                summary.effects.insert(EcmaExternalEffect::Clock);
            }
            ("Math.random", 0) => {
                summary.value_type = ValueType::Number;
                summary.effects.insert(EcmaExternalEffect::Random);
            }
            ("console.log" | "console.error", _) => {
                summary.value_type = ValueType::Undefined;
                summary.effects.insert(EcmaExternalEffect::Console);
                summary.partials.insert(EcmaPartialBehavior::Throw);
            }
            ("node:fs.readFileSync", 2)
                if argument_types == [ValueType::String, ValueType::String]
                    && string_literals[1].as_deref() == Some("utf8") =>
            {
                summary.value_type = ValueType::String;
                summary.effects.insert(EcmaExternalEffect::FileRead);
                summary.partials.insert(EcmaPartialBehavior::Throw);
            }
            ("node:fs.writeFileSync", 2)
                if argument_types == [ValueType::String, ValueType::String] =>
            {
                summary.value_type = ValueType::Undefined;
                summary.effects.insert(EcmaExternalEffect::FileWrite);
                summary.partials.insert(EcmaPartialBehavior::Throw);
            }
            ("node:child_process.spawnSync", 1) if argument_types == [ValueType::String] => {
                summary.value_type = ValueType::Opaque;
                summary.effects.insert(EcmaExternalEffect::Process);
                summary.partials.insert(EcmaPartialBehavior::Throw);
                summary.partials.insert(EcmaPartialBehavior::Diverge);
            }
            _ => {
                self.unsupported(format!("Unsupported or unknown call target: {path}"));
                return None;
            }
        }
        Some(summary)
    }

    fn infer_property(&mut self, target: &[String]) -> Option<ExpressionSummary> {
        if target.len() == 3 && target[0] == "process" && target[1] == "env" {
            let mut summary =
                ExpressionSummary::pure(ValueType::OptionalUndefined(PrimitiveValueType::String));
            summary.effects.insert(EcmaExternalEffect::Environment);
            return Some(summary);
        }
        self.unsupported(format!(
            "Unsupported or unsafe property access: {}",
            target.join(".")
        ));
        None
    }

    fn infer_unary(
        &mut self,
        operator: EcmaUnaryOperator,
        operand: ValueType,
    ) -> Option<ValueType> {
        let result = match operator {
            EcmaUnaryOperator::Positive if operand == ValueType::Number => ValueType::Number,
            EcmaUnaryOperator::Negative
                if matches!(operand, ValueType::Number | ValueType::BigInt) =>
            {
                operand
            }
            EcmaUnaryOperator::Not if operand == ValueType::Boolean => ValueType::Boolean,
            _ => {
                self.type_error(format!(
                    "Unary operator {operator:?} does not accept {}",
                    operand.name()
                ));
                return None;
            }
        };
        Some(result)
    }

    fn infer_binary(
        &mut self,
        operator: EcmaBinaryOperator,
        left: ValueType,
        right: ValueType,
    ) -> Option<ValueType> {
        let same = left == right;
        let numeric = same && matches!(left, ValueType::Number | ValueType::BigInt);
        let result = match operator {
            EcmaBinaryOperator::Add
                if same
                    && matches!(
                        left,
                        ValueType::Number | ValueType::BigInt | ValueType::String
                    ) =>
            {
                left
            }
            EcmaBinaryOperator::Subtract
            | EcmaBinaryOperator::Multiply
            | EcmaBinaryOperator::Divide
            | EcmaBinaryOperator::Remainder
                if numeric =>
            {
                left
            }
            EcmaBinaryOperator::StrictEqual | EcmaBinaryOperator::StrictNotEqual
                if same || optional_absence_comparison(left, right) =>
            {
                ValueType::Boolean
            }
            EcmaBinaryOperator::Less
            | EcmaBinaryOperator::LessEqual
            | EcmaBinaryOperator::Greater
            | EcmaBinaryOperator::GreaterEqual
                if same
                    && matches!(
                        left,
                        ValueType::Number | ValueType::BigInt | ValueType::String
                    ) =>
            {
                ValueType::Boolean
            }
            EcmaBinaryOperator::And | EcmaBinaryOperator::Or
                if left == ValueType::Boolean && right == ValueType::Boolean =>
            {
                ValueType::Boolean
            }
            _ => {
                self.type_error(format!(
                    "Binary operator {operator:?} does not accept {} and {}",
                    left.name(),
                    right.name()
                ));
                return None;
            }
        };
        Some(result)
    }

    fn unsupported(&mut self, message: impl Into<String>) {
        self.error("J0001", message);
    }

    fn type_error(&mut self, message: impl Into<String>) {
        self.error("J0002", message);
    }

    fn invalid_declaration(&mut self, message: impl Into<String>) {
        self.error("J0003", message);
    }

    fn error(&mut self, code: &'static str, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic::error(
            code,
            self.filename.clone(),
            None,
            None,
            message,
        ));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueType {
    Undefined,
    Null,
    Boolean,
    Number,
    BigInt,
    String,
    Void,
    OptionalNull(PrimitiveValueType),
    OptionalUndefined(PrimitiveValueType),
    Opaque,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrimitiveValueType {
    Boolean,
    Number,
    BigInt,
    String,
}

impl ValueType {
    fn name(self) -> String {
        match self {
            Self::Undefined => "undefined".to_owned(),
            Self::Null => "null".to_owned(),
            Self::Boolean => "boolean".to_owned(),
            Self::Number => "number".to_owned(),
            Self::BigInt => "bigint".to_owned(),
            Self::String => "string".to_owned(),
            Self::Void => "void".to_owned(),
            Self::OptionalNull(value) => format!("{} | null", value.name()),
            Self::OptionalUndefined(value) => format!("{} | undefined", value.name()),
            Self::Opaque => "opaque Node.js value".to_owned(),
        }
    }
}

impl PrimitiveValueType {
    fn name(self) -> &'static str {
        match self {
            Self::Boolean => "boolean",
            Self::Number => "number",
            Self::BigInt => "bigint",
            Self::String => "string",
        }
    }
}

fn optional_absence_comparison(left: ValueType, right: ValueType) -> bool {
    matches!(
        (left, right),
        (ValueType::OptionalNull(_), ValueType::Null)
            | (ValueType::Null, ValueType::OptionalNull(_))
            | (ValueType::OptionalUndefined(_), ValueType::Undefined)
            | (ValueType::Undefined, ValueType::OptionalUndefined(_))
    )
}

fn narrowed_parameters(
    condition: &EcmaExpressionNode,
    parameters: &HashMap<String, ValueType>,
) -> (HashMap<String, ValueType>, HashMap<String, ValueType>) {
    let mut then_parameters = parameters.clone();
    let mut else_parameters = parameters.clone();
    let EcmaExpressionNode::Binary {
        left,
        operator,
        right,
        ..
    } = condition
    else {
        return (then_parameters, else_parameters);
    };
    let Some((name, absence)) = optional_test(left, right).or_else(|| optional_test(right, left))
    else {
        return (then_parameters, else_parameters);
    };
    let Some(original) = parameters.get(name).copied() else {
        return (then_parameters, else_parameters);
    };
    let Some((present, expected_absence)) = split_optional(original) else {
        return (then_parameters, else_parameters);
    };
    if absence != expected_absence {
        return (then_parameters, else_parameters);
    }
    match operator {
        EcmaBinaryOperator::StrictEqual => {
            then_parameters.insert(name.to_owned(), absence);
            else_parameters.insert(name.to_owned(), present);
        }
        EcmaBinaryOperator::StrictNotEqual => {
            then_parameters.insert(name.to_owned(), present);
            else_parameters.insert(name.to_owned(), absence);
        }
        _ => {}
    }
    (then_parameters, else_parameters)
}

fn optional_test<'a>(
    candidate: &'a EcmaExpressionNode,
    absence: &EcmaExpressionNode,
) -> Option<(&'a str, ValueType)> {
    let EcmaExpressionNode::Identifier { name, .. } = candidate else {
        return None;
    };
    match absence {
        EcmaExpressionNode::Null { .. } => Some((name, ValueType::Null)),
        EcmaExpressionNode::Undefined { .. } => Some((name, ValueType::Undefined)),
        _ => None,
    }
}

fn split_optional(value: ValueType) -> Option<(ValueType, ValueType)> {
    match value {
        ValueType::OptionalNull(value) => Some((primitive_value_type(value), ValueType::Null)),
        ValueType::OptionalUndefined(value) => {
            Some((primitive_value_type(value), ValueType::Undefined))
        }
        _ => None,
    }
}

fn primitive_value_type(value: PrimitiveValueType) -> ValueType {
    match value {
        PrimitiveValueType::Boolean => ValueType::Boolean,
        PrimitiveValueType::Number => ValueType::Number,
        PrimitiveValueType::BigInt => ValueType::BigInt,
        PrimitiveValueType::String => ValueType::String,
    }
}

fn value_type_from_node(node: &EcmaTypeNode) -> Option<ValueType> {
    match node {
        EcmaTypeNode::Undefined => Some(ValueType::Undefined),
        EcmaTypeNode::Null => Some(ValueType::Null),
        EcmaTypeNode::Boolean => Some(ValueType::Boolean),
        EcmaTypeNode::Number => Some(ValueType::Number),
        EcmaTypeNode::BigInt => Some(ValueType::BigInt),
        EcmaTypeNode::String => Some(ValueType::String),
        EcmaTypeNode::Void => Some(ValueType::Void),
        EcmaTypeNode::Optional { value, absence } => {
            let base = match value_type_from_node(value)? {
                ValueType::Boolean => PrimitiveValueType::Boolean,
                ValueType::Number => PrimitiveValueType::Number,
                ValueType::BigInt => PrimitiveValueType::BigInt,
                ValueType::String => PrimitiveValueType::String,
                _ => return None,
            };
            Some(match absence {
                EcmaOptionalAbsence::Null => ValueType::OptionalNull(base),
                EcmaOptionalAbsence::Undefined => ValueType::OptionalUndefined(base),
            })
        }
        EcmaTypeNode::Unsupported { .. } => None,
    }
}

#[derive(Clone)]
struct FunctionSignature {
    parameters: Vec<ValueType>,
    returns: ValueType,
}

#[derive(Clone, Default, PartialEq, Eq)]
struct FunctionBehavior {
    effects: HashSet<EcmaExternalEffect>,
    partials: HashSet<EcmaPartialBehavior>,
}

#[derive(Clone)]
pub(crate) struct ExternalFunction {
    signature: FunctionSignature,
    behavior: FunctionBehavior,
}

pub(crate) fn explicit_external_function(
    function: &EcmaFunctionNode,
) -> Result<ExternalFunction, &'static str> {
    let parameters = function
        .parameters
        .iter()
        .map(|parameter| value_type_from_node(&parameter.annotation))
        .collect::<Option<Vec<_>>>()
        .ok_or("Cross-module parameters must use supported exact Efct types")?;
    let returns = value_type_from_node(&function.returns)
        .ok_or("Cross-module returns must use a supported exact Efct type")?;
    let (effects, partial) = match &function.contract {
        EcmaFunctionContract::Pure { partial } => (HashSet::new(), partial),
        EcmaFunctionContract::Effects { effects, partial } => {
            let EcmaEffectContract::Explicit { effects } = effects else {
                return Err("Cross-module effect contracts must be explicit");
            };
            (effects.iter().copied().collect(), partial)
        }
    };
    let partials = match partial {
        EcmaPartialContract::ExplicitEmpty => HashSet::new(),
        EcmaPartialContract::Explicit { behaviors } => behaviors.iter().copied().collect(),
        EcmaPartialContract::Inferred => {
            return Err("Cross-module partial contracts must be explicit");
        }
    };
    Ok(ExternalFunction {
        signature: FunctionSignature {
            parameters,
            returns,
        },
        behavior: FunctionBehavior { effects, partials },
    })
}

fn collect_builtin_imports(items: &[EcmaModuleItem]) -> HashMap<String, String> {
    let mut imports = HashMap::new();
    for (module, names) in items.iter().filter_map(|item| match item {
        EcmaModuleItem::Import { module, names, .. } => Some((module.as_str(), names)),
        _ => None,
    }) {
        for name in names {
            let supported = matches!(
                (module, name.imported.as_str()),
                ("node:fs", "readFileSync" | "writeFileSync") | ("node:child_process", "spawnSync")
            );
            if supported && !name.type_only {
                imports.insert(name.local.clone(), format!("{module}.{}", name.imported));
            }
        }
    }
    imports
}

fn canonical_call_path(target: &[String], builtin_imports: &HashMap<String, String>) -> String {
    if let [local] = target
        && let Some(path) = builtin_imports.get(local)
    {
        return path.clone();
    }
    target.join(".")
}

fn solve_function_behaviors(
    items: &[EcmaModuleItem],
    functions: &HashMap<String, FunctionSignature>,
    builtin_imports: &HashMap<String, String>,
    mut behaviors: HashMap<String, FunctionBehavior>,
) -> HashMap<String, FunctionBehavior> {
    let mut calls = HashMap::new();
    for function in items
        .iter()
        .filter_map(|item| match item {
            EcmaModuleItem::ModuleDefinition { functions, .. } => Some(functions.as_slice()),
            _ => None,
        })
        .flatten()
    {
        let mut behavior = FunctionBehavior::default();
        let mut function_calls = HashSet::new();
        scan_statements(
            &function.body,
            functions,
            builtin_imports,
            &mut behavior,
            &mut function_calls,
        );
        behaviors.insert(function.name.clone(), behavior);
        calls.insert(function.name.clone(), function_calls);
    }
    for name in functions.keys() {
        if is_recursive(name, name, &calls, &mut HashSet::new()) {
            behaviors
                .entry(name.clone())
                .or_default()
                .partials
                .insert(EcmaPartialBehavior::Diverge);
        }
    }
    loop {
        let previous = behaviors.clone();
        for (caller, callees) in &calls {
            let behavior = behaviors.entry(caller.clone()).or_default();
            for callee in callees {
                if let Some(callee_behavior) = previous.get(callee) {
                    behavior
                        .effects
                        .extend(callee_behavior.effects.iter().copied());
                    behavior
                        .partials
                        .extend(callee_behavior.partials.iter().copied());
                }
            }
        }
        if behaviors == previous {
            break;
        }
    }
    behaviors
}

fn scan_statements(
    statements: &[EcmaStatementNode],
    functions: &HashMap<String, FunctionSignature>,
    builtin_imports: &HashMap<String, String>,
    behavior: &mut FunctionBehavior,
    calls: &mut HashSet<String>,
) -> bool {
    let mut may_fallthrough = true;
    for statement in statements {
        if !may_fallthrough {
            break;
        }
        may_fallthrough = match statement {
            EcmaStatementNode::Variable { value, .. }
            | EcmaStatementNode::Assignment { value, .. } => {
                scan_expression(value, functions, builtin_imports, behavior, calls);
                true
            }
            EcmaStatementNode::Expression { expression, .. } => {
                scan_expression(expression, functions, builtin_imports, behavior, calls);
                true
            }
            EcmaStatementNode::Return { value, .. } => {
                if let Some(value) = value {
                    scan_expression(value, functions, builtin_imports, behavior, calls);
                }
                false
            }
            EcmaStatementNode::If {
                condition,
                then_body,
                else_body,
                ..
            } => {
                scan_expression(condition, functions, builtin_imports, behavior, calls);
                let then_falls =
                    scan_statements(then_body, functions, builtin_imports, behavior, calls);
                let else_falls =
                    scan_statements(else_body, functions, builtin_imports, behavior, calls);
                then_falls || else_falls
            }
            EcmaStatementNode::While {
                condition, body, ..
            } => {
                scan_expression(condition, functions, builtin_imports, behavior, calls);
                if boolean_literal(condition) == Some(false) {
                    true
                } else {
                    let body_falls =
                        scan_statements(body, functions, builtin_imports, behavior, calls);
                    if body_falls {
                        behavior.partials.insert(EcmaPartialBehavior::Diverge);
                    }
                    boolean_literal(condition) != Some(true)
                }
            }
            EcmaStatementNode::Throw { value, .. } => {
                scan_expression(value, functions, builtin_imports, behavior, calls);
                behavior.partials.insert(EcmaPartialBehavior::Throw);
                false
            }
            EcmaStatementNode::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                let mut protected_behavior = FunctionBehavior::default();
                let mut protected_calls = HashSet::new();
                let protected_falls = scan_statements(
                    body,
                    functions,
                    builtin_imports,
                    &mut protected_behavior,
                    &mut protected_calls,
                );
                let caught = protected_behavior
                    .partials
                    .contains(&EcmaPartialBehavior::Throw);
                if catch_body.is_some() {
                    protected_behavior
                        .partials
                        .remove(&EcmaPartialBehavior::Throw);
                }
                behavior.effects.extend(protected_behavior.effects);
                behavior.partials.extend(protected_behavior.partials);
                calls.extend(protected_calls);
                let mut falls = protected_falls;
                if let Some(catch_body) = catch_body
                    && caught
                {
                    falls |=
                        scan_statements(catch_body, functions, builtin_imports, behavior, calls);
                }
                if let Some(finally_body) = finally_body {
                    let mut finally_behavior = FunctionBehavior::default();
                    let mut finally_calls = HashSet::new();
                    let finally_falls = scan_statements(
                        finally_body,
                        functions,
                        builtin_imports,
                        &mut finally_behavior,
                        &mut finally_calls,
                    );
                    behavior.effects.extend(finally_behavior.effects);
                    calls.extend(finally_calls);
                    if finally_falls {
                        behavior.partials.extend(finally_behavior.partials);
                    } else {
                        behavior.partials = finally_behavior.partials;
                        falls = false;
                    }
                }
                falls
            }
            EcmaStatementNode::Unsupported { .. } => true,
        };
    }
    may_fallthrough
}

fn scan_expression(
    expression: &EcmaExpressionNode,
    functions: &HashMap<String, FunctionSignature>,
    builtin_imports: &HashMap<String, String>,
    behavior: &mut FunctionBehavior,
    calls: &mut HashSet<String>,
) {
    match expression {
        EcmaExpressionNode::Unary { operand, .. } => {
            scan_expression(operand, functions, builtin_imports, behavior, calls);
        }
        EcmaExpressionNode::Binary { left, right, .. } => {
            scan_expression(left, functions, builtin_imports, behavior, calls);
            scan_expression(right, functions, builtin_imports, behavior, calls);
        }
        EcmaExpressionNode::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            scan_expression(condition, functions, builtin_imports, behavior, calls);
            scan_expression(when_true, functions, builtin_imports, behavior, calls);
            scan_expression(when_false, functions, builtin_imports, behavior, calls);
        }
        EcmaExpressionNode::Call {
            target, arguments, ..
        } => {
            for argument in arguments {
                scan_expression(argument, functions, builtin_imports, behavior, calls);
            }
            let path = canonical_call_path(target, builtin_imports);
            match path.as_str() {
                "Date.now" | "performance.now" => {
                    behavior.effects.insert(EcmaExternalEffect::Clock);
                }
                "Math.random" => {
                    behavior.effects.insert(EcmaExternalEffect::Random);
                }
                "console.log" | "console.error" => {
                    behavior.effects.insert(EcmaExternalEffect::Console);
                    behavior.partials.insert(EcmaPartialBehavior::Throw);
                }
                "node:fs.readFileSync" => {
                    behavior.effects.insert(EcmaExternalEffect::FileRead);
                    behavior.partials.insert(EcmaPartialBehavior::Throw);
                }
                "node:fs.writeFileSync" => {
                    behavior.effects.insert(EcmaExternalEffect::FileWrite);
                    behavior.partials.insert(EcmaPartialBehavior::Throw);
                }
                "node:child_process.spawnSync" => {
                    behavior.effects.insert(EcmaExternalEffect::Process);
                    behavior.partials.insert(EcmaPartialBehavior::Throw);
                    behavior.partials.insert(EcmaPartialBehavior::Diverge);
                }
                _ if target.len() == 1 && functions.contains_key(&target[0]) => {
                    calls.insert(target[0].clone());
                }
                _ => {}
            }
        }
        EcmaExpressionNode::Error {
            message: Some(message),
            ..
        } => scan_expression(message, functions, builtin_imports, behavior, calls),
        EcmaExpressionNode::Error { message: None, .. } => {}
        EcmaExpressionNode::Property { target, .. }
            if target.len() == 3 && target[0] == "process" && target[1] == "env" =>
        {
            behavior.effects.insert(EcmaExternalEffect::Environment);
        }
        _ => {}
    }
}

fn is_recursive(
    origin: &str,
    current: &str,
    calls: &HashMap<String, HashSet<String>>,
    visited: &mut HashSet<String>,
) -> bool {
    let Some(callees) = calls.get(current) else {
        return false;
    };
    for callee in callees {
        if callee == origin {
            return true;
        }
        if visited.insert(callee.clone()) && is_recursive(origin, callee, calls, visited) {
            return true;
        }
    }
    false
}

struct FlowSummary {
    may_fallthrough: bool,
    effects: HashSet<EcmaExternalEffect>,
    partials: HashSet<EcmaPartialBehavior>,
}

impl FlowSummary {
    fn fallthrough() -> Self {
        Self {
            may_fallthrough: true,
            effects: HashSet::new(),
            partials: HashSet::new(),
        }
    }

    fn terminated() -> Self {
        Self {
            may_fallthrough: false,
            effects: HashSet::new(),
            partials: HashSet::new(),
        }
    }

    fn from_expression(summary: ExpressionSummary) -> Self {
        Self {
            may_fallthrough: true,
            effects: summary.effects,
            partials: summary.partials,
        }
    }

    fn branches(mut left: Self, right: Self) -> Self {
        left.effects.extend(right.effects);
        left.partials.extend(right.partials);
        left.may_fallthrough |= right.may_fallthrough;
        left
    }

    fn with_diverge(mut self, may_fallthrough: bool) -> Self {
        self.partials.insert(EcmaPartialBehavior::Diverge);
        self.may_fallthrough = may_fallthrough;
        self
    }

    fn then(mut self, next: Self) -> Self {
        if self.may_fallthrough {
            self.effects.extend(next.effects);
            self.partials.extend(next.partials);
            self.may_fallthrough = next.may_fallthrough;
        }
        self
    }
}

struct ExpressionSummary {
    value_type: ValueType,
    string_literal: Option<String>,
    effects: HashSet<EcmaExternalEffect>,
    partials: HashSet<EcmaPartialBehavior>,
}

impl ExpressionSummary {
    fn pure(value_type: ValueType) -> Self {
        Self {
            value_type,
            string_literal: None,
            effects: HashSet::new(),
            partials: HashSet::new(),
        }
    }

    fn string_literal(value: String) -> Self {
        Self {
            value_type: ValueType::String,
            string_literal: Some(value),
            effects: HashSet::new(),
            partials: HashSet::new(),
        }
    }

    fn merge(mut left: Self, right: Self, value_type: ValueType) -> Self {
        left.value_type = value_type;
        left.string_literal = None;
        left.effects.extend(right.effects);
        left.partials.extend(right.partials);
        left
    }

    fn extend(&mut self, other: Self) {
        self.effects.extend(other.effects);
        self.partials.extend(other.partials);
    }
}

fn boolean_literal(expression: &EcmaExpressionNode) -> Option<bool> {
    match expression {
        EcmaExpressionNode::Boolean { value, .. } => Some(*value),
        _ => None,
    }
}
