use efct_protocol::{ExpressionNode, StatementNode};

use super::LoweringContext;
use crate::hir::{FunctionDeclaration, Import, MODULE_INITIALIZER_NAME, Statement};

pub(super) enum ClassifiedModuleStatement {
    Contract {
        declaration: FunctionDeclaration,
        span: efct_protocol::SourceSpan,
    },
    Initializer(Box<StatementNode>),
    InvalidContract,
}

impl LoweringContext {
    pub(super) fn classify_module_statement(
        &mut self,
        statement: StatementNode,
        imports: &[Import],
    ) -> ClassifiedModuleStatement {
        let StatementNode::Assign {
            targets,
            value,
            type_comment,
            span,
        } = statement
        else {
            return ClassifiedModuleStatement::Initializer(Box::new(statement));
        };
        let assigns_contract = targets.iter().any(
            |target| matches!(target, ExpressionNode::Name { identifier, .. } if identifier == "_efct"),
        );
        if !assigns_contract {
            return ClassifiedModuleStatement::Initializer(Box::new(StatementNode::Assign {
                targets,
                value,
                type_comment,
                span,
            }));
        }
        if targets.len() != 1
            || !matches!(&targets[0], ExpressionNode::Name { identifier, .. } if identifier == "_efct")
        {
            self.error(
                "P1006",
                Some(span),
                None,
                "The `_efct` module contract must be assigned directly",
            );
            return ClassifiedModuleStatement::InvalidContract;
        }
        if type_comment.is_some() {
            self.error(
                "P1502",
                Some(span),
                None,
                "The `_efct` module contract cannot use a type comment",
            );
            return ClassifiedModuleStatement::InvalidContract;
        }
        let Some(declaration) = lower_module_declaration(value, self, span, imports) else {
            return ClassifiedModuleStatement::InvalidContract;
        };
        ClassifiedModuleStatement::Contract { declaration, span }
    }

    pub(super) fn lower_module_initializer_statement(
        &mut self,
        statement: StatementNode,
        body: &mut Vec<Statement>,
    ) {
        match statement {
            StatementNode::Assign { span, .. } => self.error(
                "P1401",
                Some(span),
                None,
                "A module assignment must be an annotated immutable constant",
            ),
            StatementNode::Expression { value, span } => {
                if let Some(value) = self.lower_expression(value, Some(MODULE_INITIALIZER_NAME)) {
                    body.push(Statement::Expression { value, span });
                }
            }
            StatementNode::Pass { .. } => {}
            other => self.error(
                "P1401",
                Some(statement_node_span(&other)),
                None,
                "The current version only supports expression statements in module initialization",
            ),
        }
    }
}

fn lower_module_declaration(
    expression: ExpressionNode,
    context: &mut LoweringContext,
    span: efct_protocol::SourceSpan,
    imports: &[Import],
) -> Option<FunctionDeclaration> {
    if super::is_efct_name(&expression, "pure", imports) {
        return Some(FunctionDeclaration::InferredPure);
    }
    if let ExpressionNode::Call {
        callee,
        arguments,
        keywords,
        ..
    } = expression
        && keywords.is_empty()
    {
        if super::is_efct_name(&callee, "pure", imports) {
            let partials = context.lower_partial_arguments(arguments, None, imports)?;
            return Some(FunctionDeclaration::BoundedPure(partials));
        }
        if !super::is_efct_name(&callee, "effects", imports) {
            context.error(
                "P1006",
                Some(span),
                None,
                "The `_efct` module contract must be `efct.pure(...)` or `efct.effects(...)`",
            );
            return None;
        }
        if arguments.is_empty() {
            context.error(
                "P1006",
                Some(span),
                None,
                "A pure module must declare `_efct = efct.pure` instead of an empty effect set",
            );
            return None;
        }
        let effects = context.lower_effect_arguments(arguments, None, imports)?;
        return Some(FunctionDeclaration::BoundedEffects(effects));
    }
    context.error(
        "P1006",
        Some(span),
        None,
        "The `_efct` module contract must be `efct.pure(...)` or `efct.effects(...)`",
    );
    None
}

pub(super) fn module_import_statements(import: &Import) -> Vec<Statement> {
    let (module, span) = match import {
        Import::Module { path, span, .. } => (path, span),
        Import::Symbol { module, span, .. } => (module, span),
    };
    if crate::python_import_role(module).is_some() {
        return Vec::new();
    }
    let parts = module.split('.').collect::<Vec<_>>();
    (1..=parts.len())
        .map(|length| Statement::ModuleImport {
            module: format!("{}.{MODULE_INITIALIZER_NAME}", parts[..length].join(".")),
            span: *span,
        })
        .collect()
}

fn statement_node_span(statement: &StatementNode) -> efct_protocol::SourceSpan {
    match statement {
        StatementNode::Return { span, .. }
        | StatementNode::Assign { span, .. }
        | StatementNode::AnnotatedAssignment { span, .. }
        | StatementNode::AugmentedAssignment { span, .. }
        | StatementNode::Expression { span, .. }
        | StatementNode::If { span, .. }
        | StatementNode::For { span, .. }
        | StatementNode::While { span, .. }
        | StatementNode::Match { span, .. }
        | StatementNode::Try { span, .. }
        | StatementNode::TryStar { span, .. }
        | StatementNode::With { span, .. }
        | StatementNode::Raise { span, .. }
        | StatementNode::Assert { span, .. }
        | StatementNode::Break { span }
        | StatementNode::Continue { span }
        | StatementNode::Pass { span }
        | StatementNode::Unsupported { span, .. } => *span,
    }
}
