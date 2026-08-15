use efct_protocol::{
    ConstantValue, ExceptionHandlerNode, MatchCaseNode, PatternNode, StatementNode,
};

use super::LoweringContext;
use crate::hir::{
    ExceptionHandler, ExceptionHandlerBinding, ExceptionHandlerSelector, ExceptionHandlers,
    MatchCase, Pattern, RaiseCause, Statement, WithItem,
};

impl LoweringContext {
    pub(super) fn lower_statements(
        &mut self,
        statements: Vec<StatementNode>,
        function: &str,
    ) -> Option<Vec<Statement>> {
        let mut lowered = Vec::with_capacity(statements.len());
        for statement in statements {
            let statement = self.lower_statement(statement, function)?;
            lowered.push(statement);
        }
        Some(lowered)
    }

    fn lower_statement(&mut self, statement: StatementNode, function: &str) -> Option<Statement> {
        let function_name = Some(function);
        match statement {
            StatementNode::Return { value, span } => Some(Statement::Return {
                value: value.and_then(|value| self.lower_expression(value, function_name)),
                span,
            }),
            StatementNode::Assign {
                targets,
                value,
                type_comment,
                span,
            } => {
                if type_comment.is_some() {
                    self.error(
                        "P1502",
                        Some(span),
                        function_name,
                        "Assignment type comments are not allowed",
                    );
                    return None;
                }
                if targets.len() != 1 {
                    self.error(
                        "P1401",
                        Some(span),
                        function_name,
                        "Chained assignment is not supported in the MVP",
                    );
                    return None;
                }
                Some(Statement::Assign {
                    target: self.lower_expression(targets.into_iter().next()?, function_name)?,
                    value: self.lower_expression(value, function_name)?,
                    span,
                })
            }
            StatementNode::AnnotatedAssignment {
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
                        function_name,
                        "A local typed-assignment target must be a simple name",
                    );
                    return None;
                }
                Some(Statement::AnnotatedAssignment {
                    target: self.lower_expression(target, function_name)?,
                    annotation: self.lower_expression(annotation, function_name)?,
                    value: value.and_then(|value| self.lower_expression(value, function_name)),
                    span,
                })
            }
            StatementNode::AugmentedAssignment {
                target,
                operator,
                value,
                span,
            } => Some(Statement::AugmentedAssignment {
                target: self.lower_expression(target, function_name)?,
                operator,
                value: self.lower_expression(value, function_name)?,
                span,
            }),
            StatementNode::Expression { value, span } => Some(Statement::Expression {
                value: self.lower_expression(value, function_name)?,
                span,
            }),
            StatementNode::If {
                condition,
                body,
                otherwise,
                span,
            } => Some(Statement::If {
                condition: self.lower_expression(condition, function_name)?,
                body: self.lower_statements(body, function)?,
                otherwise: self.lower_statements(otherwise, function)?,
                span,
            }),
            StatementNode::For {
                target,
                iterable,
                body,
                otherwise,
                type_comment,
                span,
            } => {
                if type_comment.is_some() {
                    self.error(
                        "P1502",
                        Some(span),
                        function_name,
                        "Loop type comments are not allowed",
                    );
                    return None;
                }
                Some(Statement::For {
                    target: self.lower_expression(target, function_name)?,
                    iterable: self.lower_expression(iterable, function_name)?,
                    body: self.lower_statements(body, function)?,
                    otherwise: self.lower_statements(otherwise, function)?,
                    span,
                })
            }
            StatementNode::While {
                condition,
                body,
                otherwise,
                span,
            } => Some(Statement::While {
                condition: self.lower_expression(condition, function_name)?,
                body: self.lower_statements(body, function)?,
                otherwise: self.lower_statements(otherwise, function)?,
                span,
            }),
            StatementNode::Match {
                subject,
                cases,
                span,
            } => {
                if cases.is_empty() {
                    self.error(
                        "P1401",
                        Some(span),
                        function_name,
                        "A match statement must contain at least one case",
                    );
                    return None;
                }
                Some(Statement::Match {
                    subject: self.lower_expression(subject, function_name)?,
                    cases: cases
                        .into_iter()
                        .map(|case| self.lower_match_case(case, function))
                        .collect::<Option<Vec<_>>>()?,
                    span,
                })
            }
            StatementNode::Try {
                body,
                handlers,
                otherwise,
                finalizer,
                span,
            } => {
                if handlers.is_empty() && finalizer.is_empty() {
                    self.error(
                        "P1401",
                        Some(span),
                        function_name,
                        "A try statement must contain an exception handler or finally block",
                    );
                    return None;
                }
                let handlers = handlers
                    .into_iter()
                    .map(|handler| self.lower_exception_handler(handler, function))
                    .collect::<Option<Vec<_>>>()?;
                Some(Statement::Try {
                    body: self.lower_statements(body, function)?,
                    handlers: ExceptionHandlers::Standard(handlers),
                    otherwise: self.lower_statements(otherwise, function)?,
                    finalizer: self.lower_statements(finalizer, function)?,
                    span,
                })
            }
            StatementNode::TryStar {
                body,
                handlers,
                otherwise,
                finalizer,
                span,
            } => {
                if handlers.is_empty() {
                    self.error(
                        "P1401",
                        Some(span),
                        function_name,
                        "A try statement with except* must contain at least one handler",
                    );
                    return None;
                }
                let handlers = handlers
                    .into_iter()
                    .map(|handler| self.lower_exception_handler(handler, function))
                    .collect::<Option<Vec<_>>>()?;
                Some(Statement::Try {
                    body: self.lower_statements(body, function)?,
                    handlers: ExceptionHandlers::Group(handlers),
                    otherwise: self.lower_statements(otherwise, function)?,
                    finalizer: self.lower_statements(finalizer, function)?,
                    span,
                })
            }
            StatementNode::With { items, body, span } => {
                if items.is_empty() {
                    self.error(
                        "P1401",
                        Some(span),
                        function_name,
                        "A with statement must contain at least one context manager",
                    );
                    return None;
                }
                Some(Statement::With {
                    items: items
                        .into_iter()
                        .map(|item| match item {
                            efct_protocol::WithItemNode::Unbound { context } => {
                                Some(WithItem::Unbound {
                                    context: self.lower_expression(context, function_name)?,
                                })
                            }
                            efct_protocol::WithItemNode::Bound { context, target } => {
                                Some(WithItem::Bound {
                                    context: self.lower_expression(context, function_name)?,
                                    target: self.lower_expression(target, function_name)?,
                                })
                            }
                        })
                        .collect::<Option<Vec<_>>>()?,
                    body: self.lower_statements(body, function)?,
                    span,
                })
            }
            StatementNode::Raise {
                exception,
                cause,
                span,
            } => Some(Statement::Raise {
                exception: exception
                    .and_then(|expression| self.lower_expression(expression, function_name)),
                cause: match cause {
                    None => RaiseCause::Implicit,
                    Some(expression) => {
                        let expression = self.lower_expression(expression, function_name)?;
                        if matches!(
                            expression,
                            crate::hir::Expression::Constant {
                                value: ConstantValue::None,
                                ..
                            }
                        ) {
                            RaiseCause::Suppressed
                        } else {
                            RaiseCause::Explicit(expression)
                        }
                    }
                },
                span,
            }),
            StatementNode::Assert {
                condition,
                message,
                span,
            } => Some(Statement::Assert {
                condition: self.lower_expression(condition, function_name)?,
                message: match message {
                    Some(expression) => Some(self.lower_expression(expression, function_name)?),
                    None => None,
                },
                span,
            }),
            StatementNode::Break { span } => Some(Statement::Break(span)),
            StatementNode::Continue { span } => Some(Statement::Continue(span)),
            StatementNode::Pass { span } => Some(Statement::Pass(span)),
            StatementNode::Unsupported { node, span } => {
                self.error(
                    "P1401",
                    Some(span),
                    function_name,
                    format!("Python syntax node {node} is not supported"),
                );
                None
            }
        }
    }

    fn lower_match_case(&mut self, case: MatchCaseNode, function: &str) -> Option<MatchCase> {
        if case.guard.is_some() {
            self.error(
                "P1401",
                Some(case.span),
                Some(function),
                "Match guards are not currently supported",
            );
            return None;
        }
        Some(MatchCase {
            pattern: self.lower_pattern(case.pattern, function)?,
            body: self.lower_statements(case.body, function)?,
            span: case.span,
        })
    }

    fn lower_pattern(&mut self, pattern: PatternNode, function: &str) -> Option<Pattern> {
        match pattern {
            PatternNode::Class {
                class,
                positional,
                keyword_attributes,
                keyword_patterns,
                span,
            } => {
                if !keyword_attributes.is_empty() || !keyword_patterns.is_empty() {
                    self.error(
                        "P1401",
                        Some(span),
                        Some(function),
                        "Keyword class patterns are not currently supported",
                    );
                    return None;
                }
                Some(Pattern::Class {
                    class: self.lower_expression(class, Some(function))?,
                    positional: positional
                        .into_iter()
                        .map(|pattern| self.lower_pattern(pattern, function))
                        .collect::<Option<Vec<_>>>()?,
                    span,
                })
            }
            PatternNode::Capture { name, span } => Some(Pattern::Capture { name, span }),
            PatternNode::Wildcard { span } => Some(Pattern::Wildcard { span }),
            PatternNode::As { span, .. } => {
                self.error(
                    "P1401",
                    Some(span),
                    Some(function),
                    "An as-pattern is not currently supported",
                );
                None
            }
            PatternNode::Unsupported { node, span } => {
                self.error(
                    "P1401",
                    Some(span),
                    Some(function),
                    format!("Python pattern node {node} is not supported"),
                );
                None
            }
        }
    }

    fn lower_exception_handler(
        &mut self,
        handler: ExceptionHandlerNode,
        function: &str,
    ) -> Option<ExceptionHandler> {
        let function_name = Some(function);
        match handler {
            ExceptionHandlerNode::Typed {
                exception,
                body,
                span,
            } => Some(ExceptionHandler {
                selector: self.lower_exception_handler_selector(exception, function)?,
                binding: ExceptionHandlerBinding::Unbound,
                body: self.lower_statements(body, function)?,
                span,
            }),
            ExceptionHandlerNode::TypedBinding {
                exception,
                binding,
                body,
                span,
            } => Some(ExceptionHandler {
                selector: self.lower_exception_handler_selector(exception, function)?,
                binding: ExceptionHandlerBinding::Bound(binding),
                body: self.lower_statements(body, function)?,
                span,
            }),
            ExceptionHandlerNode::Bare { span, .. } => {
                self.error(
                    "P1401",
                    Some(span),
                    function_name,
                    "A bare except is not allowed; declare an exception type explicitly",
                );
                None
            }
        }
    }

    fn lower_exception_handler_selector(
        &mut self,
        exception: efct_protocol::ExpressionNode,
        function: &str,
    ) -> Option<ExceptionHandlerSelector> {
        let function_name = Some(function);
        match exception {
            efct_protocol::ExpressionNode::Tuple { elements, span, .. } => {
                let mut elements = elements.into_iter();
                let Some(first) = elements.next() else {
                    self.error(
                        "P1401",
                        Some(span),
                        function_name,
                        "An exception handler type tuple must not be empty",
                    );
                    return None;
                };
                Some(ExceptionHandlerSelector::Union {
                    first: self.lower_expression(first, function_name)?,
                    remaining: elements
                        .map(|element| self.lower_expression(element, function_name))
                        .collect::<Option<Vec<_>>>()?,
                })
            }
            exception => Some(ExceptionHandlerSelector::Single(
                self.lower_expression(exception, function_name)?,
            )),
        }
    }
}
