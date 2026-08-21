export const PROTOCOL_VERSION = 1 as const;
export const PACKAGE_VERSION = "0.1.0" as const;
export const SUPPORTED_NODE_VERSION = "24.19.0" as const;
export const REQUIRED_NODE_API_VERSION = 8 as const;
export const SUPPORTED_TYPESCRIPT_VERSION = "5.9.3" as const;

export interface TypeScriptCompilerIdentity {
  readonly version: string;
  readonly installation_sha256: string;
}

export interface NodeRuntimeIdentity {
  readonly version: readonly [number, number, number];
  readonly node_api_version: number;
}

export interface Utf16SourceSpan {
  readonly start: number;
  readonly end: number;
}

export interface EcmaImportName {
  readonly imported: string;
  readonly local: string;
  readonly type_only: boolean;
}

export interface EcmaImportItem {
  readonly kind: "import";
  readonly module: string;
  readonly resolved?: string;
  readonly names: readonly EcmaImportName[];
  readonly span: Utf16SourceSpan;
}

export interface EcmaConstantItem {
  readonly kind: "constant";
  readonly name: string;
  readonly annotation?: EcmaTypeNode;
  readonly value: EcmaExpressionNode;
  readonly span: Utf16SourceSpan;
}

export interface UnsupportedEcmaModuleItem {
  readonly kind: "unsupported";
  readonly node: string;
  readonly span: Utf16SourceSpan;
}

export type EcmaPartialContract =
  | { readonly kind: "inferred" }
  | { readonly kind: "explicit_empty" }
  | {
      readonly kind: "explicit";
      readonly behaviors: readonly EcmaPartialBehavior[];
    };

export type EcmaPartialBehavior = "throw" | "diverge";

export interface PureFunctionContract {
  readonly kind: "pure";
  readonly partial: EcmaPartialContract;
}

export type EcmaExternalEffect =
  | "console"
  | "file_read"
  | "file_write"
  | "network"
  | "clock"
  | "random"
  | "environment"
  | "process"
  | "state_read"
  | "state_write"
  | "unsafe";

export type EcmaEffectContract =
  | { readonly kind: "inferred" }
  | { readonly kind: "explicit"; readonly effects: readonly EcmaExternalEffect[] };

export interface EffectsFunctionContract {
  readonly kind: "effects";
  readonly effects: EcmaEffectContract;
  readonly partial: EcmaPartialContract;
}

export type EcmaFunctionContract = PureFunctionContract | EffectsFunctionContract;

export type EcmaTypeNode =
  | { readonly kind: "undefined" }
  | { readonly kind: "null" }
  | { readonly kind: "boolean" }
  | { readonly kind: "number" }
  | { readonly kind: "big_int" }
  | { readonly kind: "string" }
  | { readonly kind: "void" }
  | {
      readonly kind: "optional";
      readonly value: EcmaTypeNode;
      readonly absence: "null" | "undefined";
    }
  | { readonly kind: "unsupported"; readonly node: string; readonly span: Utf16SourceSpan };

export interface EcmaParameterNode {
  readonly name: string;
  readonly annotation: EcmaTypeNode;
  readonly span: Utf16SourceSpan;
}

export type EcmaUnaryOperator = "positive" | "negative" | "not";

export type EcmaBinaryOperator =
  | "add"
  | "subtract"
  | "multiply"
  | "divide"
  | "remainder"
  | "strict_equal"
  | "strict_not_equal"
  | "less"
  | "less_equal"
  | "greater"
  | "greater_equal"
  | "and"
  | "or";

export type EcmaExpressionNode =
  | { readonly kind: "identifier"; readonly name: string; readonly span: Utf16SourceSpan }
  | { readonly kind: "undefined"; readonly span: Utf16SourceSpan }
  | { readonly kind: "null"; readonly span: Utf16SourceSpan }
  | { readonly kind: "boolean"; readonly value: boolean; readonly span: Utf16SourceSpan }
  | { readonly kind: "number"; readonly text: string; readonly span: Utf16SourceSpan }
  | { readonly kind: "big_int"; readonly text: string; readonly span: Utf16SourceSpan }
  | { readonly kind: "string"; readonly value: string; readonly span: Utf16SourceSpan }
  | {
      readonly kind: "unary";
      readonly operator: EcmaUnaryOperator;
      readonly operand: EcmaExpressionNode;
      readonly span: Utf16SourceSpan;
    }
  | {
      readonly kind: "binary";
      readonly left: EcmaExpressionNode;
      readonly operator: EcmaBinaryOperator;
      readonly right: EcmaExpressionNode;
      readonly span: Utf16SourceSpan;
    }
  | {
      readonly kind: "conditional";
      readonly condition: EcmaExpressionNode;
      readonly when_true: EcmaExpressionNode;
      readonly when_false: EcmaExpressionNode;
      readonly span: Utf16SourceSpan;
    }
  | {
      readonly kind: "call";
      readonly target: readonly string[];
      readonly arguments: readonly EcmaExpressionNode[];
      readonly span: Utf16SourceSpan;
    }
  | {
      readonly kind: "property";
      readonly target: readonly string[];
      readonly span: Utf16SourceSpan;
    }
  | {
      readonly kind: "error";
      readonly constructor: "error" | "type_error" | "range_error";
      readonly message?: EcmaExpressionNode;
      readonly span: Utf16SourceSpan;
    }
  | { readonly kind: "unsupported"; readonly node: string; readonly span: Utf16SourceSpan };

export type EcmaStatementNode =
  | {
      readonly kind: "variable";
      readonly name: string;
      readonly annotation?: EcmaTypeNode;
      readonly value: EcmaExpressionNode;
      readonly span: Utf16SourceSpan;
    }
  | {
      readonly kind: "assignment";
      readonly name: string;
      readonly value: EcmaExpressionNode;
      readonly span: Utf16SourceSpan;
    }
  | {
      readonly kind: "expression";
      readonly expression: EcmaExpressionNode;
      readonly span: Utf16SourceSpan;
    }
  | {
      readonly kind: "return";
      readonly value?: EcmaExpressionNode;
      readonly span: Utf16SourceSpan;
    }
  | {
      readonly kind: "if";
      readonly condition: EcmaExpressionNode;
      readonly then_body: readonly EcmaStatementNode[];
      readonly else_body: readonly EcmaStatementNode[];
      readonly span: Utf16SourceSpan;
    }
  | {
      readonly kind: "while";
      readonly condition: EcmaExpressionNode;
      readonly body: readonly EcmaStatementNode[];
      readonly span: Utf16SourceSpan;
    }
  | {
      readonly kind: "throw";
      readonly value: EcmaExpressionNode;
      readonly span: Utf16SourceSpan;
    }
  | {
      readonly kind: "try";
      readonly body: readonly EcmaStatementNode[];
      readonly catch_body?: readonly EcmaStatementNode[];
      readonly finally_body?: readonly EcmaStatementNode[];
      readonly span: Utf16SourceSpan;
    }
  | { readonly kind: "unsupported"; readonly node: string; readonly span: Utf16SourceSpan };

export interface EcmaFunctionNode {
  readonly name: string;
  readonly contract: EcmaFunctionContract;
  readonly parameters: readonly EcmaParameterNode[];
  readonly returns: EcmaTypeNode;
  readonly body: readonly EcmaStatementNode[];
  readonly span: Utf16SourceSpan;
}

export interface EcmaModuleDefinitionItem {
  readonly kind: "module_definition";
  readonly exports: readonly string[];
  readonly functions: readonly EcmaFunctionNode[];
  readonly span: Utf16SourceSpan;
}

export type EcmaModuleItem =
  | EcmaImportItem
  | EcmaConstantItem
  | EcmaModuleDefinitionItem
  | UnsupportedEcmaModuleItem;

export interface EcmaModuleNode {
  readonly items: readonly EcmaModuleItem[];
}

export interface TypeScriptSourceLanguage {
  readonly kind: "typescript";
  readonly compiler: TypeScriptCompilerIdentity;
  readonly runtime: NodeRuntimeIdentity;
  readonly config_sha256: string;
  readonly root: EcmaModuleNode;
}

export interface JavaScriptSourceLanguage {
  readonly kind: "javascript";
  readonly checker: TypeScriptCompilerIdentity;
  readonly runtime: NodeRuntimeIdentity;
  readonly config_sha256: string;
  readonly root: EcmaModuleNode;
}

export type EcmaSourceLanguage = TypeScriptSourceLanguage | JavaScriptSourceLanguage;

export interface EcmaSourceEnvelope {
  readonly protocol_version: typeof PROTOCOL_VERSION;
  readonly filename: string;
  readonly source_sha256: string;
  readonly language: EcmaSourceLanguage;
}

export type EcmaLanguageIdentity =
  | {
      readonly kind: "typescript";
      readonly compiler: TypeScriptCompilerIdentity;
      readonly runtime: NodeRuntimeIdentity;
    }
  | {
      readonly kind: "javascript";
      readonly checker: TypeScriptCompilerIdentity;
      readonly runtime: NodeRuntimeIdentity;
    };

export interface EcmaProjectModule {
  readonly name: string;
  readonly envelope: EcmaSourceEnvelope;
}

export interface EcmaProjectEnvelope {
  readonly protocol_version: typeof PROTOCOL_VERSION;
  readonly language: EcmaLanguageIdentity;
  readonly root: string;
  readonly modules: readonly EcmaProjectModule[];
  readonly policy: "default";
  readonly external_symbols: readonly [];
}
