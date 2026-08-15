use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use efct_model::{
    LanguageIdentity, PythonImplementation, SUPPORTED_CPYTHON_MESSAGE, SourceSpan, TrustPolicy,
    supports_cpython_version,
};

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceEnvelope {
    pub protocol_version: u32,
    pub filename: String,
    pub source_sha256: String,
    pub language: SourceLanguage,
}

pub type ProtocolEnvelope = SourceEnvelope;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceLanguage {
    Python {
        implementation: PythonImplementation,
        version: [u8; 3],
        root: ModuleNode,
    },
    TypeScript {
        compiler_version: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectEnvelope {
    pub protocol_version: u32,
    pub language: LanguageIdentity,
    pub root: String,
    pub modules: Vec<ProjectModule>,
    pub policy: TrustPolicy,
    pub external_symbols: Vec<ExternalSymbol>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalSymbol {
    pub path: String,
    pub parameters: Vec<String>,
    pub returns: String,
    pub effects: Vec<String>,
    pub trust: ExternalTrust,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExternalTrust {
    Audited { evidence: String },
    Unsafe { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectModule {
    pub name: String,
    pub envelope: SourceEnvelope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleNode {
    pub kind: ModuleKind,
    pub items: Vec<ModuleItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModuleKind {
    Module,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModuleItem {
    Import {
        names: Vec<ImportAlias>,
        span: SourceSpan,
    },
    ImportFrom {
        module: Option<String>,
        names: Vec<ImportAlias>,
        level: u32,
        span: SourceSpan,
    },
    AnnotatedAssignment {
        target: ExpressionNode,
        annotation: ExpressionNode,
        value: Option<ExpressionNode>,
        simple: bool,
        span: SourceSpan,
    },
    Statement {
        statement: StatementNode,
    },
    Function {
        name: String,
        type_parameters: Vec<TypeParameterNode>,
        parameters: Box<ArgumentsNode>,
        returns: Option<ExpressionNode>,
        decorators: Vec<ExpressionNode>,
        body: Vec<StatementNode>,
        type_comment: Option<String>,
        span: SourceSpan,
    },
    Class {
        name: String,
        bases: Vec<ExpressionNode>,
        keywords: Vec<KeywordNode>,
        decorators: Vec<ExpressionNode>,
        body: Vec<ClassItemNode>,
        span: SourceSpan,
    },
    TypeIgnore {
        tag: String,
        span: SourceSpan,
    },
    Unsupported {
        node: String,
        span: SourceSpan,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TypeParameterNode {
    TypeVariable {
        name: String,
        bound: Option<ExpressionNode>,
        has_default: bool,
        span: SourceSpan,
    },
    Unsupported {
        node: String,
        span: SourceSpan,
    },
}

impl TypeParameterNode {
    #[must_use]
    pub fn span(&self) -> SourceSpan {
        match self {
            Self::TypeVariable { span, .. } | Self::Unsupported { span, .. } => *span,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClassItemNode {
    Field {
        name: String,
        annotation: ExpressionNode,
        has_value: bool,
        span: SourceSpan,
    },
    Docstring {
        span: SourceSpan,
    },
    Pass {
        span: SourceSpan,
    },
    Unsupported {
        node: String,
        span: SourceSpan,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportAlias {
    pub name: String,
    pub alias: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArgumentsNode {
    pub positional_only: Vec<ParameterNode>,
    pub positional: Vec<ParameterNode>,
    pub variable: Option<ParameterNode>,
    pub keyword_only: Vec<ParameterNode>,
    pub keyword_variadic: Option<ParameterNode>,
    pub defaults: Vec<ExpressionNode>,
    pub keyword_defaults: Vec<Option<ExpressionNode>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterNode {
    pub name: String,
    pub annotation: Option<ExpressionNode>,
    pub type_comment: Option<String>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WithItemNode {
    Unbound {
        context: ExpressionNode,
    },
    Bound {
        context: ExpressionNode,
        target: ExpressionNode,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StatementNode {
    Return {
        value: Option<ExpressionNode>,
        span: SourceSpan,
    },
    Assign {
        targets: Vec<ExpressionNode>,
        value: ExpressionNode,
        type_comment: Option<String>,
        span: SourceSpan,
    },
    AnnotatedAssignment {
        target: ExpressionNode,
        annotation: ExpressionNode,
        value: Option<ExpressionNode>,
        simple: bool,
        span: SourceSpan,
    },
    AugmentedAssignment {
        target: ExpressionNode,
        operator: BinaryOperator,
        value: ExpressionNode,
        span: SourceSpan,
    },
    Expression {
        value: ExpressionNode,
        span: SourceSpan,
    },
    If {
        condition: ExpressionNode,
        body: Vec<StatementNode>,
        otherwise: Vec<StatementNode>,
        span: SourceSpan,
    },
    For {
        target: ExpressionNode,
        iterable: ExpressionNode,
        body: Vec<StatementNode>,
        otherwise: Vec<StatementNode>,
        type_comment: Option<String>,
        span: SourceSpan,
    },
    While {
        condition: ExpressionNode,
        body: Vec<StatementNode>,
        otherwise: Vec<StatementNode>,
        span: SourceSpan,
    },
    Match {
        subject: ExpressionNode,
        cases: Vec<MatchCaseNode>,
        span: SourceSpan,
    },
    Try {
        body: Vec<StatementNode>,
        handlers: Vec<ExceptionHandlerNode>,
        otherwise: Vec<StatementNode>,
        finalizer: Vec<StatementNode>,
        span: SourceSpan,
    },
    TryStar {
        body: Vec<StatementNode>,
        handlers: Vec<ExceptionHandlerNode>,
        otherwise: Vec<StatementNode>,
        finalizer: Vec<StatementNode>,
        span: SourceSpan,
    },
    With {
        items: Vec<WithItemNode>,
        body: Vec<StatementNode>,
        span: SourceSpan,
    },
    Raise {
        exception: Option<ExpressionNode>,
        cause: Option<ExpressionNode>,
        span: SourceSpan,
    },
    Assert {
        condition: ExpressionNode,
        message: Option<ExpressionNode>,
        span: SourceSpan,
    },
    Break {
        span: SourceSpan,
    },
    Continue {
        span: SourceSpan,
    },
    Pass {
        span: SourceSpan,
    },
    Unsupported {
        node: String,
        span: SourceSpan,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatchCaseNode {
    pub pattern: PatternNode,
    pub guard: Option<ExpressionNode>,
    pub body: Vec<StatementNode>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PatternNode {
    Class {
        class: ExpressionNode,
        positional: Vec<PatternNode>,
        keyword_attributes: Vec<String>,
        keyword_patterns: Vec<PatternNode>,
        span: SourceSpan,
    },
    Capture {
        name: String,
        span: SourceSpan,
    },
    Wildcard {
        span: SourceSpan,
    },
    As {
        pattern: Box<PatternNode>,
        name: String,
        span: SourceSpan,
    },
    Unsupported {
        node: String,
        span: SourceSpan,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExceptionHandlerNode {
    Typed {
        exception: ExpressionNode,
        body: Vec<StatementNode>,
        span: SourceSpan,
    },
    TypedBinding {
        exception: ExpressionNode,
        binding: String,
        body: Vec<StatementNode>,
        span: SourceSpan,
    },
    Bare {
        body: Vec<StatementNode>,
        span: SourceSpan,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExpressionNode {
    Name {
        identifier: String,
        context: ExpressionContext,
        span: SourceSpan,
    },
    Constant {
        value: ConstantValue,
        span: SourceSpan,
    },
    Tuple {
        elements: Vec<ExpressionNode>,
        context: ExpressionContext,
        span: SourceSpan,
    },
    List {
        elements: Vec<ExpressionNode>,
        context: ExpressionContext,
        span: SourceSpan,
    },
    Unary {
        operator: UnaryOperator,
        operand: Box<ExpressionNode>,
        span: SourceSpan,
    },
    Binary {
        operator: BinaryOperator,
        left: Box<ExpressionNode>,
        right: Box<ExpressionNode>,
        span: SourceSpan,
    },
    Boolean {
        operator: BooleanOperator,
        values: Vec<ExpressionNode>,
        span: SourceSpan,
    },
    Compare {
        left: Box<ExpressionNode>,
        operators: Vec<ComparisonOperator>,
        comparators: Vec<ExpressionNode>,
        span: SourceSpan,
    },
    Conditional {
        condition: Box<ExpressionNode>,
        then_value: Box<ExpressionNode>,
        else_value: Box<ExpressionNode>,
        span: SourceSpan,
    },
    Call {
        callee: Box<ExpressionNode>,
        arguments: Vec<ExpressionNode>,
        keywords: Vec<KeywordNode>,
        span: SourceSpan,
    },
    Attribute {
        value: Box<ExpressionNode>,
        name: String,
        context: ExpressionContext,
        span: SourceSpan,
    },
    Subscript {
        value: Box<ExpressionNode>,
        slice: Box<ExpressionNode>,
        context: ExpressionContext,
        span: SourceSpan,
    },
    Unsupported {
        node: String,
        span: SourceSpan,
    },
}

impl ExpressionNode {
    #[must_use]
    pub fn span(&self) -> SourceSpan {
        match self {
            Self::Name { span, .. }
            | Self::Constant { span, .. }
            | Self::Tuple { span, .. }
            | Self::List { span, .. }
            | Self::Unary { span, .. }
            | Self::Binary { span, .. }
            | Self::Boolean { span, .. }
            | Self::Compare { span, .. }
            | Self::Conditional { span, .. }
            | Self::Call { span, .. }
            | Self::Attribute { span, .. }
            | Self::Subscript { span, .. }
            | Self::Unsupported { span, .. } => *span,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeywordNode {
    pub name: Option<String>,
    pub value: ExpressionNode,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ConstantValue {
    None,
    Bool(bool),
    Int(String),
    Str(String),
    Bytes(String),
    Ellipsis,
    Unsupported(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpressionContext {
    Load,
    Store,
    Delete,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnaryOperator {
    Positive,
    Negative,
    Not,
    Invert,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BinaryOperator {
    Add,
    Subtract,
    Multiply,
    MatrixMultiply,
    TrueDivide,
    FloorDivide,
    Modulo,
    Power,
    LeftShift,
    RightShift,
    BitOr,
    BitXor,
    BitAnd,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BooleanOperator {
    And,
    Or,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonOperator {
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Is,
    IsNot,
    In,
    NotIn,
    Unknown,
}

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("Invalid AST protocol JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("Unsupported AST protocol version {actual}; the current version is {expected}")]
    UnsupportedVersion { actual: u32, expected: u32 },
    #[error("The source SHA-256 has an invalid format")]
    InvalidSourceHash,
}

pub fn decode(payload: &[u8]) -> Result<SourceEnvelope, ProtocolError> {
    let envelope: SourceEnvelope = serde_json::from_slice(payload)?;
    validate_envelope(&envelope)?;
    Ok(envelope)
}

pub fn decode_project(payload: &[u8]) -> Result<ProjectEnvelope, ProtocolError> {
    let project: ProjectEnvelope = serde_json::from_slice(payload)?;
    if project.protocol_version != PROTOCOL_VERSION {
        return Err(ProtocolError::UnsupportedVersion {
            actual: project.protocol_version,
            expected: PROTOCOL_VERSION,
        });
    }
    for module in &project.modules {
        validate_envelope(&module.envelope)?;
        if source_identity(&module.envelope.language) != project.language {
            return Err(ProtocolError::InvalidJson(serde_json::Error::io(
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Language identities in the project do not match",
                ),
            )));
        }
    }
    Ok(project)
}

fn source_identity(source: &SourceLanguage) -> LanguageIdentity {
    match source {
        SourceLanguage::Python {
            implementation,
            version,
            ..
        } => LanguageIdentity::Python {
            implementation: *implementation,
            version: *version,
        },
        SourceLanguage::TypeScript { compiler_version } => LanguageIdentity::TypeScript {
            compiler_version: compiler_version.clone(),
        },
    }
}

fn validate_envelope(envelope: &SourceEnvelope) -> Result<(), ProtocolError> {
    if envelope.protocol_version != PROTOCOL_VERSION {
        return Err(ProtocolError::UnsupportedVersion {
            actual: envelope.protocol_version,
            expected: PROTOCOL_VERSION,
        });
    }
    if envelope.source_sha256.len() != 64
        || !envelope
            .source_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(ProtocolError::InvalidSourceHash);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn rejects_unknown_protocol_fields() {
        let payload = format!(
            r#"{{
                "protocol_version": 1,
                "language": {{"kind": "python", "implementation": "cpython", "version": [3, 14, 4], "root": {{"kind": "module", "items": [], "extra": true}}}},
                "filename": "empty.py",
                "source_sha256": "{HASH}"
            }}"#
        );

        assert!(matches!(
            decode(payload.as_bytes()),
            Err(ProtocolError::InvalidJson(_))
        ));
    }

    #[test]
    fn accepts_an_empty_module() {
        let payload = format!(
            r#"{{
                "protocol_version": 1,
                "language": {{"kind": "python", "implementation": "cpython", "version": [3, 14, 4], "root": {{"kind": "module", "items": []}}}},
                "filename": "empty.py",
                "source_sha256": "{HASH}"
            }}"#
        );

        let envelope = decode(payload.as_bytes()).expect("an empty module envelope must be valid");
        assert!(matches!(
            envelope.language,
            SourceLanguage::Python { root, .. } if root.items.is_empty()
        ));
    }

    #[test]
    fn rejects_an_unsupported_protocol_version() {
        let payload = format!(
            r#"{{
                "protocol_version": 0,
                "language": {{"kind": "python", "implementation": "cpython", "version": [3, 14, 4], "root": {{"kind": "module", "items": []}}}},
                "filename": "empty.py",
                "source_sha256": "{HASH}"
            }}"#
        );

        assert!(matches!(
            decode(payload.as_bytes()),
            Err(ProtocolError::UnsupportedVersion {
                actual: 0,
                expected: 1
            })
        ));
    }

    #[test]
    fn rejects_an_invalid_source_hash() {
        let payload = br#"{
            "protocol_version": 1,
            "language": {"kind": "python", "implementation": "cpython", "version": [3, 14, 4], "root": {"kind": "module", "items": []}},
            "filename": "empty.py",
            "source_sha256": "abc"
        }"#;

        assert!(matches!(
            decode(payload),
            Err(ProtocolError::InvalidSourceHash)
        ));
    }
}
