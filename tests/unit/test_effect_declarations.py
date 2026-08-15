import inspect

import efct
import pytest
from efct import effect, partial


def test_effect_parameters_describe_the_nonempty_declaration_shape() -> None:
    parameters = tuple(inspect.signature(efct.effects).parameters.values())

    assert [parameter.name for parameter in parameters] == [
        "first_effect",
        "additional_effects",
    ]
    assert parameters[0].kind is inspect.Parameter.POSITIONAL_ONLY
    assert parameters[1].kind is inspect.Parameter.VAR_POSITIONAL


def test_typed_effects_have_stable_string_names() -> None:
    declarations: tuple[tuple[effect.Effect, str], ...] = (
        (effect.Console(), "console"),
        (effect.File.Read(), "file.read"),
        (effect.File.Write(), "file.write"),
        (effect.Network(), "network"),
        (effect.Clock(), "clock"),
        (effect.Random(), "random"),
        (effect.Environment(), "environment"),
        (effect.Process(), "process"),
        (effect.State.Read(), "global.read"),
        (effect.State.Write(), "global.write"),
        (effect.Unsafe(), "unsafe"),
    )

    assert [
        effect._canonical_name(declaration) for declaration, _ in declarations
    ] == [name for _, name in declarations]


def test_effect_namespaces_cannot_be_instantiated() -> None:
    with pytest.raises(TypeError, match="Effect namespace File cannot be instantiated"):
        effect.File()
    with pytest.raises(TypeError, match="Effect namespace State cannot be instantiated"):
        effect.State()


def test_raise_requires_an_exception_class() -> None:
    with pytest.raises(TypeError, match="Raise requires an exception class"):
        partial.Raise(str)  # type: ignore[arg-type]


def test_typed_partials_have_stable_string_names() -> None:
    declaration = partial.Raise(ValueError)

    assert partial._canonical_name(declaration) == "raise:builtins.ValueError"


def test_raise_group_requires_an_exception_class() -> None:
    with pytest.raises(TypeError, match="RaiseGroup requires an Exception class"):
        partial.RaiseGroup(KeyboardInterrupt)  # type: ignore[arg-type]


def test_typed_exception_group_partial_has_stable_string_name() -> None:
    declaration = partial.RaiseGroup(ValueError)

    assert partial._canonical_name(declaration) == (
        "raise-group:builtins.ValueError"
    )


def test_typed_divergence_partial_has_stable_string_name() -> None:
    declaration = partial.Diverge()

    assert partial._canonical_name(declaration) == "diverge"
    assert partial._is_canonical_name("diverge")


def test_effect_declaration_forms_cannot_be_mixed() -> None:
    with pytest.raises(
        TypeError,
        match="String and typed effect declarations cannot be mixed",
    ):
        efct.effects("console", effect.Network())  # type: ignore[arg-type]

    with pytest.raises(
        TypeError,
        match="String and typed effect declarations cannot be mixed",
    ):
        efct.effects(effect.Console(), "network")  # type: ignore[arg-type]


def test_pure_contract_forms_are_distinct() -> None:
    assert callable(efct.pure)
    assert callable(efct.pure())
    assert callable(efct.pure(partial.Raise(ValueError)))


def test_pure_rejects_external_effect_declarations() -> None:
    with pytest.raises(
        TypeError,
        match="Pure arguments must be strings or typed partial declarations",
    ):
        efct.pure(effect.Console())  # type: ignore[arg-type]


def test_pure_partial_declaration_forms_cannot_be_mixed() -> None:
    with pytest.raises(
        TypeError,
        match="String and typed partial declarations cannot be mixed",
    ):
        efct.pure(
            "raise:builtins.ValueError",
            partial.Raise(ValueError),
        )  # type: ignore[arg-type]


def test_pure_string_declarations_must_describe_partial_behavior() -> None:
    with pytest.raises(
        TypeError,
        match="Pure string declarations must be supported partials",
    ):
        efct.pure("console")

    with pytest.raises(
        TypeError,
        match="Pure string declarations must be supported partials",
    ):
        efct.pure("raise:ConfigError")
