use std::ffi::CString;

use efct_protocol::{
    ArgumentsNode, BinaryOperator, BooleanOperator, ClassItemNode, ComparisonOperator,
    ConstantValue, ExceptionHandlerNode, ExpressionContext, ExpressionNode, ImportAlias,
    KeywordNode, MatchCaseNode, ModuleItem, ModuleKind, ModuleNode, PROTOCOL_VERSION,
    ParameterNode, PatternNode, ProtocolEnvelope, PythonImplementation, SourceLanguage, SourceSpan,
    StatementNode, TypeParameterNode, UnaryOperator, WithItemNode,
};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::FromPyObjectOwned;
use pyo3::types::{PyAnyMethods, PyString, PyStringMethods, PyTypeMethods};
use pyo3::{Bound, PyAny, PyResult, Python, ffi};

const PY_CF_SOURCE_IS_UTF8: i32 = 0x0100;
const PY_CF_ONLY_AST: i32 = 0x0400;
const PY_CF_IGNORE_COOKIE: i32 = 0x0800;
const PY_CF_TYPE_COMMENTS: i32 = 0x1000;

pub struct ParsedSource {
    pub envelope: ProtocolEnvelope,
}

pub fn supports_runtime(py: Python<'_>) -> PyResult<bool> {
    let implementation = py
        .import("sys")?
        .getattr("implementation")?
        .getattr("name")?
        .extract::<String>()?;
    let version = py.version_info();
    Ok(implementation == "cpython"
        && efct_protocol::supports_cpython_version([version.major, version.minor, version.patch]))
}

fn require_supported_runtime(py: Python<'_>) -> PyResult<()> {
    if !supports_runtime(py)? {
        return Err(PyValueError::new_err(
            efct_protocol::SUPPORTED_CPYTHON_MESSAGE,
        ));
    }
    Ok(())
}

pub fn parse_source(
    py: Python<'_>,
    source: &str,
    filename: &str,
    source_sha256: String,
) -> PyResult<ParsedSource> {
    require_supported_runtime(py)?;
    if source_sha256.len() != 64 || !source_sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(PyValueError::new_err(
            "The source SHA-256 has an invalid format",
        ));
    }
    let source = CString::new(source)
        .map_err(|_| PyValueError::new_err("Python source cannot contain null bytes"))?;
    let filename_object = PyString::new(py, filename);
    let version = py.version_info();
    let mut flags = ffi::PyCompilerFlags {
        cf_flags: PY_CF_SOURCE_IS_UTF8 | PY_CF_ONLY_AST | PY_CF_IGNORE_COOKIE | PY_CF_TYPE_COMMENTS,
        cf_feature_version: i32::from(version.minor),
    };
    let pointer = unsafe {
        ffi::Py_CompileStringObject(
            source.as_ptr(),
            filename_object.as_ptr(),
            ffi::Py_file_input,
            &mut flags,
            -1,
        )
    };
    let tree = unsafe { Bound::from_owned_ptr_or_err(py, pointer)? };
    let envelope = envelope_from_tree(py, &tree, filename, source_sha256)?;
    Ok(ParsedSource { envelope })
}

fn envelope_from_tree(
    py: Python<'_>,
    tree: &Bound<'_, PyAny>,
    filename: &str,
    source_sha256: String,
) -> PyResult<ProtocolEnvelope> {
    if source_sha256.len() != 64 || !source_sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(PyValueError::new_err(
            "The source SHA-256 has an invalid format",
        ));
    }
    let version = py.version_info();
    Ok(ProtocolEnvelope {
        protocol_version: PROTOCOL_VERSION,
        filename: filename.to_owned(),
        source_sha256,
        language: SourceLanguage::Python {
            implementation: PythonImplementation::Cpython,
            version: [version.major, version.minor, version.patch],
            root: module(tree)?,
        },
    })
}

fn module(node: &Bound<'_, PyAny>) -> PyResult<ModuleNode> {
    if kind(node)? != "Module" {
        return Err(PyValueError::new_err(
            "The CPython parse result is not a module AST",
        ));
    }
    let mut items = collect_attr(node, "body", module_item)?;
    items.extend(collect_attr(node, "type_ignores", type_ignore)?);
    Ok(ModuleNode {
        kind: ModuleKind::Module,
        items,
    })
}

fn type_ignore(node: &Bound<'_, PyAny>) -> PyResult<ModuleItem> {
    let line = attr::<u32>(node, "lineno")?;
    Ok(ModuleItem::TypeIgnore {
        tag: attr(node, "tag")?,
        span: SourceSpan {
            start_line: line,
            start_utf8_byte: 0,
            end_line: line,
            end_utf8_byte: 0,
        },
    })
}

fn module_item(node: &Bound<'_, PyAny>) -> PyResult<ModuleItem> {
    let span = span(node)?;
    Ok(match kind(node)?.as_str() {
        "Import" => ModuleItem::Import {
            names: collect_attr(node, "names", import_alias)?,
            span,
        },
        "ImportFrom" => ModuleItem::ImportFrom {
            module: attr(node, "module")?,
            names: collect_attr(node, "names", import_alias)?,
            level: attr(node, "level")?,
            span,
        },
        "AnnAssign" => ModuleItem::AnnotatedAssignment {
            target: expression_attr(node, "target")?,
            annotation: expression_attr(node, "annotation")?,
            value: optional_expression_attr(node, "value")?,
            simple: attr::<u8>(node, "simple")? != 0,
            span,
        },
        "Assign" | "TypeAlias" | "AugAssign" | "Expr" | "If" | "For" | "AsyncFor" | "While"
        | "With" | "AsyncWith" | "Match" | "Raise" | "Try" | "TryStar" | "Assert" | "Global"
        | "Nonlocal" | "Pass" | "Break" | "Continue" => ModuleItem::Statement {
            statement: statement(node)?,
        },
        "FunctionDef" => ModuleItem::Function {
            name: attr(node, "name")?,
            type_parameters: collect_attr(node, "type_params", type_parameter)?,
            parameters: Box::new(arguments(&node.getattr("args")?)?),
            returns: optional_expression_attr(node, "returns")?,
            decorators: collect_attr(node, "decorator_list", expression)?,
            body: collect_attr(node, "body", statement)?,
            type_comment: attr(node, "type_comment")?,
            span,
        },
        "ClassDef" => ModuleItem::Class {
            name: attr(node, "name")?,
            bases: collect_attr(node, "bases", expression)?,
            keywords: collect_attr(node, "keywords", keyword)?,
            decorators: collect_attr(node, "decorator_list", expression)?,
            body: collect_attr(node, "body", class_item)?,
            span,
        },
        other => ModuleItem::Unsupported {
            node: other.to_owned(),
            span,
        },
    })
}

fn type_parameter(node: &Bound<'_, PyAny>) -> PyResult<TypeParameterNode> {
    let span = span(node)?;
    Ok(match kind(node)?.as_str() {
        "TypeVar" => TypeParameterNode::TypeVariable {
            name: attr(node, "name")?,
            bound: optional_expression_attr(node, "bound")?,
            has_default: !node.getattr("default_value")?.is_none(),
            span,
        },
        other => TypeParameterNode::Unsupported {
            node: other.to_owned(),
            span,
        },
    })
}

fn class_item(node: &Bound<'_, PyAny>) -> PyResult<ClassItemNode> {
    let node_kind = kind(node)?;
    let node_span = span(node)?;
    if node_kind == "Pass" {
        return Ok(ClassItemNode::Pass { span: node_span });
    }
    if node_kind == "Expr" {
        let value = node.getattr("value")?;
        if kind(&value)? == "Constant"
            && matches!(constant(&value.getattr("value")?)?, ConstantValue::Str(_))
        {
            return Ok(ClassItemNode::Docstring { span: node_span });
        }
    }
    if node_kind == "AnnAssign" {
        let target = node.getattr("target")?;
        if kind(&target)? == "Name" && attr::<u8>(node, "simple")? != 0 {
            return Ok(ClassItemNode::Field {
                name: attr(&target, "id")?,
                annotation: expression_attr(node, "annotation")?,
                has_value: !node.getattr("value")?.is_none(),
                span: node_span,
            });
        }
    }
    Ok(ClassItemNode::Unsupported {
        node: node_kind,
        span: node_span,
    })
}

fn import_alias(node: &Bound<'_, PyAny>) -> PyResult<ImportAlias> {
    Ok(ImportAlias {
        name: attr(node, "name")?,
        alias: attr(node, "asname")?,
    })
}

fn arguments(node: &Bound<'_, PyAny>) -> PyResult<ArgumentsNode> {
    Ok(ArgumentsNode {
        positional_only: collect_attr(node, "posonlyargs", parameter)?,
        positional: collect_attr(node, "args", parameter)?,
        variable: optional_attr(node, "vararg", parameter)?,
        keyword_only: collect_attr(node, "kwonlyargs", parameter)?,
        keyword_variadic: optional_attr(node, "kwarg", parameter)?,
        defaults: collect_attr(node, "defaults", expression)?,
        keyword_defaults: collect_optional_attr(node, "kw_defaults", expression)?,
    })
}

fn parameter(node: &Bound<'_, PyAny>) -> PyResult<ParameterNode> {
    Ok(ParameterNode {
        name: attr(node, "arg")?,
        annotation: optional_expression_attr(node, "annotation")?,
        type_comment: attr(node, "type_comment")?,
        span: span(node)?,
    })
}

fn statement(node: &Bound<'_, PyAny>) -> PyResult<StatementNode> {
    let node_span = span(node)?;
    Ok(match kind(node)?.as_str() {
        "Return" => StatementNode::Return {
            value: optional_expression_attr(node, "value")?,
            span: node_span,
        },
        "Assign" => StatementNode::Assign {
            targets: collect_attr(node, "targets", expression)?,
            value: expression_attr(node, "value")?,
            type_comment: attr(node, "type_comment")?,
            span: node_span,
        },
        "AnnAssign" => StatementNode::AnnotatedAssignment {
            target: expression_attr(node, "target")?,
            annotation: expression_attr(node, "annotation")?,
            value: optional_expression_attr(node, "value")?,
            simple: attr::<u8>(node, "simple")? != 0,
            span: node_span,
        },
        "AugAssign" => StatementNode::AugmentedAssignment {
            target: expression_attr(node, "target")?,
            operator: binary_operator(&node.getattr("op")?)?,
            value: expression_attr(node, "value")?,
            span: node_span,
        },
        "Expr" => StatementNode::Expression {
            value: expression_attr(node, "value")?,
            span: node_span,
        },
        "If" => StatementNode::If {
            condition: expression_attr(node, "test")?,
            body: collect_attr(node, "body", statement)?,
            otherwise: collect_attr(node, "orelse", statement)?,
            span: node_span,
        },
        "For" => StatementNode::For {
            target: expression_attr(node, "target")?,
            iterable: expression_attr(node, "iter")?,
            body: collect_attr(node, "body", statement)?,
            otherwise: collect_attr(node, "orelse", statement)?,
            type_comment: attr(node, "type_comment")?,
            span: node_span,
        },
        "While" => StatementNode::While {
            condition: expression_attr(node, "test")?,
            body: collect_attr(node, "body", statement)?,
            otherwise: collect_attr(node, "orelse", statement)?,
            span: node_span,
        },
        "Match" => StatementNode::Match {
            subject: expression_attr(node, "subject")?,
            cases: collect_attr(node, "cases", match_case)?,
            span: node_span,
        },
        "Try" => StatementNode::Try {
            body: collect_attr(node, "body", statement)?,
            handlers: collect_attr(node, "handlers", exception_handler)?,
            otherwise: collect_attr(node, "orelse", statement)?,
            finalizer: collect_attr(node, "finalbody", statement)?,
            span: node_span,
        },
        "TryStar" => StatementNode::TryStar {
            body: collect_attr(node, "body", statement)?,
            handlers: collect_attr(node, "handlers", exception_handler)?,
            otherwise: collect_attr(node, "orelse", statement)?,
            finalizer: collect_attr(node, "finalbody", statement)?,
            span: node_span,
        },
        "With" => StatementNode::With {
            items: collect_attr(node, "items", with_item)?,
            body: collect_attr(node, "body", statement)?,
            span: node_span,
        },
        "Raise" => StatementNode::Raise {
            exception: optional_expression_attr(node, "exc")?,
            cause: optional_expression_attr(node, "cause")?,
            span: node_span,
        },
        "Assert" => StatementNode::Assert {
            condition: expression_attr(node, "test")?,
            message: optional_expression_attr(node, "msg")?,
            span: node_span,
        },
        "Break" => StatementNode::Break { span: node_span },
        "Continue" => StatementNode::Continue { span: node_span },
        "Pass" => StatementNode::Pass { span: node_span },
        other => StatementNode::Unsupported {
            node: other.to_owned(),
            span: node_span,
        },
    })
}

fn with_item(node: &Bound<'_, PyAny>) -> PyResult<WithItemNode> {
    let context = expression_attr(node, "context_expr")?;
    Ok(match optional_expression_attr(node, "optional_vars")? {
        Some(target) => WithItemNode::Bound { context, target },
        None => WithItemNode::Unbound { context },
    })
}

fn match_case(node: &Bound<'_, PyAny>) -> PyResult<MatchCaseNode> {
    let pattern_node = node.getattr("pattern")?;
    let case_span = span(&pattern_node)?;
    Ok(MatchCaseNode {
        pattern: pattern(&pattern_node)?,
        guard: optional_expression_attr(node, "guard")?,
        body: collect_attr(node, "body", statement)?,
        span: case_span,
    })
}

fn pattern(node: &Bound<'_, PyAny>) -> PyResult<PatternNode> {
    let node_span = span(node)?;
    Ok(match kind(node)?.as_str() {
        "MatchClass" => PatternNode::Class {
            class: expression_attr(node, "cls")?,
            positional: collect_attr(node, "patterns", pattern)?,
            keyword_attributes: attr(node, "kwd_attrs")?,
            keyword_patterns: collect_attr(node, "kwd_patterns", pattern)?,
            span: node_span,
        },
        "MatchAs" => {
            let nested = optional_attr(node, "pattern", pattern)?;
            let name: Option<String> = attr(node, "name")?;
            match (nested, name) {
                (None, None) => PatternNode::Wildcard { span: node_span },
                (None, Some(name)) => PatternNode::Capture {
                    name,
                    span: node_span,
                },
                (Some(pattern), Some(name)) => PatternNode::As {
                    pattern: Box::new(pattern),
                    name,
                    span: node_span,
                },
                (Some(_), None) => PatternNode::Unsupported {
                    node: "MatchAsWithoutName".to_owned(),
                    span: node_span,
                },
            }
        }
        other => PatternNode::Unsupported {
            node: other.to_owned(),
            span: node_span,
        },
    })
}

fn exception_handler(node: &Bound<'_, PyAny>) -> PyResult<ExceptionHandlerNode> {
    let node_span = span(node)?;
    let body = collect_attr(node, "body", statement)?;
    let exception = node.getattr("type")?;
    if exception.is_none() {
        return Ok(ExceptionHandlerNode::Bare {
            body,
            span: node_span,
        });
    }
    let exception = expression(&exception)?;
    let binding: Option<String> = attr(node, "name")?;
    Ok(match binding {
        Some(binding) => ExceptionHandlerNode::TypedBinding {
            exception,
            binding,
            body,
            span: node_span,
        },
        None => ExceptionHandlerNode::Typed {
            exception,
            body,
            span: node_span,
        },
    })
}

fn expression(node: &Bound<'_, PyAny>) -> PyResult<ExpressionNode> {
    let node_span = span(node)?;
    Ok(match kind(node)?.as_str() {
        "Name" => ExpressionNode::Name {
            identifier: attr(node, "id")?,
            context: expression_context(&node.getattr("ctx")?)?,
            span: node_span,
        },
        "Constant" => ExpressionNode::Constant {
            value: constant(&node.getattr("value")?)?,
            span: node_span,
        },
        "Tuple" => ExpressionNode::Tuple {
            elements: collect_attr(node, "elts", expression)?,
            context: expression_context(&node.getattr("ctx")?)?,
            span: node_span,
        },
        "List" => ExpressionNode::List {
            elements: collect_attr(node, "elts", expression)?,
            context: expression_context(&node.getattr("ctx")?)?,
            span: node_span,
        },
        "UnaryOp" => ExpressionNode::Unary {
            operator: unary_operator(&node.getattr("op")?)?,
            operand: Box::new(expression_attr(node, "operand")?),
            span: node_span,
        },
        "BinOp" => ExpressionNode::Binary {
            operator: binary_operator(&node.getattr("op")?)?,
            left: Box::new(expression_attr(node, "left")?),
            right: Box::new(expression_attr(node, "right")?),
            span: node_span,
        },
        "BoolOp" => ExpressionNode::Boolean {
            operator: boolean_operator(&node.getattr("op")?)?,
            values: collect_attr(node, "values", expression)?,
            span: node_span,
        },
        "Compare" => ExpressionNode::Compare {
            left: Box::new(expression_attr(node, "left")?),
            operators: collect_attr(node, "ops", comparison_operator)?,
            comparators: collect_attr(node, "comparators", expression)?,
            span: node_span,
        },
        "IfExp" => ExpressionNode::Conditional {
            condition: Box::new(expression_attr(node, "test")?),
            then_value: Box::new(expression_attr(node, "body")?),
            else_value: Box::new(expression_attr(node, "orelse")?),
            span: node_span,
        },
        "Call" => ExpressionNode::Call {
            callee: Box::new(expression_attr(node, "func")?),
            arguments: collect_attr(node, "args", expression)?,
            keywords: collect_attr(node, "keywords", keyword)?,
            span: node_span,
        },
        "Attribute" => ExpressionNode::Attribute {
            value: Box::new(expression_attr(node, "value")?),
            name: attr(node, "attr")?,
            context: expression_context(&node.getattr("ctx")?)?,
            span: node_span,
        },
        "Subscript" => ExpressionNode::Subscript {
            value: Box::new(expression_attr(node, "value")?),
            slice: Box::new(expression_attr(node, "slice")?),
            context: expression_context(&node.getattr("ctx")?)?,
            span: node_span,
        },
        other => ExpressionNode::Unsupported {
            node: other.to_owned(),
            span: node_span,
        },
    })
}

fn keyword(node: &Bound<'_, PyAny>) -> PyResult<KeywordNode> {
    Ok(KeywordNode {
        name: attr(node, "arg")?,
        value: expression_attr(node, "value")?,
        span: span(node)?,
    })
}

fn constant(value: &Bound<'_, PyAny>) -> PyResult<ConstantValue> {
    if value.is_none() {
        return Ok(ConstantValue::None);
    }
    let value_kind = kind(value)?;
    Ok(match value_kind.as_str() {
        "ellipsis" => ConstantValue::Ellipsis,
        "bool" => ConstantValue::Bool(value.extract()?),
        "int" => ConstantValue::Int(value.str()?.to_str()?.to_owned()),
        "str" => ConstantValue::Str(value.extract()?),
        "bytes" => ConstantValue::Bytes(hex(&value.extract::<Vec<u8>>()?)),
        other => ConstantValue::Unsupported(other.to_owned()),
    })
}

fn expression_context(node: &Bound<'_, PyAny>) -> PyResult<ExpressionContext> {
    Ok(match kind(node)?.as_str() {
        "Load" => ExpressionContext::Load,
        "Store" => ExpressionContext::Store,
        "Del" => ExpressionContext::Delete,
        _ => ExpressionContext::Unknown,
    })
}

fn unary_operator(node: &Bound<'_, PyAny>) -> PyResult<UnaryOperator> {
    Ok(match kind(node)?.as_str() {
        "UAdd" => UnaryOperator::Positive,
        "USub" => UnaryOperator::Negative,
        "Not" => UnaryOperator::Not,
        "Invert" => UnaryOperator::Invert,
        _ => UnaryOperator::Unknown,
    })
}

fn binary_operator(node: &Bound<'_, PyAny>) -> PyResult<BinaryOperator> {
    Ok(match kind(node)?.as_str() {
        "Add" => BinaryOperator::Add,
        "Sub" => BinaryOperator::Subtract,
        "Mult" => BinaryOperator::Multiply,
        "MatMult" => BinaryOperator::MatrixMultiply,
        "Div" => BinaryOperator::TrueDivide,
        "FloorDiv" => BinaryOperator::FloorDivide,
        "Mod" => BinaryOperator::Modulo,
        "Pow" => BinaryOperator::Power,
        "LShift" => BinaryOperator::LeftShift,
        "RShift" => BinaryOperator::RightShift,
        "BitOr" => BinaryOperator::BitOr,
        "BitXor" => BinaryOperator::BitXor,
        "BitAnd" => BinaryOperator::BitAnd,
        _ => BinaryOperator::Unknown,
    })
}

fn boolean_operator(node: &Bound<'_, PyAny>) -> PyResult<BooleanOperator> {
    Ok(match kind(node)?.as_str() {
        "And" => BooleanOperator::And,
        "Or" => BooleanOperator::Or,
        _ => BooleanOperator::Unknown,
    })
}

fn comparison_operator(node: &Bound<'_, PyAny>) -> PyResult<ComparisonOperator> {
    Ok(match kind(node)?.as_str() {
        "Eq" => ComparisonOperator::Equal,
        "NotEq" => ComparisonOperator::NotEqual,
        "Lt" => ComparisonOperator::Less,
        "LtE" => ComparisonOperator::LessEqual,
        "Gt" => ComparisonOperator::Greater,
        "GtE" => ComparisonOperator::GreaterEqual,
        "Is" => ComparisonOperator::Is,
        "IsNot" => ComparisonOperator::IsNot,
        "In" => ComparisonOperator::In,
        "NotIn" => ComparisonOperator::NotIn,
        _ => ComparisonOperator::Unknown,
    })
}

fn expression_attr(node: &Bound<'_, PyAny>, name: &str) -> PyResult<ExpressionNode> {
    expression(&node.getattr(name)?)
}

fn optional_expression_attr(
    node: &Bound<'_, PyAny>,
    name: &str,
) -> PyResult<Option<ExpressionNode>> {
    optional_attr(node, name, expression)
}

fn optional_attr<T>(
    node: &Bound<'_, PyAny>,
    name: &str,
    mapper: impl FnOnce(&Bound<'_, PyAny>) -> PyResult<T>,
) -> PyResult<Option<T>> {
    let value = node.getattr(name)?;
    if value.is_none() {
        Ok(None)
    } else {
        mapper(&value).map(Some)
    }
}

fn collect_attr<T>(
    node: &Bound<'_, PyAny>,
    name: &str,
    mapper: impl FnMut(&Bound<'_, PyAny>) -> PyResult<T>,
) -> PyResult<Vec<T>> {
    collect(&node.getattr(name)?, mapper)
}

fn collect_optional_attr<T>(
    node: &Bound<'_, PyAny>,
    name: &str,
    mut mapper: impl FnMut(&Bound<'_, PyAny>) -> PyResult<T>,
) -> PyResult<Vec<Option<T>>> {
    let mut values = Vec::new();
    for item in node.getattr(name)?.try_iter()? {
        let item = item?;
        values.push(if item.is_none() {
            None
        } else {
            Some(mapper(&item)?)
        });
    }
    Ok(values)
}

fn collect<T>(
    values: &Bound<'_, PyAny>,
    mut mapper: impl FnMut(&Bound<'_, PyAny>) -> PyResult<T>,
) -> PyResult<Vec<T>> {
    let mut result = Vec::new();
    for item in values.try_iter()? {
        result.push(mapper(&item?)?);
    }
    Ok(result)
}

fn attr<'py, T>(node: &Bound<'py, PyAny>, name: &str) -> PyResult<T>
where
    T: FromPyObjectOwned<'py>,
{
    node.getattr(name)?.extract().map_err(Into::into)
}

fn span(node: &Bound<'_, PyAny>) -> PyResult<SourceSpan> {
    Ok(SourceSpan {
        start_line: attr(node, "lineno")?,
        start_utf8_byte: attr(node, "col_offset")?,
        end_line: attr(node, "end_lineno")?,
        end_utf8_byte: attr(node, "end_col_offset")?,
    })
}

fn kind(node: &Bound<'_, PyAny>) -> PyResult<String> {
    Ok(node.get_type().name()?.to_str()?.to_owned())
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(char::from(DIGITS[usize::from(byte >> 4)]));
        result.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    result
}
