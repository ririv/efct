use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub use efct::Effect as ExternalEffect;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ExceptionId(String);

impl ExceptionId {
    pub fn parse(value: &str) -> Result<Self, ExceptionIdError> {
        if is_qualified_identifier(value) {
            Ok(Self(value.to_owned()))
        } else {
            Err(ExceptionIdError(value.to_owned()))
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ExceptionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExceptionIdError(String);

impl fmt::Display for ExceptionIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Invalid exception identifier {}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum PartialBehavior {
    Raise(ExceptionId),
    RaiseGroup(ExceptionId),
    Throw,
    Diverge,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Effect {
    External(ExternalEffect),
    Partial(PartialBehavior),
}

pub type EffectSet = BTreeSet<Effect>;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct EffectVariable {
    pub scope: String,
    pub name: String,
}

impl EffectVariable {
    #[must_use]
    pub fn new(scope: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            scope: scope.into(),
            name: name.into(),
        }
    }
}

impl fmt::Display for EffectVariable {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum EffectTerm {
    Concrete(Effect),
    Variable(EffectVariable),
}

impl fmt::Display for EffectTerm {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Concrete(effect) => write!(formatter, "{effect}"),
            Self::Variable(variable) => write!(formatter, "{variable}"),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct EffectFormula(BTreeSet<EffectTerm>);

impl EffectFormula {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, effect: Effect) -> bool {
        self.0.insert(EffectTerm::Concrete(effect))
    }

    pub fn insert_variable(&mut self, variable: EffectVariable) -> bool {
        self.0.insert(EffectTerm::Variable(variable))
    }

    #[must_use]
    pub fn contains_effect(&self, effect: &Effect) -> bool {
        self.0.contains(&EffectTerm::Concrete(effect.clone()))
    }

    #[must_use]
    pub fn contains_divergence(&self) -> bool {
        self.contains_effect(&Effect::Partial(PartialBehavior::Diverge))
    }

    pub fn iter(&self) -> impl Iterator<Item = &EffectTerm> {
        self.0.iter()
    }

    #[must_use]
    pub fn contains_variable(&self) -> bool {
        self.0
            .iter()
            .any(|term| matches!(term, EffectTerm::Variable(_)))
    }

    pub fn difference<'a>(&'a self, other: &'a Self) -> impl Iterator<Item = &'a EffectTerm> {
        self.0.difference(&other.0)
    }

    #[must_use]
    pub fn without_handled(&self, handled: &EffectSet) -> Self {
        self.0
            .iter()
            .filter(|term| match term {
                EffectTerm::Concrete(effect) => !handled.contains(effect),
                EffectTerm::Variable(_) => true,
            })
            .cloned()
            .collect()
    }

    #[must_use]
    pub fn without_concrete_exceptional_behavior(&self) -> Self {
        self.0
            .iter()
            .filter(|term| {
                matches!(
                    term,
                    EffectTerm::Concrete(
                        Effect::External(_) | Effect::Partial(PartialBehavior::Diverge)
                    ) | EffectTerm::Variable(_)
                )
            })
            .cloned()
            .collect()
    }

    #[must_use]
    pub fn partial_behavior_and_variables(&self) -> Self {
        self.0
            .iter()
            .filter(|term| {
                matches!(
                    term,
                    EffectTerm::Concrete(Effect::Partial(_)) | EffectTerm::Variable(_)
                )
            })
            .cloned()
            .collect()
    }

    #[must_use]
    pub fn substitute(&self, bindings: &BTreeMap<EffectVariable, EffectFormula>) -> Self {
        let mut result = Self::new();
        for term in &self.0 {
            match term {
                EffectTerm::Concrete(effect) => {
                    result.insert(effect.clone());
                }
                EffectTerm::Variable(variable) => {
                    if let Some(formula) = bindings.get(variable) {
                        result.extend(formula.clone());
                    } else {
                        result.insert_variable(variable.clone());
                    }
                }
            }
        }
        result
    }
}

impl IntoIterator for EffectFormula {
    type Item = EffectTerm;
    type IntoIter = std::collections::btree_set::IntoIter<EffectTerm>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl FromIterator<EffectTerm> for EffectFormula {
    fn from_iter<T: IntoIterator<Item = EffectTerm>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl Extend<EffectTerm> for EffectFormula {
    fn extend<T: IntoIterator<Item = EffectTerm>>(&mut self, iter: T) {
        self.0.extend(iter);
    }
}

impl Extend<Effect> for EffectFormula {
    fn extend<T: IntoIterator<Item = Effect>>(&mut self, iter: T) {
        self.0.extend(iter.into_iter().map(EffectTerm::Concrete));
    }
}

impl<const N: usize> From<[Effect; N]> for EffectFormula {
    fn from(value: [Effect; N]) -> Self {
        value.into_iter().map(EffectTerm::Concrete).collect()
    }
}

impl Effect {
    #[must_use]
    pub fn is_partial(&self) -> bool {
        matches!(self, Self::Partial(_))
    }

    #[must_use]
    pub fn raised_exception(&self) -> Option<&ExceptionId> {
        match self {
            Self::Partial(
                PartialBehavior::Raise(exception) | PartialBehavior::RaiseGroup(exception),
            ) => Some(exception),
            Self::Partial(PartialBehavior::Throw | PartialBehavior::Diverge)
            | Self::External(_) => None,
        }
    }

    pub fn parse(value: &str) -> Result<Self, EffectParseError> {
        match value {
            "throw" => Ok(Self::Partial(PartialBehavior::Throw)),
            "diverge" => Ok(Self::Partial(PartialBehavior::Diverge)),
            _ if value.starts_with("raise-group:") => {
                let exception = value
                    .strip_prefix("raise-group:")
                    .expect("checked raise-group prefix");
                let exception = ExceptionId::parse(exception)
                    .map_err(|_| EffectParseError::InvalidException(value.to_owned()))?;
                Ok(Self::Partial(PartialBehavior::RaiseGroup(exception)))
            }
            _ if value.starts_with("raise:") => {
                let exception = value.strip_prefix("raise:").expect("checked raise prefix");
                let exception = ExceptionId::parse(exception)
                    .map_err(|_| EffectParseError::InvalidException(value.to_owned()))?;
                Ok(Self::Partial(PartialBehavior::Raise(exception)))
            }
            _ => value
                .parse::<ExternalEffect>()
                .map(Self::External)
                .map_err(|_| EffectParseError::Unknown(value.to_owned())),
        }
    }
}

impl From<ExternalEffect> for Effect {
    fn from(value: ExternalEffect) -> Self {
        Self::External(value)
    }
}

impl From<PartialBehavior> for Effect {
    fn from(value: PartialBehavior) -> Self {
        Self::Partial(value)
    }
}

impl fmt::Display for Effect {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::External(effect) => write!(formatter, "{effect}"),
            Self::Partial(partial) => write!(formatter, "{partial}"),
        }
    }
}

impl fmt::Display for PartialBehavior {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Raise(exception) => write!(formatter, "raise:{exception}"),
            Self::RaiseGroup(exception) => write!(formatter, "raise-group:{exception}"),
            Self::Throw => formatter.write_str("throw"),
            Self::Diverge => formatter.write_str("diverge"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectParseError {
    Unknown(String),
    InvalidException(String),
}

impl fmt::Display for EffectParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(value) => write!(formatter, "Unknown effect name {value}"),
            Self::InvalidException(value) => {
                write!(formatter, "Invalid exception effect name {value}")
            }
        }
    }
}

fn is_qualified_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.split('.').all(|segment| {
            let mut characters = segment.chars();
            characters
                .next()
                .is_some_and(|first| first == '_' || first.is_alphabetic())
                && characters.all(|character| character == '_' || character.is_alphanumeric())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_closed_effect_names() {
        assert_eq!(
            Effect::parse("console"),
            Ok(Effect::External(ExternalEffect::Console))
        );
        assert_eq!(
            Effect::parse("raise:builtins.ValueError"),
            Ok(Effect::Partial(PartialBehavior::Raise(
                ExceptionId::parse("builtins.ValueError").unwrap()
            )))
        );
        assert_eq!(
            Effect::parse("raise-group:builtins.ValueError"),
            Ok(Effect::Partial(PartialBehavior::RaiseGroup(
                ExceptionId::parse("builtins.ValueError").unwrap()
            )))
        );
        assert_eq!(
            Effect::parse("throw"),
            Ok(Effect::Partial(PartialBehavior::Throw))
        );
        assert_eq!(
            Effect::parse("diverge"),
            Ok(Effect::Partial(PartialBehavior::Diverge))
        );
        assert!(Effect::parse("mutation").is_err());
        assert!(Effect::parse("raise:bad-name").is_err());
    }

    #[test]
    fn projects_partial_behavior_and_variables_without_external_effects() {
        let raised = Effect::Partial(PartialBehavior::Raise(
            ExceptionId::parse("builtins.ValueError").unwrap(),
        ));
        let variable = EffectVariable::new("apply", "E");
        let mut formula = EffectFormula::from([Effect::External(ExternalEffect::Console)]);
        formula.insert(raised.clone());
        formula.insert_variable(variable.clone());

        let projected = formula.partial_behavior_and_variables();
        let variable_term = EffectTerm::Variable(variable);

        assert!(projected.contains_effect(&raised));
        assert!(projected.iter().any(|term| term == &variable_term));
        assert!(!projected.contains_effect(&Effect::External(ExternalEffect::Console)));
    }
}
