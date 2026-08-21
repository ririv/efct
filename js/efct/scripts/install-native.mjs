import { execFile } from "node:child_process";
import { copyFile, mkdir } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

const executeFile = promisify(execFile);

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const workspaceRoot = resolve(packageRoot, "../..");
const artifacts = {
  darwin: "libefct_napi.dylib",
  linux: "libefct_napi.so",
  win32: "efct_napi.dll",
};
const artifact = artifacts[process.platform];
if (artifact === undefined) {
  throw new Error(`Unsupported Efct native build platform: ${process.platform}`);
}
const destinationDirectory = resolve(packageRoot, "native");
const destination = resolve(
  destinationDirectory,
  `efct-napi.${process.platform}-${process.arch}.node`,
);

await mkdir(destinationDirectory, { recursive: true });
await copyFile(resolve(workspaceRoot, "target/release", artifact), destination);
if (process.platform === "darwin") {
  await executeFile("/usr/bin/codesign", ["--force", "--sign", "-", destination]);
}
