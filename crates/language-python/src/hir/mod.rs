use efct_protocol::{
    BinaryOperator, BooleanOperator, ComparisonOperator, ConstantValue, SourceSpan, UnaryOperator,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    pub filename: String,
    pub source_sha256: String,
    pub imports: Vec<Import>,
    pub constants: Vec<ConstantDefinition>,
    pub records: Vec<RecordDefinition>,
    pub exceptions: Vec<ExceptionDefinition>,
    pub functions: Vec<Function>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExceptionDefinition {
    pub name: String,
    pub base: Expression,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordDefinition {
    pub name: String,
    pub fields: Vec<RecordField>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordField {
    pub name: String,
    pub annotation: Expression,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Import {
    Module {
        path: String,
        binding: String,
        span: SourceSpan,
    },
    Symbol {
        module: String,
        name: String,
        binding: String,
        span: SourceSpan,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstantDefinition {
    pub name: String,
    pub annotation: Expression,
    pub value: Expression,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    pub id: FunctionId,
    pub kind: FunctionKind,
    pub name: String,
    pub declaration: FunctionDeclaration,
    pub effect_parameters: Vec<EffectParameter>,
    pub parameters: Vec<Parameter>,
    pub returns: Option<Expression>,
    pub body: Vec<Statement>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionKind {
    Declared,
    ModuleInitializer,
}

pub const MODULE_INITIALIZER_NAME: &str = "<module>";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FunctionId(pub usize);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FunctionDeclaration {
    InferredPure,
    BoundedPure(Vec<DeclarationValue>),
    InferredEffects,
    BoundedEffects(Vec<DeclarationValue>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclarationValue {
    pub name: String,
    pub notation: DeclarationNotation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclarationNotation {
    String,
    Typed,
}

impl FunctionDeclaration {
    #[must_use]
    pub fn is_pure(&self) -> bool {
        matches!(self, Self::InferredPure | Self::BoundedPure(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectParameter {
    pub name: String,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parameter {
    pub name: String,
    pub annotation: Option<Expression>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchCase {
    pub pattern: Pattern,
    pub body: Vec<Statement>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pattern {
    Class {
        class: Expression,
        positional: Vec<Pattern>,
        span: SourceSpan,
    },
    Capture {
        name: String,
        span: SourceSpan,
    },
    Wildcard {
        span: SourceSpan,
    },
}

impl Pattern {
    #[must_use]
    pub fn span(&self) -> SourceSpan {
        match self {
            Self::Class { span, .. } | Self::Capture { span, .. } | Self::Wildcard { span } => {
                *span
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Statement {
    ModuleImport {
        module: String,
        span: SourceSpan,
    },
    Return {
        value: Option<Expression>,
        span: SourceSpan,
    },
    Assign {
        target: Expression,
        value: Expression,
        span: SourceSpan,
    },
    AnnotatedAssignment {
        target: Expression,
        annotation: Expression,
        value: Option<Expression>,
        span: SourceSpan,
    },
    AugmentedAssignment {
        target: Expression,
        operator: BinaryOperator,
        value: Expression,
        span: SourceSpan,
    },
    Expression {
        value: Expression,
        span: SourceSpan,
    },
    If {
        condition: Expression,
        body: Vec<Statement>,
        otherwise: Vec<Statement>,
        span: SourceSpan,
    },
    For {
        target: Expression,
        iterable: Expression,
        body: Vec<Statement>,
        otherwise: Vec<Statement>,
        span: SourceSpan,
    },
    While {
        condition: Expression,
        body: Vec<Statement>,
        otherwise: Vec<Statement>,
        span: SourceSpan,
    },
    Match {
        subject: Expression,
        cases: Vec<MatchCase>,
        span: SourceSpan,
    },
    Try {
        body: Vec<Statement>,
        handlers: ExceptionHandlers,
        otherwise: Vec<Statement>,
        finalizer: Vec<Statement>,
        span: SourceSpan,
    },
    With {
        items: Vec<WithItem>,
        body: Vec<Statement>,
        span: SourceSpan,
    },
    Raise {
        exception: Option<Expression>,
        cause: RaiseCause,
        span: SourceSpan,
    },
    Assert {
        condition: Expression,
        message: Option<Expression>,
        span: SourceSpan,
    },
    Break(SourceSpan),
    Continue(SourceSpan),
    Pass(SourceSpan),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RaiseCause {
    Implicit,
    Suppressed,
    Explicit(Expression),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExceptionHandler {
    pub selector: ExceptionHandlerSelector,
    pub binding: ExceptionHandlerBinding,
    pub body: Vec<Statement>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExceptionHandlers {
    Standard(Vec<ExceptionHandler>),
    Group(Vec<ExceptionHandler>),
}

impl ExceptionHandlers {
    pub fn as_slice(&self) -> &[ExceptionHandler] {
        match self {
            Self::Standard(handlers) | Self::Group(handlers) => handlers,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExceptionHandlerSelector {
    Single(Expression),
    Union {
        first: Expression,
        remaining: Vec<Expression>,
    },
}

impl ExceptionHandlerSelector {
    pub fn parts(&self) -> (&Expression, &[Expression]) {
        match self {
            Self::Single(exception) => (exception, &[]),
            Self::Union { first, remaining } => (first, remaining),
        }
    }

    pub fn parts_mut(&mut self) -> (&mut Expression, &mut [Expression]) {
        match self {
            Self::Single(exception) => (exception, &mut []),
            Self::Union { first, remaining } => (first, remaining),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WithItem {
    Unbound {
        context: Expression,
    },
    Bound {
        context: Expression,
        target: Expression,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExceptionHandlerBinding {
    Unbound,
    Bound(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expression {
    Name {
        identifier: String,
        span: SourceSpan,
    },
    Constant {
        value: ConstantValue,
        span: SourceSpan,
    },
    Tuple {
        elements: Vec<Expression>,
        span: SourceSpan,
    },
    List {
        elements: Vec<Expression>,
        span: SourceSpan,
    },
    Unary {
        operator: UnaryOperator,
        operand: Box<Expression>,
        span: SourceSpan,
    },
    Binary {
        operator: BinaryOperator,
        left: Box<Expression>,
        right: Box<Expression>,
        span: SourceSpan,
    },
    Boolean {
        operator: BooleanOperator,
        values: Vec<Expression>,
        span: SourceSpan,
    },
    Compare {
        left: Box<Expression>,
        operators: Vec<ComparisonOperator>,
        comparators: Vec<Expression>,
        span: SourceSpan,
    },
    Conditional {
        condition: Box<Expression>,
        then_value: Box<Expression>,
        else_value: Box<Expression>,
        span: SourceSpan,
    },
    Call {
        callee: Box<Expression>,
        arguments: Vec<Expression>,
        span: SourceSpan,
    },
    Attribute {
        value: Box<Expression>,
        name: String,
        span: SourceSpan,
    },
    Subscript {
        value: Box<Expression>,
        slice: Box<Expression>,
        span: SourceSpan,
    },
}

impl Expression {
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
            | Self::Subscript { span, .. } => *span,
        }
    }
}
