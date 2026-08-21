import {
  type DeclaredImplementation,
  type FunctionDeclaration,
  type FunctionImplementation,
} from "./declarations.js";
import { type EcmaTypeNode } from "./frontend/types.js";
import { matchesRuntimeType, verifyRuntimeIdentity } from "./runtime.js";

type ModuleDeclarations = Readonly<Record<string, FunctionDeclaration<FunctionImplementation>>>;

export type DefinedModule<Declarations extends ModuleDeclarations> = {
  readonly [Name in keyof Declarations]: DeclaredImplementation<Declarations[Name]>;
};

export class EfctStartupError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "EfctStartupError";
  }
}

export interface RuntimeFunctionPlan {
  readonly contract:
    | { readonly kind: "pure"; readonly partials: readonly string[] | "inferred" }
    | {
        readonly kind: "effects";
        readonly effects: readonly string[] | "inferred";
        readonly partials: readonly string[] | "inferred";
      };
  readonly parameters: readonly EcmaTypeNode[];
  readonly returns: EcmaTypeNode;
}

export interface RuntimeModulePlan {
  readonly url: string;
  readonly functions: ReadonlyMap<string, RuntimeFunctionPlan>;
}

type RuntimeModuleState =
  | { readonly kind: "pending"; readonly plan: RuntimeModulePlan }
  | { readonly kind: "sealed" };

type RuntimeState =
  | { readonly kind: "idle" }
  | { readonly kind: "verifying"; readonly modules: Map<string, RuntimeModuleState> };

let runtimeState: RuntimeState = { kind: "idle" };

export function beginRuntimeVerification(plans: readonly RuntimeModulePlan[]): void {
  if (runtimeState.kind !== "idle") {
    throw new EfctStartupError("An Efct runtime verification is already active");
  }
  runtimeState = {
    kind: "verifying",
    modules: new Map(plans.map((plan) => [plan.url, { kind: "pending", plan }])),
  };
}

export function endRuntimeVerification(): void {
  runtimeState = { kind: "idle" };
}

export function completeRuntimeVerification(): void {
  if (runtimeState.kind !== "verifying") {
    throw new EfctStartupError("No Efct runtime verification is active");
  }
  const pending = [...runtimeState.modules.entries()]
    .filter(([, state]) => state.kind === "pending")
    .map(([url]) => url);
  if (pending.length > 0) {
    throw new EfctStartupError(
      `Verified modules were not sealed during evaluation: ${pending.join(", ")}`,
    );
  }
  runtimeState = { kind: "idle" };
}

export function defineModule<Declarations extends ModuleDeclarations>(
  moduleUrl: string,
  declarations: Declarations,
): DefinedModule<Declarations> {
  if (runtimeState.kind !== "verifying") {
    throw new EfctStartupError(
      "Efct verification context is missing; run this module through efct run",
    );
  }
  const state = runtimeState.modules.get(moduleUrl);
  if (state === undefined) {
    throw new EfctStartupError(`No verified runtime plan exists for ${moduleUrl}`);
  }
  if (state.kind === "sealed") {
    throw new EfctStartupError(`Efct module was already sealed: ${moduleUrl}`);
  }
  const actualNames = Object.keys(declarations).sort();
  const expectedNames = [...state.plan.functions.keys()].sort();
  if (JSON.stringify(actualNames) !== JSON.stringify(expectedNames)) {
    throw new EfctStartupError(`Runtime declarations do not match the verified plan for ${moduleUrl}`);
  }
  const exports: Record<string, FunctionImplementation> = {};
  for (const [name, plan] of state.plan.functions) {
    const declaration = declarations[name];
    if (declaration === undefined || !matchesContract(plan, declaration)) {
      throw new EfctStartupError(`Runtime declaration for ${name} does not match its verified plan`);
    }
    exports[name] = createCheckedFunction(name, plan, declaration.implementation);
  }
  runtimeState.modules.set(moduleUrl, { kind: "sealed" });
  return Object.freeze(exports) as DefinedModule<Declarations>;
}

function matchesContract(
  plan: RuntimeFunctionPlan,
  declaration: FunctionDeclaration<FunctionImplementation>,
): boolean {
  if (plan.contract.kind !== declaration.kind) {
    return false;
  }
  const partials = declaration.partials === "inferred"
    ? "inferred"
    : declaration.partials.map((partial) => partial.kind);
  if (!sameSet(plan.contract.partials, partials)) {
    return false;
  }
  return plan.contract.kind === "pure" || declaration.kind === "effects"
    && sameSet(
      plan.contract.effects,
      declaration.effects === "inferred"
        ? "inferred"
        : declaration.effects.map((effect) => effect.kind.replace(".", "_")),
    );
}

function sameSet(
  expected: readonly string[] | "inferred",
  actual: readonly string[] | "inferred",
): boolean {
  if (expected === "inferred" || actual === "inferred") {
    return expected === actual;
  }
  return expected.length === actual.length
    && [...expected].sort().every((value, index) => value === [...actual].sort()[index]);
}

function createCheckedFunction(
  name: string,
  plan: RuntimeFunctionPlan,
  implementation: FunctionImplementation,
): FunctionImplementation {
  const checked = (...arguments_: never[]): unknown => {
    verifyRuntimeIdentity();
    if (arguments_.length !== plan.parameters.length) {
      throw new TypeError(`${name} expects ${plan.parameters.length} arguments`);
    }
    for (const [index, type] of plan.parameters.entries()) {
      if (!matchesRuntimeType(type, arguments_[index])) {
        throw new TypeError(`${name} received an invalid value for argument ${index + 1}`);
      }
    }
    const result = Reflect.apply(implementation, undefined, arguments_);
    if (!matchesRuntimeType(plan.returns, result)) {
      throw new TypeError(`${name} returned a value outside its exact Efct type`);
    }
    return result;
  };
  return Object.freeze(checked);
}
