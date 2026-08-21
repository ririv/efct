from efct import effect, effects, partial, pure


@pure()
def increment(value: int) -> int:
    return value + 1


@effects(
    effect.Console(),
    partial.Raise(OSError),
    partial.Raise(ValueError),
)
def show(value: int) -> None:
    print(increment(value))
