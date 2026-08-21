# Efct for TypeScript and JavaScript

[简体中文](README.zh-CN.md)

Efct verifies whether functions stay inside explicit purity, external-effect, and partial-behavior boundaries. It rejects unknown syntax and calls instead of silently treating them as safe.

The 0.1 release targets ESM on exactly Node.js 24.19.0 and TypeScript 5.9.3. TypeScript and checked JavaScript share the same verifier. Browser, Bun, CommonJS, async functions, promises, objects, arrays, classes, closures, and arbitrary npm dependencies are not certified by this release.

## Install

```console
npm install @efct/efct
```

Efct uses a prebuilt Node-API native verifier. Installing the published package does not compile Rust locally.

The application package must declare ESM explicitly:

```json
{
  "type": "module"
}
```

## Define verified functions

```ts
import { defineModule, effect, effects, partial, pure } from "@efct/efct";

export const { add, currentTime, requireNonNegative } = defineModule(
  import.meta.url,
  {
    add: pure()(function add(left: number, right: number): number {
      return left + right;
    }),

    currentTime: effects(effect.Clock())(function currentTime(): number {
      return Date.now();
    }),

    requireNonNegative: pure(partial.Throw())(function requireNonNegative(
      value: number,
    ): number {
      if (value < 0) {
        throw new RangeError("value must be non-negative");
      }
      return value;
    }),
  },
);
```

`pure()` is an explicit empty partial whitelist. `pure(partial.Throw())` permits `throw`, while `pure(partial.Diverge())` permits possible non-termination. `effects(...)` is an external-effect whitelist; partial behavior must be listed in the same declaration when applicable.

The shorthand `pure(function named() {})` and `effects(function named() {})` ask Efct to infer the contract. Inferred contracts are accepted only inside their defining module. Importable functions use explicit contracts so a cross-module boundary cannot change silently.

String declarations remain available for generated code and compatibility:

```ts
import { readFileSync } from "node:fs";
import { effects } from "@efct/efct";

effects("file.read", "throw")(function readText(path: string): string {
  return readFileSync(path, "utf8");
});
```

Do not mix strong declarations and strings in one function contract.

## Check a project

```console
npx efct check src/math.ts
npx efct check src/math.ts --json
```

The entry file and its relative ESM import closure are checked as one project. Relative imports may cross verified modules; cycles, missing modules, inferred cross-module contracts, unknown packages, and unsupported syntax fail explicitly.

Exit status is `0` for an accepted project, `1` for verification diagnostics, and `2` for invalid usage or startup failure.

## Run verified code

Direct `node file.ts` execution is intentionally rejected by `defineModule`. Use `efct run` so verification finishes before any application module is evaluated:

```console
npx efct run src/math.ts
npx efct run src/math.ts --call add --args '[20, 22]'
```

The runner loads the verified in-memory source snapshot through Node's synchronous module hooks. TypeScript is erased with the pinned compiler, local imports use the verified resolution graph, and each module is sealed against its runtime plan. Exported wrappers enforce exact argument counts, primitive argument types, and return types before values cross the verified boundary.

Verified execution must start as a clean Node process. `NODE_OPTIONS`, preload modules, and custom loader flags are rejected because code that runs before Efct could replace the runtime identities being certified.

`--args` accepts a JSON array, so it can represent the 0.1 JSON-compatible primitive boundary. A successful call prints its JSON result; a `void` result prints `undefined`.

## Supported 0.1 subset

- TypeScript `.ts` and checked JavaScript `.js`/`.mjs`, ESM only
- exact `undefined`, `null`, `boolean`, `number`, `bigint`, `string`, `void`, and one nullish optional member
- named function expressions declared directly inside one `defineModule` call
- static primitive module constants
- primitive local variables, reassignment, and conditional expressions
- `if`, `while`, `return`, `throw`, unbound `catch`, `finally`, arithmetic, strict comparisons, and direct calls
- same-module calls and relative imports between verified modules
- controlled clock, random, console, environment, file, and process APIs
- `Throw` and `Diverge` partial propagation

Anything outside this closed subset is rejected. That is a certification boundary, not a claim that the JavaScript feature itself is unsafe.

## License

MIT
