import { registerHooks } from "node:module";
import * as childProcess from "node:child_process";
import * as fileSystem from "node:fs";

import ts from "typescript";

import {
  prepareProjectEnvelope,
  type PreparedProjectEnvelope,
} from "./frontend/envelope.js";
import {
  type EcmaFunctionContract,
  type EcmaFunctionNode,
  type EcmaProjectEnvelope,
  type EcmaTypeNode,
} from "./frontend/types.js";
import { checkProject } from "./native.js";
import {
  beginRuntimeVerification,
  completeRuntimeVerification,
  endRuntimeVerification,
  type RuntimeModulePlan,
} from "./module.js";

export class EfctVerificationError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "EfctVerificationError";
  }
}

const runtimeIdentity = Object.freeze({
  dateNow: Date.now,
  performanceNow: performance.now,
  mathRandom: Math.random,
  consoleLog: console.log,
  consoleError: console.error,
  readFileSync: fileSystem.readFileSync,
  writeFileSync: fileSystem.writeFileSync,
  spawnSync: childProcess.spawnSync,
  error: Error,
  typeError: TypeError,
  rangeError: RangeError,
});

export function verifyRuntimeIdentity(): void {
  const valid = Date.now === runtimeIdentity.dateNow
    && performance.now === runtimeIdentity.performanceNow
    && Math.random === runtimeIdentity.mathRandom
    && console.log === runtimeIdentity.consoleLog
    && console.error === runtimeIdentity.consoleError
    && fileSystem.readFileSync === runtimeIdentity.readFileSync
    && fileSystem.writeFileSync === runtimeIdentity.writeFileSync
    && childProcess.spawnSync === runtimeIdentity.spawnSync
    && Error === runtimeIdentity.error
    && TypeError === runtimeIdentity.typeError
    && RangeError === runtimeIdentity.rangeError;
  if (!valid) {
    throw new EfctVerificationError("A registered Node.js runtime binding changed after startup");
  }
}

export async function runVerifiedModule(
  entryFilename: string,
): Promise<Record<string, unknown>> {
  verifyCleanNodeLaunch();
  const prepared = await prepareProjectEnvelope(entryFilename);
  const diagnostics = checkProject(prepared.envelope);
  if (diagnostics.some((diagnostic) => diagnostic.severity === "Error")) {
    throw new EfctVerificationError(JSON.stringify(diagnostics));
  }
  verifyRuntimeIdentity();
  return executePreparedProject(prepared);
}

function verifyCleanNodeLaunch(): void {
  const nodeOptions = process.env.NODE_OPTIONS;
  if (nodeOptions !== undefined && nodeOptions.trim() !== "") {
    throw new EfctVerificationError("NODE_OPTIONS is not allowed for verified execution");
  }
  const unsafeLaunchOption = process.execArgv.find((option) =>
    option === "-r"
    || option.startsWith("--require")
    || option.startsWith("--import")
    || option.startsWith("--loader")
    || option.startsWith("--experimental-loader")
  );
  if (unsafeLaunchOption !== undefined) {
    throw new EfctVerificationError(
      `Node preload and loader options are not allowed: ${unsafeLaunchOption}`,
    );
  }
}

async function executePreparedProject(
  prepared: PreparedProjectEnvelope,
): Promise<Record<string, unknown>> {
  const resolution = localResolutionMap(prepared.envelope);
  const hooks = registerHooks({
    resolve(specifier, context, nextResolve) {
      const resolved = context.parentURL === undefined
        ? undefined
        : resolution.get(`${context.parentURL}\0${specifier}`);
      return resolved === undefined
        ? nextResolve(specifier, context)
        : { url: resolved, shortCircuit: true };
    },
    load(url, context, nextLoad) {
      const source = prepared.sources.get(url);
      if (source === undefined) {
        return nextLoad(url, context);
      }
      return {
        format: "module",
        shortCircuit: true,
        source: url.endsWith(".ts")
          ? stripVerifiedTypeScript(source, url)
          : source,
      };
    },
  });
  beginRuntimeVerification(runtimePlans(prepared.envelope));
  try {
    const loaded = await import(prepared.envelope.root) as Record<string, unknown>;
    completeRuntimeVerification();
    return loaded;
  } finally {
    hooks.deregister();
    endRuntimeVerification();
  }
}

function stripVerifiedTypeScript(source: string, filename: string): string {
  return ts.transpileModule(source, {
    fileName: filename,
    compilerOptions: {
      module: ts.ModuleKind.ESNext,
      target: ts.ScriptTarget.ESNext,
      verbatimModuleSyntax: true,
    },
    reportDiagnostics: false,
  }).outputText;
}

function localResolutionMap(project: EcmaProjectEnvelope): ReadonlyMap<string, string> {
  const resolution = new Map<string, string>();
  for (const module of project.modules) {
    for (const item of module.envelope.language.root.items) {
      if (item.kind === "import" && item.resolved !== undefined) {
        resolution.set(`${module.name}\0${item.module}`, item.resolved);
      }
    }
  }
  return resolution;
}

function runtimePlans(project: EcmaProjectEnvelope): readonly RuntimeModulePlan[] {
  return project.modules.map((module) => {
    const definitions = module.envelope.language.root.items.filter(
      (item) => item.kind === "module_definition",
    );
    if (definitions.length !== 1 || definitions[0] === undefined) {
      throw new EfctVerificationError(`Verified module lacks one definition: ${module.name}`);
    }
    return {
      url: module.name,
      functions: new Map(definitions[0].functions.map((functionNode) => [
        functionNode.name,
        runtimeFunctionPlan(functionNode),
      ])),
    };
  });
}

function runtimeFunctionPlan(functionNode: EcmaFunctionNode) {
  return {
    contract: normalizeContract(functionNode.contract),
    parameters: functionNode.parameters.map((parameter) => parameter.annotation),
    returns: functionNode.returns,
  } as const;
}

function normalizeContract(contract: EcmaFunctionContract) {
  const partials = contract.partial.kind === "inferred"
    ? "inferred" as const
    : contract.partial.kind === "explicit_empty"
      ? []
      : contract.partial.behaviors;
  return contract.kind === "pure"
    ? { kind: "pure" as const, partials }
    : {
        kind: "effects" as const,
        effects: contract.effects.kind === "inferred" ? "inferred" as const : contract.effects.effects,
        partials,
      };
}

export function matchesRuntimeType(type: EcmaTypeNode, value: unknown): boolean {
  switch (type.kind) {
    case "undefined":
    case "void": return value === undefined;
    case "null": return value === null;
    case "boolean": return typeof value === "boolean";
    case "number": return typeof value === "number";
    case "big_int": return typeof value === "bigint";
    case "string": return typeof value === "string";
    case "optional":
      return type.absence === "null" && value === null
        || type.absence === "undefined" && value === undefined
        || matchesRuntimeType(type.value, value);
    case "unsupported": return false;
  }
}
