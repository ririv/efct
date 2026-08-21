import { createHash } from "node:crypto";
import { readdir, readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { dirname, relative, resolve, sep } from "node:path";
import { pathToFileURL } from "node:url";

import ts from "typescript";

import { lowerSourceFile } from "./lowering.js";

import {
  PROTOCOL_VERSION,
  REQUIRED_NODE_API_VERSION,
  SUPPORTED_NODE_VERSION,
  SUPPORTED_TYPESCRIPT_VERSION,
  type EcmaSourceEnvelope,
  type EcmaProjectEnvelope,
  type EcmaSourceLanguage,
  type NodeRuntimeIdentity,
  type TypeScriptCompilerIdentity,
} from "./types.js";

type SourceKind =
  | { readonly kind: "typescript"; readonly scriptKind: ts.ScriptKind.TS }
  | { readonly kind: "javascript"; readonly scriptKind: ts.ScriptKind.JS };

const require = createRequire(import.meta.url);
const typescriptEntry = require.resolve("typescript");
const typescriptRoot = resolve(dirname(typescriptEntry), "..");
let compilerHash: Promise<string> | undefined;

export class FrontendError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "FrontendError";
  }
}

export async function createSourceEnvelope(
  filename: string,
  source: string,
): Promise<EcmaSourceEnvelope> {
  const sourceKind = classifySource(filename);
  verifyToolchain();
  const compilerOptions = createCompilerOptions(sourceKind);
  const sourceFile = createProgramSource(filename, source, sourceKind, compilerOptions);
  const compiler = await compilerIdentity();
  const runtime = runtimeIdentity();
  const root = {
    items: lowerSourceFile(sourceFile),
  };
  const common = {
    runtime,
    config_sha256: sha256(canonicalCompilerOptions(compilerOptions)),
    root,
  };
  const language: EcmaSourceLanguage = sourceKind.kind === "typescript"
    ? { kind: "typescript", compiler, ...common }
    : { kind: "javascript", checker: compiler, ...common };

  return {
    protocol_version: PROTOCOL_VERSION,
    filename: pathToFileURL(resolve(filename)).href,
    source_sha256: sha256(source),
    language,
  };
}

export async function createProjectEnvelope(
  entryFilename: string,
): Promise<EcmaProjectEnvelope> {
  return (await prepareProjectEnvelope(entryFilename)).envelope;
}

export interface PreparedProjectEnvelope {
  readonly envelope: EcmaProjectEnvelope;
  readonly sources: ReadonlyMap<string, string>;
}

export async function prepareProjectEnvelope(
  entryFilename: string,
): Promise<PreparedProjectEnvelope> {
  const absoluteEntry = resolve(entryFilename);
  const sourceKind = classifySource(absoluteEntry);
  verifyToolchain();
  const compilerOptions = createCompilerOptions(sourceKind);
  const program = ts.createProgram([absoluteEntry], compilerOptions);
  const diagnostics = ts.getPreEmitDiagnostics(program);
  if (diagnostics.length > 0) {
    throw new FrontendError(ts.formatDiagnostics(diagnostics, diagnosticHost));
  }
  const compiler = await compilerIdentity();
  const runtime = runtimeIdentity();
  const configSha256 = sha256(canonicalCompilerOptions(compilerOptions));
  const sourceFiles = collectProjectSources(program, absoluteEntry, compilerOptions, sourceKind);
  const modules = sourceFiles.map((sourceFile) => {
    const filename = pathToFileURL(resolve(sourceFile.fileName)).href;
    const language: EcmaSourceLanguage = sourceKind.kind === "typescript"
      ? {
          kind: "typescript",
          compiler,
          runtime,
          config_sha256: configSha256,
          root: {
            items: lowerSourceFile(sourceFile, (specifier) =>
              resolveLocalImport(specifier, sourceFile.fileName, compilerOptions)),
          },
        }
      : {
          kind: "javascript",
          checker: compiler,
          runtime,
          config_sha256: configSha256,
          root: {
            items: lowerSourceFile(sourceFile, (specifier) =>
              resolveLocalImport(specifier, sourceFile.fileName, compilerOptions)),
          },
        };
    return {
      name: filename,
      envelope: {
        protocol_version: PROTOCOL_VERSION,
        filename,
        source_sha256: sha256(sourceFile.text),
        language,
      },
    };
  });
  const root = pathToFileURL(absoluteEntry).href;
  const envelope: EcmaProjectEnvelope = {
    protocol_version: PROTOCOL_VERSION,
    language: sourceKind.kind === "typescript"
      ? { kind: "typescript", compiler, runtime }
      : { kind: "javascript", checker: compiler, runtime },
    root,
    modules,
    policy: "default",
    external_symbols: [],
  };
  return {
    envelope,
    sources: new Map(sourceFiles.map((sourceFile) => [
      pathToFileURL(resolve(sourceFile.fileName)).href,
      sourceFile.text,
    ])),
  };
}

function collectProjectSources(
  program: ts.Program,
  entryFilename: string,
  compilerOptions: ts.CompilerOptions,
  projectKind: SourceKind,
): readonly ts.SourceFile[] {
  const byFilename = new Map(
    program.getSourceFiles().map((sourceFile) => [resolve(sourceFile.fileName), sourceFile]),
  );
  const pending = [entryFilename];
  const visited = new Set<string>();
  const sources: ts.SourceFile[] = [];
  while (pending.length > 0) {
    const filename = pending.pop();
    if (filename === undefined || visited.has(filename)) {
      continue;
    }
    const sourceFile = byFilename.get(filename);
    if (sourceFile === undefined) {
      throw new FrontendError(`TypeScript did not retain project source ${filename}`);
    }
    const kind = classifySource(filename);
    if (kind.kind !== projectKind.kind) {
      throw new FrontendError("Mixed TypeScript and JavaScript projects are not supported in Efct 0.1");
    }
    visited.add(filename);
    sources.push(sourceFile);
    for (const statement of sourceFile.statements) {
      if (
        ts.isImportDeclaration(statement)
        && ts.isStringLiteral(statement.moduleSpecifier)
      ) {
        const resolvedImport = resolveLocalImportPath(
          statement.moduleSpecifier.text,
          filename,
          compilerOptions,
        );
        if (resolvedImport !== undefined) {
          pending.push(resolvedImport);
        }
      }
    }
  }
  sources.sort((left, right) => left.fileName.localeCompare(right.fileName));
  return sources;
}

function resolveLocalImport(
  specifier: string,
  containingFile: string,
  compilerOptions: ts.CompilerOptions,
): string | undefined {
  const filename = resolveLocalImportPath(specifier, containingFile, compilerOptions);
  return filename === undefined ? undefined : pathToFileURL(filename).href;
}

function resolveLocalImportPath(
  specifier: string,
  containingFile: string,
  compilerOptions: ts.CompilerOptions,
): string | undefined {
  if (!specifier.startsWith("./") && !specifier.startsWith("../")) {
    return undefined;
  }
  const resolution = ts.resolveModuleName(specifier, containingFile, compilerOptions, ts.sys);
  const filename = resolution.resolvedModule?.resolvedFileName;
  if (filename === undefined || filename.endsWith(".d.ts")) {
    throw new FrontendError(`Local module ${specifier} from ${containingFile} is not executable source`);
  }
  return resolve(filename);
}

function classifySource(filename: string): SourceKind {
  if (filename.endsWith(".ts") && !filename.endsWith(".d.ts")) {
    return { kind: "typescript", scriptKind: ts.ScriptKind.TS };
  }
  if (filename.endsWith(".js") || filename.endsWith(".mjs")) {
    return { kind: "javascript", scriptKind: ts.ScriptKind.JS };
  }
  throw new FrontendError(`Unsupported JavaScript frontend file: ${filename}`);
}

function verifyToolchain(): void {
  if (ts.version !== SUPPORTED_TYPESCRIPT_VERSION) {
    throw new FrontendError(
      `Unsupported TypeScript compiler version ${ts.version}; expected ${SUPPORTED_TYPESCRIPT_VERSION}`,
    );
  }
  if (process.versions.node !== SUPPORTED_NODE_VERSION) {
    throw new FrontendError(
      `Unsupported Node.js version ${process.versions.node}; expected ${SUPPORTED_NODE_VERSION}`,
    );
  }
  const nodeApiVersion = Number.parseInt(process.versions.napi ?? "", 10);
  if (!Number.isInteger(nodeApiVersion) || nodeApiVersion < REQUIRED_NODE_API_VERSION) {
    throw new FrontendError(
      `Node-API ${REQUIRED_NODE_API_VERSION} or newer is required`,
    );
  }
}

function createCompilerOptions(sourceKind: SourceKind): ts.CompilerOptions {
  const common = {
    exactOptionalPropertyTypes: true,
    erasableSyntaxOnly: true,
    module: ts.ModuleKind.NodeNext,
    moduleResolution: ts.ModuleResolutionKind.NodeNext,
    noEmit: true,
    noImplicitAny: true,
    noUncheckedIndexedAccess: true,
    strict: true,
    strictFunctionTypes: true,
    target: ts.ScriptTarget.ESNext,
    useUnknownInCatchVariables: true,
    verbatimModuleSyntax: true,
  } satisfies ts.CompilerOptions;
  return sourceKind.kind === "typescript"
    ? common
    : { ...common, allowJs: true, checkJs: true };
}

function createProgramSource(
  filename: string,
  source: string,
  sourceKind: SourceKind,
  compilerOptions: ts.CompilerOptions,
): ts.SourceFile {
  const absoluteFilename = resolve(filename);
  const sourceFile = ts.createSourceFile(
    absoluteFilename,
    source,
    {
      languageVersion: compilerOptions.target ?? ts.ScriptTarget.ESNext,
      impliedNodeFormat: ts.ModuleKind.ESNext,
    },
    true,
    sourceKind.scriptKind,
  );
  const host = ts.createCompilerHost(compilerOptions, true);
  const defaultFileExists = host.fileExists.bind(host);
  const defaultReadFile = host.readFile.bind(host);
  const defaultGetSourceFile = host.getSourceFile.bind(host);
  host.fileExists = (path) => path === absoluteFilename || defaultFileExists(path);
  host.readFile = (path) => path === absoluteFilename ? source : defaultReadFile(path);
  host.getSourceFile = (path, languageVersion, onError, shouldCreateNewSourceFile) =>
    path === absoluteFilename
      ? sourceFile
      : defaultGetSourceFile(path, languageVersion, onError, shouldCreateNewSourceFile);
  const program = ts.createProgram([absoluteFilename], compilerOptions, host);
  const diagnostics = ts.getPreEmitDiagnostics(program);
  if (diagnostics.length > 0) {
    throw new FrontendError(ts.formatDiagnostics(diagnostics, diagnosticHost));
  }
  const programSource = program.getSourceFile(absoluteFilename);
  if (programSource === undefined) {
    throw new FrontendError(`TypeScript did not retain source file ${absoluteFilename}`);
  }
  return programSource;
}

const diagnosticHost: ts.FormatDiagnosticsHost = {
  getCanonicalFileName: (filename) => filename,
  getCurrentDirectory: () => process.cwd(),
  getNewLine: () => "\n",
};

async function compilerIdentity(): Promise<TypeScriptCompilerIdentity> {
  compilerHash ??= hashDirectory(typescriptRoot);
  return {
    version: ts.version,
    installation_sha256: await compilerHash,
  };
}

function runtimeIdentity(): NodeRuntimeIdentity {
  const version = process.versions.node.split(".").map((value) => Number.parseInt(value, 10));
  if (version.length !== 3 || version.some((value) => !Number.isInteger(value))) {
    throw new FrontendError(`Invalid Node.js version identity: ${process.versions.node}`);
  }
  const [major, minor, patch] = version;
  if (major === undefined || minor === undefined || patch === undefined) {
    throw new FrontendError(`Invalid Node.js version identity: ${process.versions.node}`);
  }
  return {
    version: [major, minor, patch],
    node_api_version: REQUIRED_NODE_API_VERSION,
  };
}

function canonicalCompilerOptions(options: ts.CompilerOptions): string {
  return JSON.stringify(Object.entries(options).sort(([left], [right]) => left.localeCompare(right)));
}

function sha256(value: string | Uint8Array): string {
  return createHash("sha256").update(value).digest("hex");
}

async function hashDirectory(root: string): Promise<string> {
  const files = await collectFiles(root);
  const hash = createHash("sha256");
  for (const file of files) {
    const relativePath = relative(root, file).split(sep).join("/");
    const pathBytes = Buffer.from(relativePath, "utf8");
    const contents = await readFile(file);
    hash.update(lengthPrefix(pathBytes.byteLength));
    hash.update(pathBytes);
    hash.update(lengthPrefix(contents.byteLength));
    hash.update(contents);
  }
  return hash.digest("hex");
}

async function collectFiles(root: string): Promise<string[]> {
  const files: string[] = [];
  const entries = await readdir(root, { withFileTypes: true });
  entries.sort((left, right) => left.name.localeCompare(right.name));
  for (const entry of entries) {
    const path = resolve(root, entry.name);
    if (entry.isDirectory()) {
      files.push(...await collectFiles(path));
    } else if (entry.isFile()) {
      files.push(path);
    } else {
      throw new FrontendError(`Unsupported TypeScript installation entry: ${path}`);
    }
  }
  return files;
}

function lengthPrefix(value: number): Buffer {
  const buffer = Buffer.allocUnsafe(8);
  buffer.writeBigUInt64BE(BigInt(value));
  return buffer;
}
