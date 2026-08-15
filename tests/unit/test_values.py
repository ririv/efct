from __future__ import annotations

import efct
import pytest
from efct import _core


def test_result_cannot_express_conflicting_states() -> None:
    ok = efct.Ok("done")
    err = efct.Err("failed")

    assert isinstance(ok, efct.Result)
    assert isinstance(err, efct.Result)
    assert ok.value == "done"
    assert err.error == "failed"
    with pytest.raises((AttributeError, TypeError)):
        ok.value = "changed"


def test_result_marker_cannot_be_constructed_directly() -> None:
    with pytest.raises(TypeError, match="constructed with Ok or Err"):
        efct.Result()


def test_python_native_optional_replaces_the_custom_option_api() -> None:
    assert not hasattr(efct, "Option")
    assert not hasattr(efct, "Some")
    assert not hasattr(efct, "Nothing")


def test_frozen_map_rejects_duplicate_keys_and_mutable_values() -> None:
    assert efct.FrozenMap((("a", 1),))["a"] == 1
    with pytest.raises(ValueError, match="duplicate"):
        efct.FrozenMap((("a", 1), ("a", 2)))
    with pytest.raises(TypeError, match="pure values"):
        efct.FrozenMap((("a", []),))


def test_verified_function_wrappers_cannot_be_forged_directly() -> None:
    assert not hasattr(_core, "create_pure_function")
    assert not hasattr(_core, "create_effect_function")
    with pytest.raises(TypeError, match="only be constructed by the verifier"):
        efct.PureFunction(object())
    with pytest.raises(TypeError, match="only be constructed by the verifier"):
        efct.EffectFunction(object())


def test_pure_callable_can_only_be_used_as_a_type_marker() -> None:
    assert "efct.callables.PureCallable" in repr(efct.PureCallable[[int], int])
    with pytest.raises(TypeError, match="only be used in type annotations"):
        efct.PureCallable()


def test_public_effect_generic_types_can_only_be_used_as_markers() -> None:
    assert "efct.callables.EffectCallable" in repr(
        efct.EffectCallable[[int], int, object]
    )
    with pytest.raises(TypeError, match="only be used in type annotations"):
        efct.EffectCallable()
    with pytest.raises(TypeError, match="effect-generic parameter constraint"):
        efct.EffectSet()
