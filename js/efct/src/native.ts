import { createRequire } from "node:module";

import { type EfctDiagnostic } from "./diagnostics.js";
import {
  type EcmaProjectEnvelope,
  type EcmaSourceEnvelope,
} from "./frontend/types.js";

interface NativeBinding {
  readonly checkEnvelope: (envelope: EcmaSourceEnvelope) => unknown;
  readonly checkProject: (project: EcmaProjectEnvelope) => unknown;
  readonly protocolVersion: () => number;
}

export function checkProject(project: EcmaProjectEnvelope): readonly EfctDiagnostic[] {
  const diagnostics: unknown = binding.checkProject(project);
  if (!Array.isArray(diagnostics)) {
    throw new TypeError("Efct native verifier returned an invalid diagnostic collection");
  }
  return diagnostics as readonly EfctDiagnostic[];
}

const require = createRequire(import.meta.url);
const supportedPlatforms = new Set([
  "darwin-arm64",
  "darwin-x64",
  "linux-arm64",
  "linux-x64",
  "win32-x64",
]);
const platform = `${process.platform}-${process.arch}`;
if (!supportedPlatforms.has(platform)) {
  throw new Error(`Efct does not provide a Node-API binary for ${platform}`);
}
const binding = require(`../../native/efct-napi.${platform}.node`) as NativeBinding;

export function checkEnvelope(envelope: EcmaSourceEnvelope): readonly EfctDiagnostic[] {
  const diagnostics: unknown = binding.checkEnvelope(envelope);
  if (!Array.isArray(diagnostics)) {
    throw new TypeError("Efct native verifier returned an invalid diagnostic collection");
  }
  return diagnostics as readonly EfctDiagnostic[];
}

export function nativeProtocolVersion(): number {
  return binding.protocolVersion();
}
