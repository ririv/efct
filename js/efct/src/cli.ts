#!/usr/bin/env node

import process from "node:process";

import { checkProject } from "./native.js";
import { createProjectEnvelope } from "./frontend/envelope.js";
import { PACKAGE_VERSION } from "./frontend/types.js";
import { type EfctDiagnostic } from "./diagnostics.js";
import { runVerifiedModule } from "./runtime.js";

const explanations: Readonly<Record<string, string>> = {
  J0001: "包含 Efct 尚未支持或无法安全识别的语法。",
  J0002: "表达式或返回值不满足精确类型契约。",
  J0003: "模块或函数声明结构无效。",
  J0004: "函数产生了 partial 白名单之外的行为。",
  J0005: "函数产生了外部效果白名单之外的行为。",
  P0002: "编译器、运行时或语言身份不受当前版本支持。",
};

async function main(arguments_: readonly string[]): Promise<number> {
  if (arguments_.length === 1 && arguments_[0] === "--version") {
    process.stdout.write(`efct ${PACKAGE_VERSION}\n`);
    return 0;
  }
  if (arguments_.length === 1 && (arguments_[0] === "--help" || arguments_[0] === "-h")) {
    writeUsage();
    return 0;
  }
  const [command, filename, ...options] = arguments_;
  if ((command !== "check" && command !== "run") || filename === undefined) {
    writeUsage();
    return 2;
  }
  if (command === "run") {
    return runCommand(filename, options);
  }
  const json = options.includes("--json");
  if (options.some((option) => option !== "--json")) {
    throw new Error(`Unknown Efct option: ${options.join(" ")}`);
  }
  const project = await createProjectEnvelope(filename);
  const diagnostics = checkProject(project);
  if (json) {
    process.stdout.write(`${JSON.stringify(diagnostics, undefined, 2)}\n`);
  } else {
    for (const diagnostic of diagnostics) {
      writeDiagnostic(diagnostic);
    }
    if (diagnostics.length === 0) {
      process.stdout.write(`Efct verification passed: ${filename}\nEfct 验证通过：${filename}\n`);
    }
  }
  return diagnostics.some((diagnostic) => diagnostic.severity === "Error") ? 1 : 0;
}

async function runCommand(filename: string, options: readonly string[]): Promise<number> {
  let exportName: string | undefined;
  let arguments_: readonly unknown[] = [];
  for (let index = 0; index < options.length; index += 2) {
    const option = options[index];
    const value = options[index + 1];
    if (value === undefined) {
      throw new Error(`Missing value for Efct option: ${option ?? "unknown"}`);
    }
    if (option === "--call") {
      exportName = value;
    } else if (option === "--args") {
      const parsed: unknown = JSON.parse(value);
      if (!Array.isArray(parsed)) {
        throw new Error("--args must be a JSON array");
      }
      arguments_ = parsed;
    } else {
      throw new Error(`Unknown Efct option: ${option}`);
    }
  }
  if (arguments_.length > 0 && exportName === undefined) {
    throw new Error("--args requires --call");
  }
  const module = await runVerifiedModule(filename);
  if (exportName !== undefined) {
    const callable = module[exportName];
    if (typeof callable !== "function") {
      throw new Error(`Verified entry does not export a function named ${exportName}`);
    }
    const result: unknown = Reflect.apply(callable, undefined, arguments_);
    process.stdout.write(
      result === undefined ? "undefined\n" : `${JSON.stringify(result)}\n`,
    );
  } else {
    process.stdout.write(`Efct verified execution loaded: ${filename}\nEfct 验证执行已加载：${filename}\n`);
  }
  return 0;
}

function writeDiagnostic(diagnostic: EfctDiagnostic): void {
  const functionSuffix = diagnostic.function === null ? "" : ` (${diagnostic.function})`;
  const chinese = explanations[diagnostic.code] ?? "Efct 验证失败。";
  process.stderr.write(
    `${diagnostic.filename}${functionSuffix}: ${diagnostic.code}: ${diagnostic.message}\n`
      + `  ${chinese}\n`,
  );
  if (diagnostic.suggestion !== null) {
    process.stderr.write(`  Suggestion: ${diagnostic.suggestion}\n  建议：${diagnostic.suggestion}\n`);
  }
}

function writeUsage(): void {
  process.stderr.write(
    "Usage: efct check <file.ts|file.js> [--json]\n"
      + "       efct run <file.ts|file.js> [--call <export>] [--args <json-array>]\n"
      + "       efct --version | --help\n"
      + "用法：efct check <file.ts|file.js> [--json]\n"
      + "      efct run <file.ts|file.js> [--call <导出名>] [--args <JSON 数组>]\n"
      + "      efct --version | --help\n",
  );
}

main(process.argv.slice(2)).then(
  (exitCode) => {
    process.exitCode = exitCode;
  },
  (error: unknown) => {
    const message = error instanceof Error ? error.message : String(error);
    process.stderr.write(`Efct failed: ${message}\nEfct 执行失败：${message}\n`);
    process.exitCode = 2;
  },
);
