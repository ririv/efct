# Efct

[English](https://github.com/ririv/efct/blob/main/README.md)

Efct 是面向一个封闭、可审计 Python 子集的静态效果与纯函数验证器。它检查函数可能观察、修改、抛出或无法正常返回的行为，并拒绝无法证明安全的代码。

Efct 目前处于 Alpha 阶段，支持 CPython 3.13 和 3.14。

## 安装

从 PyPI 安装 Efct：

```console
python -m pip install efct
```

使用 uv：

```console
uv add efct
```

确认安装结果：

```console
efct --version
```

## 快速开始

使用 `@efct.pure` 标记函数：

```python
import efct


@efct.pure
def add(left: int, right: int) -> int:
    return left + right


assert add(2, 3) == 5
```

模块导入时，Efct 会验证源码、类型、调用和效果。装饰成功后得到经过验证的 `PureFunction`；验证失败会抛出 `EfctStartupError` 并阻止模块完成导入。

也可以在不导入模块的情况下检查文件：

```console
efct check app.py
efct check src/
```

Efct 会拒绝未知语法、未知调用、跨函数边界的可变值，以及超出声明上界的行为，不会把未经验证的代码当作纯代码。

## 纯函数与部分行为

Efct 将外部效果与部分行为分开：

- 外部效果表示与外部世界交互或依赖外部状态，例如文件、网络、时钟、随机数、环境变量、进程和全局状态。
- 部分行为表示计算可能无法正常返回，例如抛出异常或发散。

`pure` 的三种写法具有不同语义：

| 声明 | 外部效果 | 部分行为 |
| --- | --- | --- |
| `@efct.pure` | 禁止 | 自动推导并传播 |
| `@efct.pure()` | 禁止 | 显式为空 |
| `@efct.pure(...)` | 禁止 | 显式白名单 |

实现函数时可以使用裸装饰器：

```python
@efct.pure
def parse(value: str) -> int:
    if value == "":
        raise ValueError("empty")
    return 1
```

Efct 会推导 `Raise(ValueError)` 并将它传播给调用者。

在稳定 API 边界上使用显式白名单：

```python
@efct.pure(efct.partial.Raise(ValueError))
def parse(value: str) -> int:
    if value == "":
        raise ValueError("empty")
    return 1
```

如果函数不得包含任何已建模部分行为，并且必须证明终止，使用空括号：

```python
@efct.pure()
def increment(value: int) -> int:
    return value + 1
```

Efct 当前建模以下部分行为：

- `efct.partial.Raise(ExceptionType)`
- `efct.partial.RaiseGroup(ExceptionType)`
- `efct.partial.Diverge()`

显式声明是行为上界。函数可以产生比声明更少的行为，但不能产生未声明的行为。

## 声明效果

函数需要与外部世界交互时，使用 `@efct.effects(...)`：

```python
import efct


@efct.effects(
    efct.effect.Console(),
    efct.partial.Raise(OSError),
    efct.partial.Raise(ValueError),
)
def show(value: int) -> None:
    print(value)
```

`print` 具有控制台效果，并存在已建模的 `OSError` 和 `ValueError` 失败路径，因此这三种行为都属于公开上界。

可用的外部效果声明包括：

- `efct.effect.Console()`
- `efct.effect.File.Read()` 和 `efct.effect.File.Write()`
- `efct.effect.Network()`
- `efct.effect.Clock()`
- `efct.effect.Random()`
- `efct.effect.Environment()`
- `efct.effect.Process()`
- `efct.effect.State.Read()` 和 `efct.effect.State.Write()`
- `efct.effect.Unsafe()`

稳定字符串形式继续可用：

```python
@efct.effects("console", "raise:builtins.OSError", "raise:builtins.ValueError")
def show(value: int) -> None:
    print(value)
```

同一个装饰器中不能混用强类型声明和字符串声明。

## 处理异常

被完整处理的异常会从向外传播的部分行为上界中移除：

```python
@efct.pure()
def item_or_zero(values: tuple[int, ...], index: int) -> int:
    try:
        return values[index]
    except IndexError:
        return 0
```

Efct 会跟踪异常继承、有序处理器、`except ... as error`、精确重新抛出、`try ... else`、`finally`、`contextlib.suppress`、异常组以及受支持的 `except*` 控制流。未匹配的异常会继续向外传播。

对于可预期的业务失败，优先使用 `Result` 数据：

```python
@efct.pure
def parse(value: str) -> efct.Result[int, str]:
    if value == "":
        return efct.Err("empty")
    return efct.Ok(1)
```

`Result` 支持对 `Ok` 和 `Err` 进行穷尽 `match`。

## 纯记录

同一个装饰器可以验证深度不可变记录：

```python
from dataclasses import dataclass

import efct


@efct.pure
@dataclass(frozen=True, slots=True)
class Point:
    x: int
    y: int
```

跨越已验证函数边界的值必须属于 Efct 支持的不可变值模型。

## 高阶函数

`PureCallable` 接受具有显式空效果与空部分行为上界的已验证函数：

```python
@efct.pure()
def apply(
    function: efct.PureCallable[[int], int],
    value: int,
) -> int:
    return function(value)
```

效果泛型函数可以保留回调的具体效果：

```python
@efct.effects
def apply_effect[E: efct.EffectSet](
    function: efct.EffectCallable[[int], int, E],
    value: int,
) -> int:
    return function(value)
```

效果变量 `E` 会在每个调用点根据回调证书完成实例化。

## 模块初始化

函数装饰器不会自动认证任意模块顶层代码。使用保留名称 `_efct` 声明模块初始化契约：

```python
import efct

_efct = efct.pure
```

有外部效果的初始化必须声明显式上界：

```python
_efct = efct.effects(
    efct.effect.Console(),
    efct.partial.Raise(OSError),
    efct.partial.Raise(ValueError),
)
```

没有 `_efct` 时模块仍然可以运行，但 Efct 不会对其初始化效果作出保证。

## 命令行

```console
efct check app.py
efct check src/
efct check src/ --format=json
efct check src/ --verified-only
efct check src/ --deny-unsafe
efct trust fingerprint pytest
efct run app.py
efct --version
```

`efct check` 只分析代码，不执行代码。退出码为零表示报告所描述的源码快照通过验证。

`efct run` 会先验证入口及其本地依赖闭包，再执行已经验证的源码快照。源码、依赖或 Python 运行时变化后必须重新检查。

## 外部库

第三方符号不会被自动视为纯函数。迁移尚未完成时，必须显式声明未经审计的边界：

```toml
schema = 1

[[symbol]]
trust = "unsafe"
path = "legacy.math.clamp"
signature = "(int, int, int) -> int"
effects = []
partials = []
reason = "the implementation has not been audited"
```

调用 `unsafe` 边界会产生 `unsafe` 效果。CI 可以使用 `--deny-unsafe` 拒绝这类调用。

Audited 边界会把公开契约绑定到精确的已安装 distribution 以及 callable 的真实实现：

```toml
schema = 1

[python]
implementation = "cpython"
version = "3.14.7"
cache_tag = "cpython-314"

[[distribution]]
name = "ourlib"
version = "1.4.2"
installation_sha256 = "..."
dependencies = []

[[symbol]]
trust = "audited"
path = "ourlib.math.clamp"
owner = "ourlib"
implementation = { kind = "python", path = "ourlib.math.clamp" }
signature = "(int, int, int) -> int"
effects = []
partials = []
```

不要手工计算安装摘要，使用：

```console
efct trust fingerprint ourlib
efct trust fingerprint ourlib --format=json
```

`dependencies` 必须列出本次审计使用的每个直接依赖，并为每个依赖提供对应的 `[[distribution]]`。原生扩展函数使用 `kind = "native"`。Efct 会拒绝过期摘要、缺失的安装记录、editable 安装、未声明的依赖引用、错误的模块归属、被替换的运行时 callable，以及与已哈希安装源码不一致的 Python 代码对象。`audited` 失败时绝不会回退为 `unsafe`。

`path` 是应用代码使用的公开导出，`implementation.path` 是实际实现函数，因此合法 re-export 可以指向已声明依赖。`effects` 只能放外部效果；`partials` 用于 `raise:...`、`raise-group:...` 和 `diverge`。

## 运行期约束

经过验证的包装器会在调用时执行证书约束：

- `EfctContractError` 表示参数契约不满足。
- `EfctIntegrityError` 表示代码、绑定、依赖或返回值与证书不一致。
- `EfctStartupError` 表示装饰期间无法读取源码或验证失败。

参数和返回类型采用精确匹配。已验证依赖被重新绑定时，证书会失效，而不是静默扩宽边界。Audited distribution 的内容在证书签发时检查，后续调用会复验已加载对象和 Python 代码对象的身份。

## 支持的 Python 边界

Efct 有意只支持严格的 Python 子集。常用的受支持能力包括不可变基础值、元组、`frozenset`、`FrozenMap`、可选值、`Result`、局部控制流、有限迭代、经过分析的 `while`、受限局部列表、静态导入、已验证跨模块调用和已登记的标准库操作。

动态导入、反射、动态修改（monkey patch）、跨边界的普通可变对象、未知第三方调用和未支持的上下文管理器会被拒绝。Efct 是验证器，不是 Python 沙箱。

## 许可证

Efct 使用 [MIT License](LICENSE)。
