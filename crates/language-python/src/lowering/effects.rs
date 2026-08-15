use efct_protocol::{ConstantValue, ExpressionNode};

use crate::hir::{DeclarationNotation, DeclarationValue};

use super::LoweringContext;

#[derive(Clone, Copy, PartialEq, Eq)]
enum DeclarationKind {
    External,
    Partial,
}

impl LoweringContext {
    pub(super) fn lower_effect_arguments(
        &mut self,
        arguments: Vec<ExpressionNode>,
        function: Option<&str>,
    ) -> Option<Vec<DeclarationValue>> {
        let mut effects = Vec::with_capacity(arguments.len());
        let mut notation = None;
        for argument in arguments {
            let span = argument.span();
            let Some((effect, current_notation, _)) = lower_declaration_argument(&argument) else {
                self.error(
                    "P1006",
                    Some(span),
                    function,
                    "An effect declaration must be a string literal or a supported efct.effect or efct.partial constructor",
                );
                return None;
            };
            if notation.is_some_and(|previous| previous != current_notation) {
                self.error(
                    "P1006",
                    Some(span),
                    function,
                    "String and typed effect declarations cannot be mixed",
                );
                return None;
            }
            notation = Some(current_notation);
            effects.push(DeclarationValue {
                name: effect,
                notation: current_notation,
            });
        }
        Some(effects)
    }

    pub(super) fn lower_partial_arguments(
        &mut self,
        arguments: Vec<ExpressionNode>,
        function: Option<&str>,
    ) -> Option<Vec<DeclarationValue>> {
        let mut partials = Vec::with_capacity(arguments.len());
        let mut notation = None;
        for argument in arguments {
            let span = argument.span();
            let Some((partial, current_notation, kind)) = lower_declaration_argument(&argument)
            else {
                self.error(
                    "P1006",
                    Some(span),
                    function,
                    "A partial declaration must be a supported string or efct.partial constructor",
                );
                return None;
            };
            if kind != DeclarationKind::Partial {
                self.error(
                    "P1006",
                    Some(span),
                    function,
                    "A pure contract may only declare partial behavior",
                );
                return None;
            }
            if notation.is_some_and(|previous| previous != current_notation) {
                self.error(
                    "P1006",
                    Some(span),
                    function,
                    "String and typed partial declarations cannot be mixed",
                );
                return None;
            }
            notation = Some(current_notation);
            partials.push(DeclarationValue {
                name: partial,
                notation: current_notation,
            });
        }
        Some(partials)
    }
}

fn lower_declaration_argument(
    argument: &ExpressionNode,
) -> Option<(String, DeclarationNotation, DeclarationKind)> {
    if let ExpressionNode::Constant {
        value: ConstantValue::Str(effect),
        ..
    } = argument
    {
        let kind = if effect == "diverge"
            || effect.starts_with("raise:")
            || effect.starts_with("raise-group:")
        {
            DeclarationKind::Partial
        } else {
            DeclarationKind::External
        };
        return Some((effect.clone(), DeclarationNotation::String, kind));
    }

    let ExpressionNode::Call {
        callee,
        arguments,
        keywords,
        ..
    } = argument
    else {
        return None;
    };
    if !keywords.is_empty() {
        return None;
    }

    let qualified_callee = qualified_name(callee)?;
    let external_constructor = qualified_callee
        .strip_prefix("efct.effect.")
        .or_else(|| qualified_callee.strip_prefix("effect."));
    let unit_name = match external_constructor {
        Some("Console") => Some("console"),
        Some("File.Read") => Some("file.read"),
        Some("File.Write") => Some("file.write"),
        Some("Network") => Some("network"),
        Some("Clock") => Some("clock"),
        Some("Random") => Some("random"),
        Some("Environment") => Some("environment"),
        Some("Process") => Some("process"),
        Some("State.Read") => Some("global.read"),
        Some("State.Write") => Some("global.write"),
        Some("Unsafe") => Some("unsafe"),
        _ => None,
    };
    if let Some(name) = unit_name {
        return arguments.is_empty().then(|| {
            (
                name.to_owned(),
                DeclarationNotation::Typed,
                DeclarationKind::External,
            )
        });
    }

    let partial_constructor = qualified_callee
        .strip_prefix("efct.partial.")
        .or_else(|| qualified_callee.strip_prefix("partial."));
    if partial_constructor == Some("Diverge") && arguments.is_empty() {
        return Some((
            "diverge".to_owned(),
            DeclarationNotation::Typed,
            DeclarationKind::Partial,
        ));
    }
    if matches!(partial_constructor, Some("Raise" | "RaiseGroup")) && arguments.len() == 1 {
        let exception = qualified_name(&arguments[0])?;
        let qualified_exception = crate::exceptions::resolve_builtin_exception(&exception)
            .map_or(exception, |exception| exception.to_string());
        let prefix = if partial_constructor == Some("RaiseGroup") {
            "raise-group:"
        } else {
            "raise:"
        };
        return Some((
            format!("{prefix}{qualified_exception}"),
            DeclarationNotation::Typed,
            DeclarationKind::Partial,
        ));
    }
    None
}

/*
The exhaustive constructor list above intentionally keeps the public declaration
language closed. New effects and partial behaviors must be added as explicit variants.
*/

fn qualified_name(expression: &ExpressionNode) -> Option<String> {
    match expression {
        ExpressionNode::Name { identifier, .. } => Some(identifier.clone()),
        ExpressionNode::Attribute { value, name, .. } => {
            Some(format!("{}.{}", qualified_name(value)?, name))
        }
        _ => None,
    }
}
