use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSpan {
    pub start_line: u32,
    pub start_utf8_byte: u32,
    pub end_line: u32,
    pub end_utf8_byte: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EffectTraceFrame {
    pub function: String,
    pub filename: String,
    pub span: SourceSpan,
    pub operation: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    pub code: &'static str,
    pub severity: Severity,
    pub filename: String,
    pub span: Option<SourceSpan>,
    pub function: Option<String>,
    pub message: String,
    pub trace: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub effect_trace: Vec<EffectTraceFrame>,
    pub suggestion: Option<String>,
}

impl Diagnostic {
    pub fn error(
        code: &'static str,
        filename: String,
        span: Option<SourceSpan>,
        function: Option<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            severity: Severity::Error,
            filename,
            span,
            function,
            message: message.into(),
            trace: Vec::new(),
            effect_trace: Vec::new(),
            suggestion: None,
        }
    }

    #[must_use]
    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }
}
