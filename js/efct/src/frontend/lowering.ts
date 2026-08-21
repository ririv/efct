import ts from "typescript";

import {
  type EcmaBinaryOperator,
  type EcmaConstantItem,
  type EcmaExpressionNode,
  type EcmaExternalEffect,
  type EcmaFunctionContract,
  type EcmaFunctionNode,
  type EcmaImportItem,
  type EcmaModuleDefinitionItem,
  type EcmaModuleItem,
  type EcmaParameterNode,
  type EcmaPartialBehavior,
  type EcmaPartialContract,
  type EcmaStatementNode,
  type EcmaTypeNode,
  type EcmaUnaryOperator,
  type UnsupportedEcmaModuleItem,
  type Utf16SourceSpan,
} from "./types.js";

export function lowerSourceFile(
  sourceFile: ts.SourceFile,
  resolveImport?: (specifier: string) => string | undefined,
): readonly EcmaModuleItem[] {
  return sourceFile.statements.map((statement) =>
    lowerModuleItem(sourceFile, statement, resolveImport)
  );
}

function lowerModuleItem(
  sourceFile: ts.SourceFile,
  statement: ts.Statement,
  resolveImport?: (specifier: string) => string | undefined,
): EcmaModuleItem {
  if (ts.isImportDeclaration(statement)) {
    return lowerImport(sourceFile, statement, resolveImport)
      ?? unsupportedModuleItem(sourceFile, statement);
  }
  if (ts.isVariableStatement(statement)) {
    return lowerModuleDefinition(sourceFile, statement)
      ?? lowerConstant(sourceFile, statement)
      ?? unsupportedModuleItem(sourceFile, statement);
  }
  return unsupportedModuleItem(sourceFile, statement);
}

function lowerConstant(
  sourceFile: ts.SourceFile,
  statement: ts.VariableStatement,
): EcmaConstantItem | undefined {
  if (
    hasModifier(statement, ts.SyntaxKind.DefaultKeyword)
    || (statement.declarationList.flags & ts.NodeFlags.Const) === 0
    || statement.declarationList.declarations.length !== 1
  ) {
    return undefined;
  }
  const [declaration] = statement.declarationList.declarations;
  if (
    declaration === undefined
    || !ts.isIdentifier(declaration.name)
    || declaration.initializer === undefined
  ) {
    return undefined;
  }
  const common = {
    kind: "constant" as const,
    name: declaration.name.text,
    value: lowerExpression(sourceFile, declaration.initializer),
    span: span(sourceFile, statement),
  };
  return declaration.type === undefined
    ? common
    : { ...common, annotation: lowerType(sourceFile, declaration.type) };
}

function lowerImport(
  sourceFile: ts.SourceFile,
  declaration: ts.ImportDeclaration,
  resolveImport?: (specifier: string) => string | undefined,
): EcmaImportItem | undefined {
  if (
    !ts.isStringLiteral(declaration.moduleSpecifier)
    || declaration.importClause === undefined
    || declaration.importClause.name !== undefined
    || declaration.importClause.namedBindings === undefined
    || !ts.isNamedImports(declaration.importClause.namedBindings)
    || declaration.attributes !== undefined
  ) {
    return undefined;
  }
  const common = {
    kind: "import",
    module: declaration.moduleSpecifier.text,
    names: declaration.importClause.namedBindings.elements.map((element) => ({
      imported: element.propertyName?.text ?? element.name.text,
      local: element.name.text,
      type_only: declaration.importClause?.isTypeOnly === true || element.isTypeOnly,
    })),
    span: span(sourceFile, declaration),
  } as const;
  const resolved = resolveImport?.(declaration.moduleSpecifier.text);
  return resolved === undefined ? common : { ...common, resolved };
}

function lowerModuleDefinition(
  sourceFile: ts.SourceFile,
  statement: ts.VariableStatement,
): EcmaModuleDefinitionItem | undefined {
  if (
    !hasModifier(statement, ts.SyntaxKind.ExportKeyword)
    || hasModifier(statement, ts.SyntaxKind.DefaultKeyword)
    || (statement.declarationList.flags & ts.NodeFlags.Const) === 0
    || statement.declarationList.declarations.length !== 1
  ) {
    return undefined;
  }
  const [declaration] = statement.declarationList.declarations;
  if (
    declaration === undefined
    || !ts.isObjectBindingPattern(declaration.name)
    || declaration.initializer === undefined
    || !ts.isCallExpression(declaration.initializer)
    || !isIdentifier(declaration.initializer.expression, "defineModule")
    || declaration.initializer.arguments.length !== 2
  ) {
    return undefined;
  }
  const [moduleUrl, definitions] = declaration.initializer.arguments;
  if (
    moduleUrl === undefined
    || definitions === undefined
    || !isImportMetaUrl(moduleUrl)
    || !ts.isObjectLiteralExpression(definitions)
  ) {
    return undefined;
  }
  const exports = lowerExportBindings(declaration.name);
  if (exports === undefined) {
    return undefined;
  }
  const functions: EcmaFunctionNode[] = [];
  for (const property of definitions.properties) {
    const functionNode = lowerFunctionProperty(sourceFile, property);
    if (functionNode === undefined) {
      return undefined;
    }
    functions.push(functionNode);
  }
  return {
    kind: "module_definition",
    exports,
    functions,
    span: span(sourceFile, statement),
  };
}

function lowerExportBindings(binding: ts.ObjectBindingPattern): string[] | undefined {
  const names: string[] = [];
  for (const element of binding.elements) {
    if (
      element.dotDotDotToken !== undefined
      || element.propertyName !== undefined
      || element.initializer !== undefined
      || !ts.isIdentifier(element.name)
    ) {
      return undefined;
    }
    names.push(element.name.text);
  }
  return names;
}

function lowerFunctionProperty(
  sourceFile: ts.SourceFile,
  property: ts.ObjectLiteralElementLike,
): EcmaFunctionNode | undefined {
  if (
    !ts.isPropertyAssignment(property)
    || !isStaticPropertyName(property.name)
  ) {
    return undefined;
  }
  const propertyName = property.name.text;
  const declaration = unwrapFunctionDeclaration(property.initializer);
  if (
    declaration === undefined
    || declaration.implementation.name === undefined
    || declaration.implementation.name.text !== propertyName
    || declaration.implementation.asteriskToken !== undefined
    || declaration.implementation.typeParameters !== undefined
    || hasModifier(declaration.implementation, ts.SyntaxKind.AsyncKeyword)
  ) {
    return undefined;
  }
  const returnType = declaration.implementation.type
    ?? ts.getJSDocReturnType(declaration.implementation);
  if (returnType === undefined) {
    return undefined;
  }
  const parameters: EcmaParameterNode[] = [];
  for (const parameter of declaration.implementation.parameters) {
    const lowered = lowerParameter(sourceFile, parameter);
    if (lowered === undefined) {
      return undefined;
    }
    parameters.push(lowered);
  }
  return {
    name: propertyName,
    contract: declaration.contract,
    parameters,
    returns: lowerType(sourceFile, returnType),
    body: declaration.implementation.body.statements.map((statement) =>
      lowerStatement(sourceFile, statement)
    ),
    span: span(sourceFile, declaration.implementation),
  };
}

interface FunctionDeclaration {
  readonly contract: EcmaFunctionContract;
  readonly implementation: ts.FunctionExpression;
}

function unwrapFunctionDeclaration(expression: ts.Expression): FunctionDeclaration | undefined {
  if (!ts.isCallExpression(expression) || expression.arguments.length !== 1) {
    return undefined;
  }
  const [implementation] = expression.arguments;
  if (implementation === undefined || !ts.isFunctionExpression(implementation)) {
    return undefined;
  }
  if (isIdentifier(expression.expression, "pure")) {
    return { contract: { kind: "pure", partial: { kind: "inferred" } }, implementation };
  }
  if (
    ts.isCallExpression(expression.expression)
    && isIdentifier(expression.expression.expression, "pure")
  ) {
    const behaviors = lowerPartialBehaviors(expression.expression.arguments);
    if (behaviors === undefined) {
      return undefined;
    }
    return {
      contract: {
        kind: "pure",
        partial: behaviors.length === 0
          ? { kind: "explicit_empty" }
          : { kind: "explicit", behaviors },
      },
      implementation,
    };
  }
  if (isIdentifier(expression.expression, "effects")) {
    return {
      contract: {
        kind: "effects",
        effects: { kind: "inferred" },
        partial: { kind: "inferred" },
      },
      implementation,
    };
  }
  if (
    ts.isCallExpression(expression.expression)
    && isIdentifier(expression.expression.expression, "effects")
    && expression.expression.arguments.length > 0
  ) {
    const declarations = lowerEffectDeclarations(expression.expression.arguments);
    if (declarations === undefined) {
      return undefined;
    }
    return {
      contract: {
        kind: "effects",
        effects: { kind: "explicit", effects: declarations.effects },
        partial: declarations.partials.length === 0
          ? { kind: "explicit_empty" }
          : { kind: "explicit", behaviors: declarations.partials },
      },
      implementation,
    };
  }
  return undefined;
}

interface LoweredEffectDeclarations {
  readonly effects: EcmaExternalEffect[];
  readonly partials: EcmaPartialBehavior[];
}

function lowerEffectDeclarations(
  arguments_: ts.NodeArray<ts.Expression>,
): LoweredEffectDeclarations | undefined {
  const effects: EcmaExternalEffect[] = [];
  const partials: EcmaPartialBehavior[] = [];
  let style: "string" | "strong" | undefined;
  for (const argument of arguments_) {
    if (ts.isStringLiteral(argument)) {
      if (style === "strong") {
        return undefined;
      }
      style = "string";
      const partial = lowerPartialName(argument.text);
      if (partial !== undefined) {
        partials.push(partial);
        continue;
      }
      const effect = lowerEffectName(argument.text);
      if (effect === undefined) {
        return undefined;
      }
      effects.push(effect);
      continue;
    }
    if (style === "string" || !ts.isCallExpression(argument) || argument.arguments.length !== 0) {
      return undefined;
    }
    style = "strong";
    const path = staticExpressionPath(argument.expression);
    if (path === undefined) {
      return undefined;
    }
    const partial = lowerStrongPartial(path);
    if (partial !== undefined) {
      partials.push(partial);
      continue;
    }
    const effect = lowerStrongEffect(path);
    if (effect === undefined) {
      return undefined;
    }
    effects.push(effect);
  }
  return { effects, partials };
}

function lowerPartialBehaviors(
  arguments_: ts.NodeArray<ts.Expression>,
): EcmaPartialBehavior[] | undefined {
  const behaviors: EcmaPartialBehavior[] = [];
  let style: "string" | "strong" | undefined;
  for (const argument of arguments_) {
    if (ts.isStringLiteral(argument)) {
      if (style === "strong") {
        return undefined;
      }
      style = "string";
      if (argument.text === "throw" || argument.text === "diverge") {
        behaviors.push(argument.text);
        continue;
      }
      return undefined;
    }
    if (
      style === "string"
      ||
      !ts.isCallExpression(argument)
      || argument.arguments.length !== 0
      || !ts.isPropertyAccessExpression(argument.expression)
      || !isIdentifier(argument.expression.expression, "partial")
    ) {
      return undefined;
    }
    style = "strong";
    switch (argument.expression.name.text) {
      case "Throw":
        behaviors.push("throw");
        break;
      case "Diverge":
        behaviors.push("diverge");
        break;
      default:
        return undefined;
    }
  }
  return behaviors;
}

function lowerPartialName(name: string): EcmaPartialBehavior | undefined {
  switch (name) {
    case "throw":
      return "throw";
    case "diverge":
      return "diverge";
    default:
      return undefined;
  }
}

function lowerEffectName(name: string): EcmaExternalEffect | undefined {
  switch (name) {
    case "console":
      return "console";
    case "file.read":
      return "file_read";
    case "file.write":
      return "file_write";
    case "network":
      return "network";
    case "clock":
      return "clock";
    case "random":
      return "random";
    case "environment":
      return "environment";
    case "process":
      return "process";
    case "state.read":
      return "state_read";
    case "state.write":
      return "state_write";
    case "unsafe":
      return "unsafe";
    default:
      return undefined;
  }
}

function lowerStrongPartial(path: readonly string[]): EcmaPartialBehavior | undefined {
  return path.length === 2 && path[0] === "partial"
    ? lowerPartialName(path[1]?.toLowerCase() ?? "")
    : undefined;
}

function lowerStrongEffect(path: readonly string[]): EcmaExternalEffect | undefined {
  const name = path.join(".");
  switch (name) {
    case "effect.Console":
      return "console";
    case "effect.File.Read":
      return "file_read";
    case "effect.File.Write":
      return "file_write";
    case "effect.Network":
      return "network";
    case "effect.Clock":
      return "clock";
    case "effect.Random":
      return "random";
    case "effect.Environment":
      return "environment";
    case "effect.Process":
      return "process";
    case "effect.State.Read":
      return "state_read";
    case "effect.State.Write":
      return "state_write";
    case "effect.Unsafe":
      return "unsafe";
    default:
      return undefined;
  }
}

function staticExpressionPath(expression: ts.Expression): string[] | undefined {
  if (ts.isIdentifier(expression)) {
    return [expression.text];
  }
  if (ts.isPropertyAccessExpression(expression)) {
    const parent = staticExpressionPath(expression.expression);
    return parent === undefined ? undefined : [...parent, expression.name.text];
  }
  return undefined;
}

function lowerParameter(
  sourceFile: ts.SourceFile,
  parameter: ts.ParameterDeclaration,
): EcmaParameterNode | undefined {
  const annotation = parameter.type ?? ts.getJSDocType(parameter);
  if (
    !ts.isIdentifier(parameter.name)
    || parameter.dotDotDotToken !== undefined
    || parameter.questionToken !== undefined
    || parameter.initializer !== undefined
    || annotation === undefined
    || parameter.modifiers !== undefined
  ) {
    return undefined;
  }
  return {
    name: parameter.name.text,
    annotation: lowerType(sourceFile, annotation),
    span: span(sourceFile, parameter),
  };
}

function lowerType(sourceFile: ts.SourceFile, node: ts.TypeNode): EcmaTypeNode {
  if (ts.isUnionTypeNode(node)) {
    return lowerOptionalType(sourceFile, node);
  }
  switch (node.kind) {
    case ts.SyntaxKind.UndefinedKeyword:
      return { kind: "undefined" };
    case ts.SyntaxKind.BooleanKeyword:
      return { kind: "boolean" };
    case ts.SyntaxKind.NumberKeyword:
      return { kind: "number" };
    case ts.SyntaxKind.BigIntKeyword:
      return { kind: "big_int" };
    case ts.SyntaxKind.StringKeyword:
      return { kind: "string" };
    case ts.SyntaxKind.VoidKeyword:
      return { kind: "void" };
    default:
      if (ts.isLiteralTypeNode(node) && node.literal.kind === ts.SyntaxKind.NullKeyword) {
        return { kind: "null" };
      }
      return {
        kind: "unsupported",
        node: syntaxName(node),
        span: span(sourceFile, node),
      };
  }
}

function lowerOptionalType(sourceFile: ts.SourceFile, node: ts.UnionTypeNode): EcmaTypeNode {
  if (node.types.length === 2) {
    const absence = node.types.find((member) =>
      member.kind === ts.SyntaxKind.UndefinedKeyword
      || ts.isLiteralTypeNode(member) && member.literal.kind === ts.SyntaxKind.NullKeyword
    );
    const value = node.types.find((member) => member !== absence);
    if (absence !== undefined && value !== undefined) {
      const lowered = lowerType(sourceFile, value);
      if (
        lowered.kind !== "undefined"
        && lowered.kind !== "null"
        && lowered.kind !== "void"
        && lowered.kind !== "optional"
        && lowered.kind !== "unsupported"
      ) {
        return {
          kind: "optional",
          value: lowered,
          absence: absence.kind === ts.SyntaxKind.UndefinedKeyword ? "undefined" : "null",
        };
      }
    }
  }
  return { kind: "unsupported", node: syntaxName(node), span: span(sourceFile, node) };
}

function lowerStatement(sourceFile: ts.SourceFile, statement: ts.Statement): EcmaStatementNode {
  if (
    ts.isVariableStatement(statement)
    && statement.declarationList.declarations.length === 1
  ) {
    const declaration = statement.declarationList.declarations[0];
    if (
      declaration !== undefined
      && ts.isIdentifier(declaration.name)
      && declaration.initializer !== undefined
    ) {
      const common = {
        kind: "variable" as const,
        name: declaration.name.text,
        value: lowerExpression(sourceFile, declaration.initializer),
        span: span(sourceFile, statement),
      };
      return declaration.type === undefined
        ? common
        : { ...common, annotation: lowerType(sourceFile, declaration.type) };
    }
  }
  if (ts.isExpressionStatement(statement)) {
    if (
      ts.isBinaryExpression(statement.expression)
      && statement.expression.operatorToken.kind === ts.SyntaxKind.EqualsToken
      && ts.isIdentifier(statement.expression.left)
    ) {
      return {
        kind: "assignment",
        name: statement.expression.left.text,
        value: lowerExpression(sourceFile, statement.expression.right),
        span: span(sourceFile, statement),
      };
    }
    return {
      kind: "expression",
      expression: lowerExpression(sourceFile, statement.expression),
      span: span(sourceFile, statement),
    };
  }
  if (ts.isReturnStatement(statement)) {
    const statementSpan = span(sourceFile, statement);
    return statement.expression === undefined
      ? { kind: "return", span: statementSpan }
      : {
          kind: "return",
          value: lowerExpression(sourceFile, statement.expression),
          span: statementSpan,
        };
  }
  if (ts.isIfStatement(statement) && ts.isBlock(statement.thenStatement)) {
    const elseBody = statement.elseStatement === undefined
      ? []
      : ts.isBlock(statement.elseStatement)
      ? statement.elseStatement.statements.map((nested) => lowerStatement(sourceFile, nested))
      : undefined;
    if (elseBody !== undefined) {
      return {
        kind: "if",
        condition: lowerExpression(sourceFile, statement.expression),
        then_body: statement.thenStatement.statements.map((nested) =>
          lowerStatement(sourceFile, nested)
        ),
        else_body: elseBody,
        span: span(sourceFile, statement),
      };
    }
  }
  if (ts.isWhileStatement(statement) && ts.isBlock(statement.statement)) {
    return {
      kind: "while",
      condition: lowerExpression(sourceFile, statement.expression),
      body: statement.statement.statements.map((nested) => lowerStatement(sourceFile, nested)),
      span: span(sourceFile, statement),
    };
  }
  if (ts.isThrowStatement(statement)) {
    return {
      kind: "throw",
      value: lowerExpression(sourceFile, statement.expression),
      span: span(sourceFile, statement),
    };
  }
  if (ts.isTryStatement(statement)) {
    if (statement.catchClause?.variableDeclaration !== undefined) {
      return {
        kind: "unsupported",
        node: syntaxName(statement.catchClause.variableDeclaration),
        span: span(sourceFile, statement),
      };
    }
    const catchBody = statement.catchClause?.block.statements.map((nested) =>
      lowerStatement(sourceFile, nested)
    );
    const finallyBody = statement.finallyBlock?.statements.map((nested) =>
      lowerStatement(sourceFile, nested)
    );
    return {
      kind: "try",
      body: statement.tryBlock.statements.map((nested) => lowerStatement(sourceFile, nested)),
      ...(catchBody === undefined ? {} : { catch_body: catchBody }),
      ...(finallyBody === undefined ? {} : { finally_body: finallyBody }),
      span: span(sourceFile, statement),
    };
  }
  return {
    kind: "unsupported",
    node: syntaxName(statement),
    span: span(sourceFile, statement),
  };
}

function lowerExpression(sourceFile: ts.SourceFile, expression: ts.Expression): EcmaExpressionNode {
  const expressionSpan = span(sourceFile, expression);
  if (ts.isIdentifier(expression)) {
    return expression.text === "undefined"
      ? { kind: "undefined", span: expressionSpan }
      : { kind: "identifier", name: expression.text, span: expressionSpan };
  }
  if (expression.kind === ts.SyntaxKind.NullKeyword) {
    return { kind: "null", span: expressionSpan };
  }
  if (expression.kind === ts.SyntaxKind.TrueKeyword) {
    return { kind: "boolean", value: true, span: expressionSpan };
  }
  if (expression.kind === ts.SyntaxKind.FalseKeyword) {
    return { kind: "boolean", value: false, span: expressionSpan };
  }
  if (ts.isNumericLiteral(expression)) {
    return { kind: "number", text: expression.text, span: expressionSpan };
  }
  if (ts.isBigIntLiteral(expression)) {
    return { kind: "big_int", text: expression.text, span: expressionSpan };
  }
  if (ts.isStringLiteral(expression)) {
    return { kind: "string", value: expression.text, span: expressionSpan };
  }
  if (ts.isParenthesizedExpression(expression)) {
    return lowerExpression(sourceFile, expression.expression);
  }
  if (ts.isPrefixUnaryExpression(expression)) {
    const operator = lowerUnaryOperator(expression.operator);
    if (operator !== undefined) {
      return {
        kind: "unary",
        operator,
        operand: lowerExpression(sourceFile, expression.operand),
        span: expressionSpan,
      };
    }
  }
  if (ts.isBinaryExpression(expression)) {
    const operator = lowerBinaryOperator(expression.operatorToken.kind);
    if (operator !== undefined) {
      return {
        kind: "binary",
        left: lowerExpression(sourceFile, expression.left),
        operator,
        right: lowerExpression(sourceFile, expression.right),
        span: expressionSpan,
      };
    }
  }
  if (ts.isConditionalExpression(expression)) {
    return {
      kind: "conditional",
      condition: lowerExpression(sourceFile, expression.condition),
      when_true: lowerExpression(sourceFile, expression.whenTrue),
      when_false: lowerExpression(sourceFile, expression.whenFalse),
      span: expressionSpan,
    };
  }
  if (ts.isCallExpression(expression)) {
    const target = staticExpressionPath(expression.expression);
    if (target !== undefined && expression.typeArguments === undefined) {
      return {
        kind: "call",
        target,
        arguments: expression.arguments.map((argument) => lowerExpression(sourceFile, argument)),
        span: expressionSpan,
      };
    }
  }
  if (ts.isPropertyAccessExpression(expression)) {
    const target = staticExpressionPath(expression);
    if (target !== undefined) {
      return { kind: "property", target, span: expressionSpan };
    }
  }
  if (
    ts.isNewExpression(expression)
    && expression.typeArguments === undefined
    && ts.isIdentifier(expression.expression)
    && expression.arguments !== undefined
    && expression.arguments.length <= 1
  ) {
    const constructor = lowerErrorConstructor(expression.expression.text);
    if (constructor !== undefined) {
      const [message] = expression.arguments;
      return message === undefined
        ? { kind: "error", constructor, span: expressionSpan }
        : {
            kind: "error",
            constructor,
            message: lowerExpression(sourceFile, message),
            span: expressionSpan,
          };
    }
  }
  return {
    kind: "unsupported",
    node: syntaxName(expression),
    span: expressionSpan,
  };
}

function lowerErrorConstructor(
  name: string,
): "error" | "type_error" | "range_error" | undefined {
  switch (name) {
    case "Error":
      return "error";
    case "TypeError":
      return "type_error";
    case "RangeError":
      return "range_error";
    default:
      return undefined;
  }
}

function lowerUnaryOperator(operator: ts.PrefixUnaryOperator): EcmaUnaryOperator | undefined {
  switch (operator) {
    case ts.SyntaxKind.PlusToken:
      return "positive";
    case ts.SyntaxKind.MinusToken:
      return "negative";
    case ts.SyntaxKind.ExclamationToken:
      return "not";
    default:
      return undefined;
  }
}

function lowerBinaryOperator(operator: ts.SyntaxKind): EcmaBinaryOperator | undefined {
  switch (operator) {
    case ts.SyntaxKind.PlusToken:
      return "add";
    case ts.SyntaxKind.MinusToken:
      return "subtract";
    case ts.SyntaxKind.AsteriskToken:
      return "multiply";
    case ts.SyntaxKind.SlashToken:
      return "divide";
    case ts.SyntaxKind.PercentToken:
      return "remainder";
    case ts.SyntaxKind.EqualsEqualsEqualsToken:
      return "strict_equal";
    case ts.SyntaxKind.ExclamationEqualsEqualsToken:
      return "strict_not_equal";
    case ts.SyntaxKind.LessThanToken:
      return "less";
    case ts.SyntaxKind.LessThanEqualsToken:
      return "less_equal";
    case ts.SyntaxKind.GreaterThanToken:
      return "greater";
    case ts.SyntaxKind.GreaterThanEqualsToken:
      return "greater_equal";
    case ts.SyntaxKind.AmpersandAmpersandToken:
      return "and";
    case ts.SyntaxKind.BarBarToken:
      return "or";
    default:
      return undefined;
  }
}

function unsupportedModuleItem(
  sourceFile: ts.SourceFile,
  statement: ts.Statement,
): UnsupportedEcmaModuleItem {
  return {
    kind: "unsupported",
    node: syntaxName(statement),
    span: span(sourceFile, statement),
  };
}

function isImportMetaUrl(expression: ts.Expression): boolean {
  return ts.isPropertyAccessExpression(expression)
    && expression.name.text === "url"
    && ts.isMetaProperty(expression.expression)
    && expression.expression.keywordToken === ts.SyntaxKind.ImportKeyword
    && expression.expression.name.text === "meta";
}

function isStaticPropertyName(name: ts.PropertyName): name is ts.Identifier | ts.StringLiteral {
  return ts.isIdentifier(name) || ts.isStringLiteral(name);
}

function isIdentifier(expression: ts.Expression, expected: string): boolean {
  return ts.isIdentifier(expression) && expression.text === expected;
}

function hasModifier(node: ts.Node, modifier: ts.SyntaxKind): boolean {
  return ts.canHaveModifiers(node)
    && ts.getModifiers(node)?.some((item) => item.kind === modifier) === true;
}

function span(sourceFile: ts.SourceFile, node: ts.Node): Utf16SourceSpan {
  return { start: node.getStart(sourceFile), end: node.getEnd() };
}

function syntaxName(node: ts.Node): string {
  return ts.SyntaxKind[node.kind] ?? `SyntaxKind(${node.kind})`;
}
