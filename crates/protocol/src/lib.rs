use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use efct_model::{
    LanguageIdentity, NodeRuntimeIdentity, PythonImplementation, SUPPORTED_CPYTHON_MESSAGE,
    SourceSpan, TrustPolicy, TypeScriptCompilerIdentity, supports_cpython_version,
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
    #[serde(rename = "typescript")]
    TypeScript {
        compiler: TypeScriptCompilerIdentity,
        runtime: NodeRuntimeIdentity,
        config_sha256: String,
        root: EcmaModuleNode,
    },
    #[serde(rename = "javascript")]
    JavaScript {
        checker: TypeScriptCompilerIdentity,
        runtime: NodeRuntimeIdentity,
        config_sha256: String,
        root: EcmaModuleNode,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EcmaModuleNode {
    pub items: Vec<EcmaModuleItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EcmaModuleItem {
    Import {
        module: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resolved: Option<String>,
        names: Vec<EcmaImportName>,
        span: Utf16SourceSpan,
    },
    Constant {
        name: String,
        annotation: Option<EcmaTypeNode>,
        value: EcmaExpressionNode,
        span: Utf16SourceSpan,
    },
    ModuleDefinition {
        exports: Vec<String>,
        functions: Vec<EcmaFunctionNode>,
        span: Utf16SourceSpan,
    },
    Unsupported {
        node: String,
        span: Utf16SourceSpan,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EcmaImportName {
    pub imported: String,
    pub local: String,
    pub type_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EcmaFunctionNode {
    pub name: String,
    pub contract: EcmaFunctionContract,
    pub parameters: Vec<EcmaParameterNode>,
    pub returns: EcmaTypeNode,
    pub body: Vec<EcmaStatementNode>,
    pub span: Utf16SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EcmaFunctionContract {
    Pure {
        partial: EcmaPartialContract,
    },
    Effects {
        effects: EcmaEffectContract,
        partial: EcmaPartialContract,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EcmaEffectContract {
    Inferred,
    Explicit { effects: Vec<EcmaExternalEffect> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EcmaExternalEffect {
    Console,
    FileRead,
    FileWrite,
    Network,
    Clock,
    Random,
    Environment,
    Process,
    StateRead,
    StateWrite,
    Unsafe,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EcmaPartialContract {
    Inferred,
    ExplicitEmpty,
    Explicit { behaviors: Vec<EcmaPartialBehavior> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EcmaPartialBehavior {
    Throw,
    Diverge,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EcmaParameterNode {
    pub name: String,
    pub annotation: EcmaTypeNode,
    pub span: Utf16SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EcmaTypeNode {
    Undefined,
    Null,
    Boolean,
    Number,
    BigInt,
    String,
    Void,
    Optional {
        value: Box<EcmaTypeNode>,
        absence: EcmaOptionalAbsence,
    },
    Unsupported {
        node: String,
        span: Utf16SourceSpan,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EcmaOptionalAbsence {
    Null,
    Undefined,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EcmaStatementNode {
    Variable {
        name: String,
        annotation: Option<EcmaTypeNode>,
        value: EcmaExpressionNode,
        span: Utf16SourceSpan,
    },
    Assignment {
        name: String,
        value: EcmaExpressionNode,
        span: Utf16SourceSpan,
    },
    Expression {
        expression: EcmaExpressionNode,
        span: Utf16SourceSpan,
    },
    Return {
        value: Option<EcmaExpressionNode>,
        span: Utf16SourceSpan,
    },
    If {
        condition: EcmaExpressionNode,
        then_body: Vec<EcmaStatementNode>,
        else_body: Vec<EcmaStatementNode>,
        span: Utf16SourceSpan,
    },
    While {
        condition: EcmaExpressionNode,
        body: Vec<EcmaStatementNode>,
        span: Utf16SourceSpan,
    },
    Throw {
        value: EcmaExpressionNode,
        span: Utf16SourceSpan,
    },
    Try {
        body: Vec<EcmaStatementNode>,
        catch_body: Option<Vec<EcmaStatementNode>>,
        finally_body: Option<Vec<EcmaStatementNode>>,
        span: Utf16SourceSpan,
    },
    Unsupported {
        node: String,
        span: Utf16SourceSpan,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EcmaExpressionNode {
    Identifier {
        name: String,
        span: Utf16SourceSpan,
    },
    Undefined {
        span: Utf16SourceSpan,
    },
    Null {
        span: Utf16SourceSpan,
    },
    Boolean {
        value: bool,
        span: Utf16SourceSpan,
    },
    Number {
        text: String,
        span: Utf16SourceSpan,
    },
    BigInt {
        text: String,
        span: Utf16SourceSpan,
    },
    String {
        value: String,
        span: Utf16SourceSpan,
    },
    Unary {
        operator: EcmaUnaryOperator,
        operand: Box<EcmaExpressionNode>,
        span: Utf16SourceSpan,
    },
    Binary {
        left: Box<EcmaExpressionNode>,
        operator: EcmaBinaryOperator,
        right: Box<EcmaExpressionNode>,
        span: Utf16SourceSpan,
    },
    Conditional {
        condition: Box<EcmaExpressionNode>,
        when_true: Box<EcmaExpressionNode>,
        when_false: Box<EcmaExpressionNode>,
        span: Utf16SourceSpan,
    },
    Call {
        target: Vec<String>,
        arguments: Vec<EcmaExpressionNode>,
        span: Utf16SourceSpan,
    },
    Property {
        target: Vec<String>,
        span: Utf16SourceSpan,
    },
    Error {
        constructor: EcmaErrorConstructor,
        message: Option<Box<EcmaExpressionNode>>,
        span: Utf16SourceSpan,
    },
    Unsupported {
        node: String,
        span: Utf16SourceSpan,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EcmaErrorConstructor {
    Error,
    TypeError,
    RangeError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EcmaUnaryOperator {
    Positive,
    Negative,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EcmaBinaryOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    StrictEqual,
    StrictNotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Utf16SourceSpan {
    pub start: u32,
    pub end: u32,
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
    #[error("The TypeScript installation SHA-256 has an invalid format")]
    InvalidCompilerHash,
    #[error("The effective TypeScript configuration SHA-256 has an invalid format")]
    InvalidConfigHash,
    #[error("An ECMAScript UTF-16 source span is invalid")]
    InvalidUtf16Span,
}

pub fn decode(payload: &[u8]) -> Result<SourceEnvelope, ProtocolError> {
    let envelope: SourceEnvelope = serde_json::from_slice(payload)?;
    validate(&envelope)?;
    Ok(envelope)
}

pub fn validate(envelope: &SourceEnvelope) -> Result<(), ProtocolError> {
    validate_envelope(envelope)
}

pub fn decode_project(payload: &[u8]) -> Result<ProjectEnvelope, ProtocolError> {
    let project: ProjectEnvelope = serde_json::from_slice(payload)?;
    validate_project(&project)?;
    Ok(project)
}

pub fn validate_project(project: &ProjectEnvelope) -> Result<(), ProtocolError> {
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
    Ok(())
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
        SourceLanguage::TypeScript {
            compiler, runtime, ..
        } => LanguageIdentity::TypeScript {
            compiler: compiler.clone(),
            runtime: *runtime,
        },
        SourceLanguage::JavaScript {
            checker, runtime, ..
        } => LanguageIdentity::JavaScript {
            checker: checker.clone(),
            runtime: *runtime,
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
    if !is_sha256(&envelope.source_sha256) {
        return Err(ProtocolError::InvalidSourceHash);
    }
    match &envelope.language {
        SourceLanguage::Python { .. } => {}
        SourceLanguage::TypeScript {
            compiler,
            config_sha256,
            root,
            ..
        } => validate_ecma_source(compiler, config_sha256, root)?,
        SourceLanguage::JavaScript {
            checker,
            config_sha256,
            root,
            ..
        } => validate_ecma_source(checker, config_sha256, root)?,
    }
    Ok(())
}

fn validate_ecma_source(
    compiler: &TypeScriptCompilerIdentity,
    config_sha256: &str,
    root: &EcmaModuleNode,
) -> Result<(), ProtocolError> {
    if !is_sha256(&compiler.installation_sha256) {
        return Err(ProtocolError::InvalidCompilerHash);
    }
    if !is_sha256(config_sha256) {
        return Err(ProtocolError::InvalidConfigHash);
    }
    if root.items.iter().any(module_item_has_invalid_span) {
        return Err(ProtocolError::InvalidUtf16Span);
    }
    Ok(())
}

fn module_item_has_invalid_span(item: &EcmaModuleItem) -> bool {
    match item {
        EcmaModuleItem::Import { span, .. } | EcmaModuleItem::Unsupported { span, .. } => {
            invalid_span(*span)
        }
        EcmaModuleItem::Constant {
            annotation,
            value,
            span,
            ..
        } => {
            invalid_span(*span)
                || annotation.as_ref().is_some_and(type_has_invalid_span)
                || expression_has_invalid_span(value)
        }
        EcmaModuleItem::ModuleDefinition {
            functions, span, ..
        } => invalid_span(*span) || functions.iter().any(function_has_invalid_span),
    }
}

fn function_has_invalid_span(function: &EcmaFunctionNode) -> bool {
    invalid_span(function.span)
        || function.parameters.iter().any(|parameter| {
            invalid_span(parameter.span) || type_has_invalid_span(&parameter.annotation)
        })
        || type_has_invalid_span(&function.returns)
        || function.body.iter().any(statement_has_invalid_span)
}

fn type_has_invalid_span(node: &EcmaTypeNode) -> bool {
    match node {
        EcmaTypeNode::Optional { value, .. } => type_has_invalid_span(value),
        EcmaTypeNode::Unsupported { span, .. } => invalid_span(*span),
        _ => false,
    }
}

fn statement_has_invalid_span(statement: &EcmaStatementNode) -> bool {
    match statement {
        EcmaStatementNode::Variable {
            annotation,
            value,
            span,
            ..
        } => {
            invalid_span(*span)
                || annotation.as_ref().is_some_and(type_has_invalid_span)
                || expression_has_invalid_span(value)
        }
        EcmaStatementNode::Assignment { value, span, .. } => {
            invalid_span(*span) || expression_has_invalid_span(value)
        }
        EcmaStatementNode::Expression { expression, span } => {
            invalid_span(*span) || expression_has_invalid_span(expression)
        }
        EcmaStatementNode::Return { value, span } => {
            invalid_span(*span) || value.as_ref().is_some_and(expression_has_invalid_span)
        }
        EcmaStatementNode::If {
            condition,
            then_body,
            else_body,
            span,
        } => {
            invalid_span(*span)
                || expression_has_invalid_span(condition)
                || then_body.iter().any(statement_has_invalid_span)
                || else_body.iter().any(statement_has_invalid_span)
        }
        EcmaStatementNode::While {
            condition,
            body,
            span,
        } => {
            invalid_span(*span)
                || expression_has_invalid_span(condition)
                || body.iter().any(statement_has_invalid_span)
        }
        EcmaStatementNode::Throw { value, span } => {
            invalid_span(*span) || expression_has_invalid_span(value)
        }
        EcmaStatementNode::Try {
            body,
            catch_body,
            finally_body,
            span,
        } => {
            invalid_span(*span)
                || body.iter().any(statement_has_invalid_span)
                || catch_body
                    .as_ref()
                    .is_some_and(|body| body.iter().any(statement_has_invalid_span))
                || finally_body
                    .as_ref()
                    .is_some_and(|body| body.iter().any(statement_has_invalid_span))
        }
        EcmaStatementNode::Unsupported { span, .. } => invalid_span(*span),
    }
}

fn expression_has_invalid_span(expression: &EcmaExpressionNode) -> bool {
    match expression {
        EcmaExpressionNode::Identifier { span, .. }
        | EcmaExpressionNode::Undefined { span }
        | EcmaExpressionNode::Null { span }
        | EcmaExpressionNode::Boolean { span, .. }
        | EcmaExpressionNode::Number { span, .. }
        | EcmaExpressionNode::BigInt { span, .. }
        | EcmaExpressionNode::String { span, .. }
        | EcmaExpressionNode::Unsupported { span, .. } => invalid_span(*span),
        EcmaExpressionNode::Unary { operand, span, .. } => {
            invalid_span(*span) || expression_has_invalid_span(operand)
        }
        EcmaExpressionNode::Binary {
            left, right, span, ..
        } => {
            invalid_span(*span)
                || expression_has_invalid_span(left)
                || expression_has_invalid_span(right)
        }
        EcmaExpressionNode::Conditional {
            condition,
            when_true,
            when_false,
            span,
        } => {
            invalid_span(*span)
                || expression_has_invalid_span(condition)
                || expression_has_invalid_span(when_true)
                || expression_has_invalid_span(when_false)
        }
        EcmaExpressionNode::Call {
            arguments, span, ..
        } => invalid_span(*span) || arguments.iter().any(expression_has_invalid_span),
        EcmaExpressionNode::Property { span, .. } => invalid_span(*span),
        EcmaExpressionNode::Error { message, span, .. } => {
            invalid_span(*span)
                || message
                    .as_ref()
                    .is_some_and(|message| expression_has_invalid_span(message))
        }
    }
}

fn invalid_span(span: Utf16SourceSpan) -> bool {
    span.start > span.end
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
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

    #[test]
    fn accepts_distinct_empty_typescript_and_javascript_envelopes() {
        for (kind, compiler_field, filename) in [
            ("typescript", "compiler", "empty.ts"),
            ("javascript", "checker", "empty.js"),
        ] {
            let payload = format!(
                r#"{{
                    "protocol_version": 1,
                    "language": {{
                        "kind": "{kind}",
                        "{compiler_field}": {{"version": "5.9.3", "installation_sha256": "{HASH}"}},
                        "runtime": {{"version": [24, 19, 0], "node_api_version": 8}},
                        "config_sha256": "{HASH}",
                        "root": {{"items": []}}
                    }},
                    "filename": "{filename}",
                    "source_sha256": "{HASH}"
                }}"#
            );

            let envelope =
                decode(payload.as_bytes()).expect("an empty ECMAScript envelope must be valid");
            assert!(matches!(
                envelope.language,
                SourceLanguage::TypeScript { .. } | SourceLanguage::JavaScript { .. }
            ));
        }
    }

    #[test]
    fn rejects_invalid_ecmascript_identity_hashes_and_spans() {
        let invalid_compiler = format!(
            r#"{{
                "protocol_version": 1,
                "language": {{
                    "kind": "typescript",
                    "compiler": {{"version": "5.9.3", "installation_sha256": "invalid"}},
                    "runtime": {{"version": [24, 19, 0], "node_api_version": 8}},
                    "config_sha256": "{HASH}",
                    "root": {{"items": []}}
                }},
                "filename": "empty.ts",
                "source_sha256": "{HASH}"
            }}"#
        );
        assert!(matches!(
            decode(invalid_compiler.as_bytes()),
            Err(ProtocolError::InvalidCompilerHash)
        ));

        let invalid_span = format!(
            r#"{{
                "protocol_version": 1,
                "language": {{
                    "kind": "javascript",
                    "checker": {{"version": "5.9.3", "installation_sha256": "{HASH}"}},
                    "runtime": {{"version": [24, 19, 0], "node_api_version": 8}},
                    "config_sha256": "{HASH}",
                    "root": {{"items": [{{"kind": "unsupported", "node": "ClassDeclaration", "span": {{"start": 8, "end": 3}}}}]}}
                }},
                "filename": "unsupported.js",
                "source_sha256": "{HASH}"
            }}"#
        );
        assert!(matches!(
            decode(invalid_span.as_bytes()),
            Err(ProtocolError::InvalidUtf16Span)
        ));
    }
}
