import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import test from "node:test";

const execute = promisify(execFile);
const cli = new URL("../src/cli.js", import.meta.url);
const accepted = new URL(
  "../../../../tests/acceptance/typescript/accepted/pure_add.ts",
  import.meta.url,
);
const rejected = new URL(
  "../../../../tests/acceptance/typescript/rejected/hidden_clock.ts",
  import.meta.url,
);
const project = new URL(
  "../../../../tests/acceptance/typescript/accepted/project_entry.ts",
  import.meta.url,
);

test("checks an accepted TypeScript file through the public CLI", async () => {
  const result = await execute(process.execPath, [cli.pathname, "check", accepted.pathname]);

  assert.match(result.stdout, /Efct verification passed/u);
  assert.match(result.stdout, /Efct 验证通过/u);
  assert.equal(result.stderr, "");
});

test("prints the public package version", async () => {
  const result = await execute(process.execPath, [cli.pathname, "--version"]);

  assert.equal(result.stdout, "efct 0.1.0\n");
  assert.equal(result.stderr, "");
});

test("returns structured diagnostics and exit code one for rejection", async () => {
  await assert.rejects(
    execute(process.execPath, [cli.pathname, "check", rejected.pathname, "--json"]),
    (error: unknown) => {
      if (!(error instanceof Error) || !("stdout" in error)) {
        return false;
      }
      const parsed = JSON.parse(String(error.stdout)) as readonly { readonly code: string }[];
      return parsed[0]?.code === "J0005";
    },
  );
});

test("checks the local ESM closure from an entry file", async () => {
  const result = await execute(process.execPath, [cli.pathname, "check", project.pathname]);

  assert.match(result.stdout, /Efct verification passed/u);
  assert.equal(result.stderr, "");
});

test("runs a verified TypeScript snapshot and checks its runtime boundary", async () => {
  const result = await execute(process.execPath, [
    cli.pathname,
    "run",
    project.pathname,
    "--call",
    "addOne",
    "--args",
    "[41]",
  ]);

  assert.equal(result.stdout, "42\n");
  assert.equal(result.stderr, "");
});

test("rejects an invalid runtime argument before entering the function body", async () => {
  await assert.rejects(
    execute(process.execPath, [
      cli.pathname,
      "run",
      project.pathname,
      "--call",
      "addOne",
      "--args",
      '["41"]',
    ]),
    (error: unknown) => error instanceof Error && /invalid value for argument 1/u.test(
      "stderr" in error ? String(error.stderr) : "",
    ),
  );
});

test("rejects Node preload configuration for verified execution", async () => {
  await assert.rejects(
    execute(
      process.execPath,
      [cli.pathname, "run", project.pathname],
      { env: { ...process.env, NODE_OPTIONS: "--trace-warnings" } },
    ),
    (error: unknown) => error instanceof Error && /NODE_OPTIONS is not allowed/u.test(
      "stderr" in error ? String(error.stderr) : "",
    ),
  );
});
