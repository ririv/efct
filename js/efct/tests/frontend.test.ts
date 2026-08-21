import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import {
  EfctStartupError,
  FrontendError,
  PROTOCOL_VERSION,
  REQUIRED_NODE_API_VERSION,
  SUPPORTED_NODE_VERSION,
  SUPPORTED_TYPESCRIPT_VERSION,
  createSourceEnvelope,
  createProjectEnvelope,
  checkEnvelope,
  checkProject,
  defineModule,
  pure,
  nativeProtocolVersion,
} from "../src/index.js";

test("creates distinct empty TypeScript and JavaScript envelopes", async () => {
  const typescript = await createSourceEnvelope("empty.ts", "");
  const javascript = await createSourceEnvelope("empty.js", "");

  assert.equal(typescript.protocol_version, PROTOCOL_VERSION);
  assert.equal(typescript.language.kind, "typescript");
  assert.equal(javascript.language.kind, "javascript");
  assert.deepEqual(typescript.language.root.items, []);
  assert.deepEqual(javascript.language.root.items, []);
  assert.match(typescript.source_sha256, /^[0-9a-f]{64}$/u);
  assert.match(typescript.filename, /^file:/u);
  assert.equal(process.versions.node, SUPPORTED_NODE_VERSION);
  assert.equal(SUPPORTED_TYPESCRIPT_VERSION, "5.9.3");
  assert.equal(typescript.language.runtime.node_api_version, REQUIRED_NODE_API_VERSION);
});

test("preserves unsupported syntax and UTF-16 offsets", async () => {
  const source = "// 😀\nclass Example {}\n";
  const envelope = await createSourceEnvelope("unsupported.ts", source);
  const [item] = envelope.language.root.items;

  assert.ok(item);
  assert.equal(item.kind, "unsupported");
  assert.equal(item.node, "ClassDeclaration");
  assert.equal(item.span.start, source.indexOf("class"));
  assert.equal(item.span.end, source.indexOf("\n", item.span.start));
});

test("rejects unsupported source extensions", async () => {
  await assert.rejects(
    createSourceEnvelope("component.tsx", ""),
    (error: unknown) => error instanceof FrontendError
      && error.message === "Unsupported JavaScript frontend file: component.tsx",
  );
});

test("rejects ordinary TypeScript diagnostics before creating an envelope", async () => {
  await assert.rejects(
    createSourceEnvelope("invalid.ts", "const value: number = 'wrong';\n"),
    (error: unknown) => error instanceof FrontendError
      && error.message.includes("Type 'string' is not assignable to type 'number'"),
  );
});

test("lowers an explicitly total pure arithmetic function", async () => {
  const filename = new URL(
    "../../../../tests/acceptance/typescript/accepted/pure_add.ts",
    import.meta.url,
  );
  const source = await readFile(filename, "utf8");
  const envelope = await createSourceEnvelope(filename.pathname, source);

  assert.equal(envelope.language.root.items[0]?.kind, "import");
  const definition = envelope.language.root.items[1];
  assert.equal(definition?.kind, "module_definition");
  if (definition?.kind !== "module_definition") {
    assert.fail("expected a module definition");
  }
  assert.deepEqual(definition.exports, ["add"]);
  assert.equal(definition.functions[0]?.name, "add");
  assert.deepEqual(definition.functions[0]?.contract, {
    kind: "pure",
    partial: { kind: "explicit_empty" },
  });
  assert.deepEqual(
    definition.functions[0]?.parameters.map((parameter) => parameter.annotation.kind),
    ["number", "number"],
  );
  assert.equal(definition.functions[0]?.returns.kind, "number");
  assert.equal(definition.functions[0]?.body[0]?.kind, "return");
});

test("preserves a rejected arrow implementation for the Rust verifier", async () => {
  const filename = new URL(
    "../../../../tests/acceptance/typescript/rejected/arrow_function.ts",
    import.meta.url,
  );
  const source = await readFile(filename, "utf8");
  const envelope = await createSourceEnvelope(filename.pathname, source);

  assert.equal(envelope.language.root.items[1]?.kind, "unsupported");
  assert.equal(
    envelope.language.root.items[1]?.kind === "unsupported"
      ? envelope.language.root.items[1].node
      : undefined,
    "FirstStatement",
  );
});

test("lowers explicit Throw and Diverge partial whitelists", async () => {
  const throwFilename = new URL(
    "../../../../tests/acceptance/typescript/accepted/pure_throw.ts",
    import.meta.url,
  );
  const divergeFilename = new URL(
    "../../../../tests/acceptance/typescript/accepted/pure_diverge.ts",
    import.meta.url,
  );
  const [throwEnvelope, divergeEnvelope] = await Promise.all([
    readFile(throwFilename, "utf8").then((source) =>
      createSourceEnvelope(throwFilename.pathname, source)
    ),
    readFile(divergeFilename, "utf8").then((source) =>
      createSourceEnvelope(divergeFilename.pathname, source)
    ),
  ]);
  const throwDefinition = throwEnvelope.language.root.items[1];
  const divergeDefinition = divergeEnvelope.language.root.items[1];

  assert.equal(throwDefinition?.kind, "module_definition");
  assert.equal(divergeDefinition?.kind, "module_definition");
  if (
    throwDefinition?.kind !== "module_definition"
    || divergeDefinition?.kind !== "module_definition"
  ) {
    assert.fail("expected module definitions");
  }
  assert.deepEqual(throwDefinition.functions[0]?.contract, {
    kind: "pure",
    partial: { kind: "explicit", behaviors: ["throw"] },
  });
  assert.equal(throwDefinition.functions[0]?.body[0]?.kind, "if");
  assert.deepEqual(divergeDefinition.functions[0]?.contract, {
    kind: "pure",
    partial: { kind: "explicit", behaviors: ["diverge"] },
  });
  assert.equal(divergeDefinition.functions[0]?.body[0]?.kind, "while");
});

test("lowers strong external effect declarations and registered calls", async () => {
  const filename = new URL(
    "../../../../tests/acceptance/typescript/accepted/effects_clock.ts",
    import.meta.url,
  );
  const source = await readFile(filename, "utf8");
  const envelope = await createSourceEnvelope(filename.pathname, source);
  const definition = envelope.language.root.items[1];

  assert.equal(definition?.kind, "module_definition");
  if (definition?.kind !== "module_definition") {
    assert.fail("expected a module definition");
  }
  assert.deepEqual(definition.functions[0]?.contract, {
    kind: "effects",
    effects: { kind: "explicit", effects: ["clock"] },
    partial: { kind: "explicit_empty" },
  });
  const statement = definition.functions[0]?.body[0];
  assert.equal(statement?.kind, "return");
  assert.equal(statement?.kind === "return" ? statement.value?.kind : undefined, "call");
});

test("keeps string declarations but rejects mixing declaration styles", async () => {
  const stringSource = `import { defineModule, effects } from "@efct/efct";
export const { currentTime } = defineModule(import.meta.url, {
  currentTime: effects("clock")(function currentTime(): number { return Date.now(); }),
});
`;
  const mixedSource = `import { defineModule, effect, effects } from "@efct/efct";
export const { currentTime } = defineModule(import.meta.url, {
  currentTime: effects(effect.Clock(), "throw")(
    function currentTime(): number { return Date.now(); },
  ),
});
`;
  const [stringEnvelope, mixedEnvelope] = await Promise.all([
    createSourceEnvelope("string-effects.ts", stringSource),
    createSourceEnvelope("mixed-effects.ts", mixedSource),
  ]);
  const stringDefinition = stringEnvelope.language.root.items[1];

  assert.equal(stringDefinition?.kind, "module_definition");
  if (stringDefinition?.kind !== "module_definition") {
    assert.fail("expected a module definition");
  }
  assert.deepEqual(stringDefinition.functions[0]?.contract, {
    kind: "effects",
    effects: { kind: "explicit", effects: ["clock"] },
    partial: { kind: "explicit_empty" },
  });
  assert.equal(mixedEnvelope.language.root.items[1]?.kind, "unsupported");
});

test("lowers JavaScript JSDoc to the same primitive HIR", async () => {
  const filename = new URL(
    "../../../../tests/acceptance/javascript/accepted/pure_add.js",
    import.meta.url,
  );
  const source = await readFile(filename, "utf8");
  const envelope = await createSourceEnvelope(filename.pathname, source);
  const definition = envelope.language.root.items[1];

  assert.equal(envelope.language.kind, "javascript");
  assert.equal(definition?.kind, "module_definition");
  if (definition?.kind !== "module_definition") {
    assert.fail("expected a module definition");
  }
  assert.deepEqual(
    definition.functions[0]?.parameters.map((parameter) => parameter.annotation.kind),
    ["number", "number"],
  );
  assert.equal(definition.functions[0]?.returns.kind, "number");
});

test("preserves JavaScript parameters without explicit JSDoc for rejection", async () => {
  const filename = new URL(
    "../../../../tests/acceptance/javascript/rejected/implicit_any.js",
    import.meta.url,
  );
  const source = await readFile(filename, "utf8");

  const envelope = await createSourceEnvelope(filename.pathname, source);

  assert.equal(envelope.language.root.items[1]?.kind, "unsupported");
});

test("distinguishes nullable and undefined optional unions", async () => {
  const acceptedFilename = new URL(
    "../../../../tests/acceptance/typescript/accepted/optional_identity.ts",
    import.meta.url,
  );
  const rejectedFilename = new URL(
    "../../../../tests/acceptance/typescript/rejected/three_way_union.ts",
    import.meta.url,
  );
  const [accepted, rejected] = await Promise.all([
    readFile(acceptedFilename, "utf8").then((source) =>
      createSourceEnvelope(acceptedFilename.pathname, source)
    ),
    readFile(rejectedFilename, "utf8").then((source) =>
      createSourceEnvelope(rejectedFilename.pathname, source)
    ),
  ]);
  const acceptedDefinition = accepted.language.root.items[1];
  const rejectedDefinition = rejected.language.root.items[1];

  assert.equal(acceptedDefinition?.kind, "module_definition");
  if (acceptedDefinition?.kind !== "module_definition") {
    assert.fail("expected a module definition");
  }
  assert.deepEqual(acceptedDefinition.functions[0]?.parameters[0]?.annotation, {
    kind: "optional",
    value: { kind: "number" },
    absence: "null",
  });
  assert.equal(
    rejectedDefinition?.kind === "module_definition"
      ? rejectedDefinition.functions[0]?.parameters[0]?.annotation.kind
      : undefined,
    "unsupported",
  );
});

test("lowers static constants without executing their initializers", async () => {
  const filename = new URL(
    "../../../../tests/acceptance/typescript/accepted/static_constant.ts",
    import.meta.url,
  );
  const source = await readFile(filename, "utf8");
  const envelope = await createSourceEnvelope(filename.pathname, source);
  const constant = envelope.language.root.items[1];

  assert.deepEqual(constant, {
    kind: "constant",
    name: "INCREMENT",
    annotation: { kind: "number" },
    value: {
      kind: "number",
      text: "1",
      span: constant?.kind === "constant" ? constant.value.span : undefined,
    },
    span: constant?.span,
  });
});

test("lowers direct same-module calls for call-graph propagation", async () => {
  const filename = new URL(
    "../../../../tests/acceptance/typescript/accepted/same_module_call.ts",
    import.meta.url,
  );
  const source = await readFile(filename, "utf8");
  const envelope = await createSourceEnvelope(filename.pathname, source);
  const definition = envelope.language.root.items[1];

  assert.equal(definition?.kind, "module_definition");
  if (definition?.kind !== "module_definition") {
    assert.fail("expected a module definition");
  }
  const statement = definition.functions[1]?.body[0];
  assert.deepEqual(
    statement?.kind === "return" ? statement.value : undefined,
    {
      kind: "call",
      target: ["currentTime"],
      arguments: [],
      span: statement?.kind === "return" && statement.value?.kind === "call"
        ? statement.value.span
        : undefined,
    },
  );
});

test("lowers unbound catch, finally, and immediate Error construction", async () => {
  const caughtFilename = new URL(
    "../../../../tests/acceptance/typescript/accepted/caught_throw.ts",
    import.meta.url,
  );
  const finallyFilename = new URL(
    "../../../../tests/acceptance/typescript/accepted/finally_override.ts",
    import.meta.url,
  );
  const [caught, finalizer] = await Promise.all([
    readFile(caughtFilename, "utf8").then((source) =>
      createSourceEnvelope(caughtFilename.pathname, source)
    ),
    readFile(finallyFilename, "utf8").then((source) =>
      createSourceEnvelope(finallyFilename.pathname, source)
    ),
  ]);
  for (const envelope of [caught, finalizer]) {
    const definition = envelope.language.root.items[1];
    assert.equal(definition?.kind, "module_definition");
    if (definition?.kind !== "module_definition") {
      assert.fail("expected a module definition");
    }
    const statement = definition.functions[0]?.body[0];
    assert.equal(statement?.kind, "try");
    assert.equal(
      statement?.kind === "try" ? statement.body[0]?.kind : undefined,
      "throw",
    );
  }
});

test("preserves registered Node imports and environment property paths", async () => {
  const fileFilename = new URL(
    "../../../../tests/acceptance/typescript/accepted/file_read.ts",
    import.meta.url,
  );
  const environmentFilename = new URL(
    "../../../../tests/acceptance/typescript/accepted/environment_read.ts",
    import.meta.url,
  );
  const [fileEnvelope, environmentEnvelope] = await Promise.all([
    readFile(fileFilename, "utf8").then((source) =>
      createSourceEnvelope(fileFilename.pathname, source)
    ),
    readFile(environmentFilename, "utf8").then((source) =>
      createSourceEnvelope(environmentFilename.pathname, source)
    ),
  ]);
  assert.equal(fileEnvelope.language.root.items[0]?.kind, "import");
  const definition = environmentEnvelope.language.root.items[1];
  assert.equal(definition?.kind, "module_definition");
  if (definition?.kind !== "module_definition") {
    assert.fail("expected a module definition");
  }
  const statement = definition.functions[0]?.body[0];
  assert.deepEqual(
    statement?.kind === "return" ? statement.value : undefined,
    {
      kind: "property",
      target: ["process", "env", "HOME"],
      span: statement?.kind === "return" && statement.value?.kind === "property"
        ? statement.value.span
        : undefined,
    },
  );
});

test("preserves optional absence tests for Rust branch narrowing", async () => {
  const filename = new URL(
    "../../../../tests/acceptance/typescript/accepted/optional_narrowing.ts",
    import.meta.url,
  );
  const source = await readFile(filename, "utf8");
  const envelope = await createSourceEnvelope(filename.pathname, source);
  const definition = envelope.language.root.items[1];

  assert.equal(definition?.kind, "module_definition");
  if (definition?.kind !== "module_definition") {
    assert.fail("expected a module definition");
  }
  const statement = definition.functions[0]?.body[0];
  assert.equal(statement?.kind, "if");
  assert.equal(statement?.kind === "if" ? statement.condition.kind : undefined, "binary");
});

test("does not execute a module without a verification context", () => {
  assert.throws(
    () => defineModule(import.meta.url, {
      add: pure()(function add(left: number, right: number): number {
        return left + right;
      }),
    }),
    EfctStartupError,
  );
});

test("passes typed envelopes through the native Rust verifier", async () => {
  const acceptedFilename = new URL(
    "../../../../tests/acceptance/typescript/accepted/pure_add.ts",
    import.meta.url,
  );
  const rejectedFilename = new URL(
    "../../../../tests/acceptance/typescript/rejected/hidden_clock.ts",
    import.meta.url,
  );
  const [accepted, rejected] = await Promise.all([
    readFile(acceptedFilename, "utf8").then((source) =>
      createSourceEnvelope(acceptedFilename.pathname, source)
    ),
    readFile(rejectedFilename, "utf8").then((source) =>
      createSourceEnvelope(rejectedFilename.pathname, source)
    ),
  ]);

  assert.equal(nativeProtocolVersion(), PROTOCOL_VERSION);
  assert.deepEqual(checkEnvelope(accepted), []);
  assert.equal(checkEnvelope(rejected)[0]?.code, "J0005");
});

test("checks a local ESM project through the native Rust verifier", async () => {
  const entry = new URL(
    "../../../../tests/acceptance/typescript/accepted/project_entry.ts",
    import.meta.url,
  );
  const project = await createProjectEnvelope(entry.pathname);

  assert.equal(project.modules.length, 2);
  assert.deepEqual(checkProject(project), []);
  const root = project.modules.find((module) => module.name === project.root);
  const localImport = root?.envelope.language.root.items.find(
    (item) => item.kind === "import" && item.module.startsWith("."),
  );
  assert.equal(localImport?.kind, "import");
  assert.match(localImport?.kind === "import" ? localImport.resolved ?? "" : "", /^file:/u);
});

test("checks local primitive state and conditional expressions", async () => {
  const filename = new URL(
    "../../../../tests/acceptance/typescript/accepted/local_state.ts",
    import.meta.url,
  );
  const project = await createProjectEnvelope(filename.pathname);

  assert.deepEqual(checkProject(project), []);
  const definition = project.modules[0]?.envelope.language.root.items.find(
    (item) => item.kind === "module_definition",
  );
  const body = definition?.kind === "module_definition"
    ? definition.functions[0]?.body
    : undefined;
  assert.equal(body?.[0]?.kind, "variable");
  assert.equal(body?.[1]?.kind, "assignment");
  assert.equal(body?.[2]?.kind === "return" ? body[2].value?.kind : undefined, "conditional");
});
