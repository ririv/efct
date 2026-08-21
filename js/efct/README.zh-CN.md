# Efct TypeScript 与 JavaScript 支持

[English](README.md)

Efct 验证函数是否遵守显式的纯度、外部效果与 partial 行为边界。遇到未知语法和未知调用时，它会明确拒绝，不会静默地把它们当成安全代码。

0.1 版本只认证 Node.js 24.19.0、TypeScript 5.9.3 和 ESM。TypeScript 与经过 `checkJs` 检查的 JavaScript 共用同一验证器。浏览器、Bun、CommonJS、异步函数、Promise、对象、数组、类、闭包和任意 npm 依赖不在此版本的认证范围内。

## 安装

```console
npm install @efct/efct
```

Efct 使用预编译的 Node-API 原生验证器。安装正式发布包时不需要在本机编译 Rust。

应用的 `package.json` 必须显式声明 ESM：

```json
{
  "type": "module"
}
```

## 定义受检函数

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

`pure()` 表示显式为空的 partial 白名单。`pure(partial.Throw())` 允许抛出，`pure(partial.Diverge())` 允许可能不终止。`effects(...)` 是外部效果白名单；如果函数还可能产生 partial 行为，也必须在同一声明中列出。

`pure(function named() {})` 与 `effects(function named() {})` 是推导简写。推导契约只在定义它的模块内部有效。可导入函数必须使用显式契约，避免跨模块边界在不知情的情况下变化。

生成代码和兼容场景仍可使用字符串声明：

```ts
import { readFileSync } from "node:fs";
import { effects } from "@efct/efct";

effects("file.read", "throw")(function readText(path: string): string {
  return readFileSync(path, "utf8");
});
```

同一个函数契约中不能混用强类型声明与字符串。

## 检查项目

```console
npx efct check src/math.ts
npx efct check src/math.ts --json
```

Efct 会把入口文件及其相对 ESM 导入闭包作为一个项目检查。受检模块之间可以使用相对导入；循环依赖、缺失模块、跨模块推导契约、未知包和不受支持语法都会明确失败。

退出码 `0` 表示项目通过，`1` 表示存在验证诊断，`2` 表示用法错误或启动失败。

## 运行受检代码

直接执行 `node file.ts` 会被 `defineModule` 有意拒绝。请使用 `efct run`，确保所有应用模块都在求值前完成验证：

```console
npx efct run src/math.ts
npx efct run src/math.ts --call add --args '[20, 22]'
```

运行器通过 Node 同步模块 hook 加载刚刚验证过的内存源码快照，使用固定 TypeScript 编译器擦除类型，按照已验证的解析图处理本地导入，并让每个模块与对应运行计划精确密封。导出包装器会在值跨越受检边界前检查精确参数数量、原始参数类型和返回类型。

验证执行必须从干净的 Node 进程开始。`NODE_OPTIONS`、预加载模块和自定义 loader 参数都会被拒绝，因为先于 Efct 运行的代码可以替换将要认证的运行时身份。

`--args` 接受 JSON 数组，因此可以表示 0.1 版本中与 JSON 兼容的原始值边界。调用成功时输出 JSON 结果；`void` 结果输出 `undefined`。

## 0.1 支持范围

- TypeScript `.ts` 与经过检查的 JavaScript `.js`/`.mjs`，仅 ESM
- 精确的 `undefined`、`null`、`boolean`、`number`、`bigint`、`string`、`void` 和单一空值可选类型
- 直接写在唯一 `defineModule` 调用中的命名函数表达式
- 静态原始值模块常量
- 原始值局部变量、重新绑定和条件表达式
- `if`、`while`、`return`、`throw`、无绑定 `catch`、`finally`、算术、严格比较和直接调用
- 同模块调用与受检模块之间的相对导入
- 受控的时钟、随机数、控制台、环境、文件和进程 API
- `Throw` 与 `Diverge` partial 传播

封闭子集之外的能力都会被拒绝。这表示认证边界，并不表示对应 JavaScript 特性本身不安全。

## 许可证

MIT
