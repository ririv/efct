use efct_protocol::{ArgumentsNode, ExpressionNode};

use super::{LoweringContext, is_efct_name};
use crate::hir::{EffectParameter, FunctionDeclaration, Import, Parameter};

impl LoweringContext {
    pub(super) fn lower_declaration(
        &mut self,
        decorators: Vec<ExpressionNode>,
        function: &str,
        span: efct_protocol::SourceSpan,
        imports: &[Import],
    ) -> Option<FunctionDeclaration> {
        if decorators.len() != 1 {
            self.error(
                "P1006",
                Some(span),
                Some(function),
                "A function must have exactly one Efct marker",
            );
            return None;
        }
        let decorator = decorators.into_iter().next()?;
        if is_efct_name(&decorator, "pure", imports) {
            return Some(FunctionDeclaration::InferredPure);
        }
        if is_efct_name(&decorator, "effects", imports) {
            return Some(FunctionDeclaration::InferredEffects);
        }
        if let ExpressionNode::Call {
            callee,
            arguments,
            keywords,
            ..
        } = decorator
        {
            if is_efct_name(&callee, "pure", imports) && keywords.is_empty() {
                let partials = self.lower_partial_arguments(arguments, Some(function), imports)?;
                return Some(FunctionDeclaration::BoundedPure(partials));
            }
            if is_efct_name(&callee, "effects", imports) && keywords.is_empty() {
                let effects = self.lower_effect_arguments(arguments, Some(function), imports)?;
                return Some(FunctionDeclaration::BoundedEffects(effects));
            }
        }
        self.error(
            "P1006",
            Some(span),
            Some(function),
            "Only @efct.pure(...), @efct.effects, or @efct.effects(...) markers are allowed",
        );
        None
    }

    pub(super) fn lower_effect_parameters(
        &mut self,
        parameters: Vec<efct_protocol::TypeParameterNode>,
        function: &str,
        imports: &[Import],
    ) -> Option<Vec<EffectParameter>> {
        let mut lowered = Vec::with_capacity(parameters.len());
        let mut names = std::collections::BTreeSet::new();
        for parameter in parameters {
            let efct_protocol::TypeParameterNode::TypeVariable {
                name,
                bound,
                has_default,
                span,
            } = parameter
            else {
                self.error(
                    "P1104",
                    Some(parameter.span()),
                    Some(function),
                    "Only efct.EffectSet effect-generic parameters are supported",
                );
                return None;
            };
            if has_default {
                self.error(
                    "P1104",
                    Some(span),
                    Some(function),
                    "An effect-generic parameter cannot have a default value",
                );
                return None;
            }
            let Some(bound) = bound else {
                self.error(
                    "P1104",
                    Some(span),
                    Some(function),
                    "An effect-generic parameter must be explicitly constrained to efct.EffectSet",
                );
                return None;
            };
            if !is_efct_name(&bound, "EffectSet", imports) {
                self.error(
                    "P1104",
                    Some(bound.span()),
                    Some(function),
                    "Only efct.EffectSet generic parameters are currently supported",
                );
                return None;
            }
            if !names.insert(name.clone()) {
                self.error(
                    "P1104",
                    Some(span),
                    Some(function),
                    format!("Effect-generic parameter {name} is declared more than once"),
                );
                return None;
            }
            lowered.push(EffectParameter { name, span });
        }
        Some(lowered)
    }

    pub(super) fn lower_parameters(
        &mut self,
        parameters: ArgumentsNode,
        function: &str,
        span: efct_protocol::SourceSpan,
    ) -> Option<Vec<Parameter>> {
        if !parameters.positional_only.is_empty()
            || parameters.variable.is_some()
            || !parameters.keyword_only.is_empty()
            || parameters.keyword_variadic.is_some()
            || !parameters.defaults.is_empty()
            || !parameters.keyword_defaults.is_empty()
        {
            self.error(
                "P1106",
                Some(span),
                Some(function),
                "The MVP only allows regular parameters without defaults",
            );
            return None;
        }

        let mut lowered = Vec::with_capacity(parameters.positional.len());
        for parameter in parameters.positional {
            if parameter.type_comment.is_some() {
                self.error(
                    "P1502",
                    Some(parameter.span),
                    Some(function),
                    "Parameter type comments are not allowed",
                );
                return None;
            }
            let annotation = match parameter.annotation {
                Some(expression) => self.lower_expression(expression, Some(function)),
                None => None,
            };
            lowered.push(Parameter {
                name: parameter.name,
                annotation,
                span: parameter.span,
            });
        }
        Some(lowered)
    }
}
