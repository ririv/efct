# 结算策略示例

[English](README.md)

这个示例为一笔样例订单计算账单。税区来自环境变量，促销资格依赖时钟，最终结果输出到控制台。Efct 会验证这些外部依赖始终位于声明过的边界上，而计价规则保持确定性。

## 运行示例

在仓库根目录中检查完整示例，但不执行程序：

```console
efct check examples/checkout
```

没有输出且退出状态为零，表示所有受检契约均已通过。

运行经过验证的入口：

```console
efct run examples/checkout/main.py
```

默认税区为 `standard`：

```text
Order: EFCT-MUG
Region: standard
Subtotal (cents): 7500
Discount (cents): 750
Tax (cents): 556
Total (cents): 7306
```

设置 `EFCT_CHECKOUT_REGION` 可以测试另一个显式输入边界：

```console
EFCT_CHECKOUT_REGION=reduced efct run examples/checkout/main.py
```

## 程序结构

示例包含两个模块：

- `adapters.py` 在显式效果契约下读取环境变量和时钟。
- `main.py` 定义不可变订单和账单记录、应用纯计价规则、声明控制台输出，并组合完整程序。

适配器函数把外部值转换为普通的不可变数据：

```python
from efct import effect, effects


@effects(effect.Environment())
def checkout_region() -> str:
    return os.getenv("EFCT_CHECKOUT_REGION", "standard")


@effects(effect.Clock())
def current_time_ns() -> int:
    return time.time_ns()
```

计价函数接收这些值，不会自行读取外部世界：

```python
from efct import pure


@pure()
def calculate_invoice(
    order: Order,
    region: str,
    current_time: int,
) -> Invoice:
    ...
```

价格使用整数分。税费与折扣运算使用静态非零的整数除数，因此可以证明 `@pure()` 所声明的显式空部分行为上界。

程序边界列出了所有外部效果和可观察的部分行为：

```python
from efct import effect, effects, partial


@effects(
    effect.Console(),
    effect.Clock(),
    effect.Environment(),
    partial.Raise(OSError),
    partial.Raise(ValueError),
)
def run() -> None:
    ...
```

## Efct 能发现什么

假设有人把时钟访问移入一个声明为显式纯的函数：

```python
import time

from efct import pure


@pure()
def promotion_active() -> bool:
    return time.time_ns() < 4102444800000000000
```

`efct check` 会拒绝它。省略文件位置后，核心诊断如下：

```text
P1001
Function promotion_active contains undeclared effect clock
Effect source:
  Call time.time_ns
Suggestion: Declare @efct.effects("clock") or remove the effectful operation
```

把装饰器改成 `@effects(effect.Clock())` 可以准确描述一个有时钟效果的函数。这个示例选择保持计价规则纯净：`current_time_ns()` 只在边界读取一次时钟，再把所得整数显式传入 `calculate_invoice(...)`。

税区遵循相同结构。在计价核心中调用 `os.getenv(...)` 会引入未声明的 `environment` 效果；把税区作为 `str` 传入，既能保持计算的确定性，也让调用者和测试明确看到这项依赖。
