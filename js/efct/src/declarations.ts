const declarationBrand: unique symbol = Symbol("efct.declaration");
const partialBrand: unique symbol = Symbol("efct.partial");
const effectBrand: unique symbol = Symbol("efct.effect");

export type FunctionImplementation = (...arguments_: never[]) => unknown;

export interface PureFunctionDeclaration<Implementation extends FunctionImplementation> {
  readonly [declarationBrand]: Implementation;
  readonly kind: "pure";
  readonly implementation: Implementation;
  readonly partials: readonly PartialDeclaration[] | "inferred";
}

export interface EffectsFunctionDeclaration<Implementation extends FunctionImplementation> {
  readonly [declarationBrand]: Implementation;
  readonly kind: "effects";
  readonly implementation: Implementation;
  readonly effects: readonly EffectDeclaration[] | "inferred";
  readonly partials: readonly PartialDeclaration[] | "inferred";
}

export type PartialKind = "throw" | "diverge";
export type PartialName = PartialKind;
export type PartialInput = PartialDeclaration | PartialName;

export type EffectName =
  | "console"
  | "file.read"
  | "file.write"
  | "network"
  | "clock"
  | "random"
  | "environment"
  | "process"
  | "state.read"
  | "state.write"
  | "unsafe";

export interface EffectDeclaration {
  readonly [effectBrand]: true;
  readonly kind: EffectName;
}

export type EffectInput = EffectDeclaration | EffectName;
export type DeclarationInput = EffectInput | PartialInput;

export interface PartialDeclaration {
  readonly [partialBrand]: true;
  readonly kind: PartialKind;
}

export const partial = Object.freeze({
  Throw: (): PartialDeclaration => declarePartial("throw"),
  Diverge: (): PartialDeclaration => declarePartial("diverge"),
});

export const effect = Object.freeze({
  Console: (): EffectDeclaration => declareEffect("console"),
  File: Object.freeze({
    Read: (): EffectDeclaration => declareEffect("file.read"),
    Write: (): EffectDeclaration => declareEffect("file.write"),
  }),
  Network: (): EffectDeclaration => declareEffect("network"),
  Clock: (): EffectDeclaration => declareEffect("clock"),
  Random: (): EffectDeclaration => declareEffect("random"),
  Environment: (): EffectDeclaration => declareEffect("environment"),
  Process: (): EffectDeclaration => declareEffect("process"),
  State: Object.freeze({
    Read: (): EffectDeclaration => declareEffect("state.read"),
    Write: (): EffectDeclaration => declareEffect("state.write"),
  }),
  Unsafe: (): EffectDeclaration => declareEffect("unsafe"),
});

export type FunctionDeclaration<Implementation extends FunctionImplementation> =
  | PureFunctionDeclaration<Implementation>
  | EffectsFunctionDeclaration<Implementation>;

export type DeclaredImplementation<Declaration> = Declaration extends FunctionDeclaration<
  infer Implementation
>
  ? Implementation
  : never;

export function pure<Implementation extends FunctionImplementation>(
  implementation: Implementation,
): PureFunctionDeclaration<Implementation>;
export function pure(): <Implementation extends FunctionImplementation>(
  implementation: Implementation,
) => PureFunctionDeclaration<Implementation>;
export function pure(...partials: readonly PartialInput[]): <
  Implementation extends FunctionImplementation,
>(implementation: Implementation) => PureFunctionDeclaration<Implementation>;
export function pure<Implementation extends FunctionImplementation>(
  ...arguments_: readonly unknown[]
):
  | PureFunctionDeclaration<Implementation>
  | (<NestedImplementation extends FunctionImplementation>(
      nestedImplementation: NestedImplementation,
    ) => PureFunctionDeclaration<NestedImplementation>) {
  const [first] = arguments_;
  if (typeof first !== "function") {
    const partials = arguments_ as readonly PartialInput[];
    return <NestedImplementation extends FunctionImplementation>(
      nestedImplementation: NestedImplementation,
    ): PureFunctionDeclaration<NestedImplementation> => declarePure(
      nestedImplementation,
      partials.map(normalizePartial),
    );
  }
  return declarePure(first as Implementation, "inferred");
}

export function effects<Implementation extends FunctionImplementation>(
  implementation: Implementation,
): EffectsFunctionDeclaration<Implementation>;
export function effects(...declarations: readonly [DeclarationInput, ...DeclarationInput[]]): <
  Implementation extends FunctionImplementation,
>(implementation: Implementation) => EffectsFunctionDeclaration<Implementation>;
export function effects<Implementation extends FunctionImplementation>(
  ...arguments_: readonly unknown[]
):
  | EffectsFunctionDeclaration<Implementation>
  | (<NestedImplementation extends FunctionImplementation>(
      nestedImplementation: NestedImplementation,
    ) => EffectsFunctionDeclaration<NestedImplementation>) {
  const [first] = arguments_;
  if (typeof first === "function") {
    return declareEffects(first as Implementation, "inferred", "inferred");
  }
  const declarations = arguments_ as readonly DeclarationInput[];
  const normalized = declarations.map(normalizeDeclaration);
  const declaredEffects = normalized.filter(isEffectDeclaration);
  const declaredPartials = normalized.filter(isPartialDeclaration);
  return <NestedImplementation extends FunctionImplementation>(
    nestedImplementation: NestedImplementation,
  ): EffectsFunctionDeclaration<NestedImplementation> =>
    declareEffects(nestedImplementation, declaredEffects, declaredPartials);
}

function normalizePartial(input: PartialInput): PartialDeclaration {
  return typeof input === "string" ? declarePartial(input) : input;
}

function normalizeDeclaration(input: DeclarationInput): EffectDeclaration | PartialDeclaration {
  if (typeof input !== "string") {
    return input;
  }
  return input === "throw" || input === "diverge"
    ? declarePartial(input)
    : declareEffect(input);
}

function declarePure<Implementation extends FunctionImplementation>(
  implementation: Implementation,
  partials: readonly PartialDeclaration[] | "inferred",
): PureFunctionDeclaration<Implementation> {
  return Object.freeze({
    [declarationBrand]: implementation,
    kind: "pure" as const,
    implementation,
    partials,
  });
}

function declareEffects<Implementation extends FunctionImplementation>(
  implementation: Implementation,
  declaredEffects: readonly EffectDeclaration[] | "inferred",
  declaredPartials: readonly PartialDeclaration[] | "inferred",
): EffectsFunctionDeclaration<Implementation> {
  return Object.freeze({
    [declarationBrand]: implementation,
    kind: "effects" as const,
    implementation,
    effects: declaredEffects,
    partials: declaredPartials,
  });
}

function declarePartial(kind: PartialKind): PartialDeclaration {
  return Object.freeze({ [partialBrand]: true as const, kind });
}

function declareEffect(kind: EffectName): EffectDeclaration {
  return Object.freeze({ [effectBrand]: true as const, kind });
}

function isEffectDeclaration(
  declaration: EffectDeclaration | PartialDeclaration,
): declaration is EffectDeclaration {
  return effectBrand in declaration;
}

function isPartialDeclaration(
  declaration: EffectDeclaration | PartialDeclaration,
): declaration is PartialDeclaration {
  return partialBrand in declaration;
}
