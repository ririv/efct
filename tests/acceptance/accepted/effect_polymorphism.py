import efct


@efct.effects
def apply[E: efct.EffectSet](
    function: efct.EffectCallable[[int], int, E],
    value: int,
) -> int:
    return function(value)


@efct.pure()
def increment(value: int) -> int:
    return value + 1


@efct.effects("console", "raise:builtins.OSError", "raise:builtins.ValueError")
def show(value: int) -> int:
    print(value)
    return value


@efct.pure
def pure_run(value: int) -> int:
    return apply(increment, value)


@efct.effects("console", "raise:builtins.OSError", "raise:builtins.ValueError")
def effect_run(value: int) -> int:
    return apply(show, value)
