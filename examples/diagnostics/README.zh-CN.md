# Efct 诊断示例

[English](README.md)

这一组程序应当被 Efct 拒绝。它与能够通过检查的[结算示例](../checkout/README.zh-CN.md)互补，专门展示把行为隐藏在纯函数契约后会发生什么。

在仓库根目录运行一个拒绝案例：

```console
efct check examples/diagnostics/rejected/hidden_file_read.py
```

命令会以状态 `1` 退出。为了便于比较，下面的诊断摘录省略了文件位置。仓库中的[集成测试](../../tests/integration/examples/test_diagnostics.py)锁定了准确的诊断消息，而 [`fixed/`](fixed/) 下的所有修正版可以一起通过：

```console
efct check examples/diagnostics/fixed
```

## 隐藏文件读取

[`hidden_file_read.py`](rejected/hidden_file_read.py)声明了空的纯函数契约，随后打开一个路径：

```python
from efct import pure


@pure()
def probe_file(path: str) -> None:
    open(path)
```

Efct 会分别报告外部依赖及其可观察的失败方式：

```text
P1001 Function probe_file contains undeclared effect file.read
P1001 Function probe_file contains undeclared partial behavior raise:builtins.OSError
P1001 Function probe_file contains undeclared partial behavior raise:builtins.ValueError
```

[`file_boundary.py`](fixed/file_boundary.py)通过声明文件能力和两种异常修正契约：

```python
@effects(
    effect.File.Read(),
    partial.Raise(OSError),
    partial.Raise(ValueError),
)
def probe_file(path: str) -> None:
    open(path)
```

允许 `File.Read()` 不会隐式允许异常。效果与部分行为是两套相互独立的显式上界。

## 隐藏时钟与随机数

[`hidden_nondeterminism.py`](rejected/hidden_nondeterminism.py)取得两个无需改变实参也可能变化的值：

```python
@pure()
def session_marker(low: int, high: int) -> tuple[int, int]:
    return (time.time_ns(), random.randint(low, high))
```

Efct 会识别两个来源，以及 `random.randint` 在区间无效时可能产生的异常：

```text
P1001 Function session_marker contains undeclared effect clock
P1001 Function session_marker contains undeclared effect random
P1001 Function session_marker contains undeclared partial behavior raise:builtins.ValueError
```

[`nondeterminism_boundary.py`](fixed/nondeterminism_boundary.py)使用 `Clock()`、`Random()` 和 `Raise(ValueError)` 显式描述这条边界。另一种设计是把时间戳和采样值作为普通不可变参数传给确定性核心，结算示例对时钟值采用的正是这种方式。

## 未声明异常

[`undeclared_exception.py`](rejected/undeclared_exception.py)承诺部分行为上界为空，但函数中存在可达的 `raise`：

```python
@pure()
def require_non_negative(value: int) -> int:
    if value < 0:
        raise ValueError("value must be non-negative")
    return value
```

即使异常不会产生外部效果，它仍然属于函数契约：

```text
P1001 Function require_non_negative contains undeclared partial behavior raise:builtins.ValueError
```

[`exception_contract.py`](fixed/exception_contract.py)使用显式白名单：

```python
@pure(partial.Raise(ValueError))
def require_non_negative(value: int) -> int:
    ...
```

这个函数依然没有外部效果，但调用者可以明确看到它对于负数输入是偏函数。

## 未认证第三方依赖

[`uncertified_dependency.py`](rejected/uncertified_dependency.py)在受检代码中导入并调用 `requests`：

```python
import requests
from efct import pure


@pure()
def download(url: str) -> None:
    requests.get(url)
```

Efct 不会猜测未知包的行为：

```text
P1301 Imported module requests is not certified by the MVP
P1004 Value name requests cannot be resolved
```

这里有两种显式设计：

- 把集成代码留在受检纯核心之外，将不可变的响应数据传入经过验证的函数。[`third_party_input.py`](fixed/third_party_input.py)展示了边界内侧的写法。
- 审查实现后，通过 Efct 的[外部库信任清单](../../README.zh-CN.md#外部库)描述依赖。清单必须列出全部效果和部分行为；只修改装饰器不能认证一个未知实现。

## 无法证明终止

[`unproven_termination.py`](rejected/unproven_termination.py)声明了空的部分行为上界，但存在永远重复的路径：

```python
@pure()
def wait_forever() -> None:
    while True:
        pass
```

Efct 使用 `Diverge` 记录可能不终止：

```text
P1001 Function wait_forever contains undeclared partial behavior diverge
```

[`bounded_iteration.py`](fixed/bounded_iteration.py)把无限循环换成有限的 `range`。如果不终止本来就是预期语义，也可以声明 `@pure(partial.Diverge())`。这个声明是白名单，不是终止性证明：它只是准确允许发散，不会把无限循环变成全函数。
