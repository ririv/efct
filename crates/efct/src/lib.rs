//! Rust types for describing external effects with Efct.
//!
//! Recoverable Rust failures belong in `Result<T, E>` and are intentionally not
//! represented as effects. Python exceptions and ECMAScript throws are part of
//! their language-specific analyzer models rather than this Rust API.
//!
//! Construct [`Effect`] values directly in Rust code. String parsing exists only
//! for configuration and interchange boundaries.

use std::fmt;
use std::str::FromStr;

/// An externally observable capability used by a computation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Effect {
    Console,
    FileRead,
    FileWrite,
    Network,
    Clock,
    Random,
    Environment,
    Process,
    GlobalRead,
    GlobalWrite,
    Unsafe,
}

impl Effect {
    /// Returns the stable interchange name of the effect.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Console => "console",
            Self::FileRead => "file.read",
            Self::FileWrite => "file.write",
            Self::Network => "network",
            Self::Clock => "clock",
            Self::Random => "random",
            Self::Environment => "environment",
            Self::Process => "process",
            Self::GlobalRead => "global.read",
            Self::GlobalWrite => "global.write",
            Self::Unsafe => "unsafe",
        }
    }
}

impl fmt::Display for Effect {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for Effect {
    type Err = ParseEffectError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "console" => Ok(Self::Console),
            "file.read" => Ok(Self::FileRead),
            "file.write" => Ok(Self::FileWrite),
            "network" => Ok(Self::Network),
            "clock" => Ok(Self::Clock),
            "random" => Ok(Self::Random),
            "environment" => Ok(Self::Environment),
            "process" => Ok(Self::Process),
            "global.read" => Ok(Self::GlobalRead),
            "global.write" => Ok(Self::GlobalWrite),
            "unsafe" => Ok(Self::Unsafe),
            _ => Err(ParseEffectError {
                value: value.to_owned(),
            }),
        }
    }
}

/// Returned when a stable effect name is not recognized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseEffectError {
    value: String,
}

impl ParseEffectError {
    /// Returns the rejected input.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

impl fmt::Display for ParseEffectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "unknown effect name {}", self.value)
    }
}

impl std::error::Error for ParseEffectError {}

#[cfg(test)]
mod tests {
    use super::*;

    const EFFECTS: [Effect; 11] = [
        Effect::Console,
        Effect::FileRead,
        Effect::FileWrite,
        Effect::Network,
        Effect::Clock,
        Effect::Random,
        Effect::Environment,
        Effect::Process,
        Effect::GlobalRead,
        Effect::GlobalWrite,
        Effect::Unsafe,
    ];

    #[test]
    fn stable_names_round_trip() {
        for effect in EFFECTS {
            assert_eq!(effect.as_str().parse(), Ok(effect));
            assert_eq!(effect.to_string(), effect.as_str());
        }
    }

    #[test]
    fn unknown_names_return_the_rejected_input() {
        let value = "not.an.effect";
        let error = value.parse::<Effect>().unwrap_err();
        assert_eq!(error.value(), value);
    }
}
