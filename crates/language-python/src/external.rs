use efct_protocol::{ExternalSymbol, ExternalTrust, TrustPolicy};

use crate::types::Type;
use efct_model::{Effect, EffectSet};

#[derive(Debug, Clone)]
pub struct ExternalDefinition {
    pub path: String,
    pub parameters: Vec<Type>,
    pub returns: Type,
    pub effects: EffectSet,
    pub trust: TrustLevel,
}

#[derive(Debug, Clone)]
pub enum TrustLevel {
    Audited(String),
    Unsafe(String),
}

pub fn decode(symbols: Vec<ExternalSymbol>) -> Result<Vec<ExternalDefinition>, (String, String)> {
    let mut definitions = Vec::new();
    for symbol in symbols {
        let parameters = symbol
            .parameters
            .iter()
            .map(|value| parse_type(value))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|message| (symbol.path.clone(), message))?;
        let returns =
            parse_type(&symbol.returns).map_err(|message| (symbol.path.clone(), message))?;
        let mut effects = EffectSet::new();
        for value in symbol.effects {
            let effect =
                Effect::parse(&value).map_err(|error| (symbol.path.clone(), error.to_string()))?;
            if !effects.insert(effect) {
                return Err((
                    symbol.path,
                    format!("External effect {value} is duplicated"),
                ));
            }
        }
        let trust = match symbol.trust {
            ExternalTrust::Audited { evidence } if !evidence.is_empty() => {
                TrustLevel::Audited(evidence)
            }
            ExternalTrust::Unsafe { reason } if !reason.is_empty() => TrustLevel::Unsafe(reason),
            _ => {
                return Err((
                    symbol.path,
                    "External trust evidence cannot be empty".to_owned(),
                ));
            }
        };
        definitions.push(ExternalDefinition {
            path: symbol.path,
            parameters,
            returns,
            effects,
            trust,
        });
    }
    definitions.sort_by(|left, right| left.path.cmp(&right.path));
    if definitions
        .windows(2)
        .any(|pair| pair[0].path == pair[1].path)
    {
        return Err((
            String::new(),
            "The external symbol is duplicated".to_owned(),
        ));
    }
    Ok(definitions)
}

pub fn policy_rejects(policy: TrustPolicy, trust: &TrustLevel) -> bool {
    matches!(policy, TrustPolicy::VerifiedOnly)
        || matches!(
            (policy, trust),
            (TrustPolicy::DenyUnsafe, TrustLevel::Unsafe(_))
        )
}

pub fn parse_signature(value: &str) -> Result<(Vec<String>, String), String> {
    let Some((parameters, returns)) = value.split_once("->") else {
        return Err(format!(
            "An external symbol signature must include a return type: {value}"
        ));
    };
    if returns.contains("->") {
        return Err(format!("Invalid external symbol signature: {value}"));
    }
    let parameters = parameters.trim();
    let Some(parameters) = parameters
        .strip_prefix('(')
        .and_then(|parameters| parameters.strip_suffix(')'))
    else {
        return Err(format!("Invalid external symbol signature: {value}"));
    };
    let parameter_values = if parameters.trim().is_empty() {
        Vec::new()
    } else {
        split_arguments(parameters)?
            .into_iter()
            .map(|parameter| {
                let parameter = parameter.trim();
                let value_type = match parameter.split_once(':') {
                    Some((name, value_type)) if is_identifier(name.trim()) => value_type.trim(),
                    Some(_) => return Err(format!("Invalid external parameter: {parameter}")),
                    None => parameter,
                };
                parse_type(value_type)?;
                Ok(value_type.to_owned())
            })
            .collect::<Result<Vec<_>, String>>()?
    };
    let returns = returns.trim();
    parse_type(returns)?;
    Ok((parameter_values, returns.to_owned()))
}

fn parse_type(value: &str) -> Result<Type, String> {
    let value = value.trim();
    if let Some(element) = optional_element(value)? {
        let element = parse_type(element)?;
        if matches!(element, Type::None | Type::Option(_)) {
            return Err(format!(
                "Optional type {value} must contain exactly one non-None type"
            ));
        }
        return Ok(Type::Option(Box::new(element)));
    }
    match value {
        "None" => return Ok(Type::None),
        "bool" => return Ok(Type::Bool),
        "int" => return Ok(Type::Int),
        "str" => return Ok(Type::Str),
        "bytes" => return Ok(Type::Bytes),
        _ => {}
    }
    if let Some(inner) = value
        .strip_prefix("frozenset[")
        .and_then(|rest| rest.strip_suffix(']'))
    {
        let items = parse_arguments(inner)?;
        if items.len() != 1 {
            return Err(format!("Type {value} requires one argument"));
        }
        return Ok(Type::FrozenSet(Box::new(items.into_iter().next().unwrap())));
    }
    if let Some(inner) = value
        .strip_prefix("tuple[")
        .and_then(|rest| rest.strip_suffix(']'))
    {
        let parts = split_arguments(inner)?;
        if parts.len() == 2 && parts[1] == "..." {
            return Ok(Type::TupleVariadic(Box::new(parse_type(parts[0])?)));
        }
        return parts
            .iter()
            .map(|part| parse_type(part))
            .collect::<Result<Vec<_>, _>>()
            .map(Type::TupleFixed);
    }
    for prefix in ["efct.FrozenMap[", "efct.Result["] {
        if let Some(inner) = value
            .strip_prefix(prefix)
            .and_then(|rest| rest.strip_suffix(']'))
        {
            let mut items = parse_arguments(inner)?;
            if items.len() != 2 {
                return Err(format!("Type {value} requires two arguments"));
            }
            let second = items.pop().unwrap();
            let first = items.pop().unwrap();
            return Ok(if prefix == "efct.FrozenMap[" {
                Type::FrozenMap(Box::new(first), Box::new(second))
            } else {
                Type::Result(Box::new(first), Box::new(second))
            });
        }
    }
    Err(format!(
        "The external signature contains unsupported type {value}"
    ))
}

fn optional_element(value: &str) -> Result<Option<&str>, String> {
    let mut depth = 0_usize;
    let mut separator = None;
    for (index, byte) in value.bytes().enumerate() {
        match byte {
            b'[' => depth += 1,
            b']' => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| "Type brackets are unbalanced".to_owned())?
            }
            b'|' if depth == 0 => match separator {
                Some(_) => {
                    return Err(format!(
                        "Only a union of one type and None is supported: {value}"
                    ));
                }
                None => separator = Some(index),
            },
            _ => {}
        }
    }
    if depth != 0 {
        return Err("Type brackets are unbalanced".to_owned());
    }
    let Some(separator) = separator else {
        return Ok(None);
    };
    let left = value[..separator].trim();
    let right = value[separator + 1..].trim();
    match (left, right) {
        ("None", element) | (element, "None") if !element.is_empty() => Ok(Some(element)),
        _ => Err(format!(
            "Only a union of one type and None is supported: {value}"
        )),
    }
}

fn is_identifier(value: &str) -> bool {
    let mut characters = value.chars();
    characters
        .next()
        .is_some_and(|first| first == '_' || first.is_alphabetic())
        && characters.all(|character| character == '_' || character.is_alphanumeric())
}

fn parse_arguments(value: &str) -> Result<Vec<Type>, String> {
    split_arguments(value)?
        .iter()
        .map(|part| parse_type(part))
        .collect()
}

fn split_arguments(value: &str) -> Result<Vec<&str>, String> {
    let mut result = Vec::new();
    let mut depth = 0_usize;
    let mut start = 0;
    for (index, byte) in value.bytes().enumerate() {
        match byte {
            b'[' => depth += 1,
            b']' => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| "Type brackets are unbalanced".to_owned())?
            }
            b',' if depth == 0 => {
                result.push(value[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    if depth != 0 {
        return Err("Type brackets are unbalanced".to_owned());
    }
    result.push(value[start..].trim());
    if result.iter().any(|part| part.is_empty()) {
        return Err("A type argument cannot be empty".to_owned());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::parse_signature;

    #[test]
    fn parses_named_and_unnamed_external_parameters() {
        assert_eq!(
            parse_signature("(value: tuple[int, int], str) -> bool"),
            Ok((
                vec!["tuple[int, int]".to_owned(), "str".to_owned()],
                "bool".to_owned(),
            ))
        );
        assert_eq!(
            parse_signature("() -> None"),
            Ok((Vec::new(), "None".to_owned()))
        );
        assert_eq!(
            parse_signature("(value: int | None) -> str | None"),
            Ok((vec!["int | None".to_owned()], "str | None".to_owned(),))
        );
    }

    #[test]
    fn rejects_invalid_external_signatures() {
        assert!(parse_signature("int -> int").is_err());
        assert!(parse_signature("(value: list[int]) -> int").is_err());
        assert!(parse_signature("(bad-name: int) -> int").is_err());
    }
}
