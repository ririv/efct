import efct
from efct import effect, partial

public_version: str = efct.__version__

@efct.effects(
    effect.File.Read(),
    effect.File.Write(),
    effect.State.Read(),
    effect.State.Write(),
    partial.Raise(ValueError),
    partial.RaiseGroup(TypeError),
    partial.Diverge(),
)
def declared_effects() -> None:
    pass


typed_effect: effect.Effect = effect.Console()
typed_partial: partial.Partial = partial.Raise(ValueError)
typed_group_partial: partial.Partial = partial.RaiseGroup(TypeError)
typed_divergence_partial: partial.Partial = partial.Diverge()
string_contract = efct.effects("console", "network")
typed_contract = efct.effects(effect.Console(), effect.Network())
string_partial_contract = efct.pure("raise:builtins.ValueError")
typed_partial_contract = efct.pure(partial.Raise(ValueError))
empty_partial_contract = efct.pure()


@efct.pure(partial.Raise(ValueError))
def declared_partial(value: int) -> int:
    if value == 0:
        raise ValueError("zero")
    return value


@efct.pure()
def exact_pure(value: int) -> int:
    return value
