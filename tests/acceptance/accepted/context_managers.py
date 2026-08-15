import contextlib
from contextlib import suppress

import efct


@efct.pure()
def recover(message: str) -> int:
    with contextlib.suppress(ValueError):
        raise ValueError(message)
    return 1


@efct.pure()
def recover_lookup() -> int:
    with contextlib.suppress(LookupError):
        raise IndexError("index")
    return 1


@efct.pure()
def recover_imported() -> int:
    with suppress(ValueError):
        raise ValueError("value")
    return 1


@efct.pure(efct.partial.Raise(TypeError))
def preserve_unmatched() -> None:
    with contextlib.suppress(ValueError):
        raise TypeError("type")
