import efct


@efct.pure()
def recover() -> int:
    try:
        raise ExceptionGroup(
            "errors",
            (ValueError("value"), TypeError("type")),
        )
    except* ValueError:
        pass
    except* TypeError:
        pass
    return 1


@efct.pure()
def recover_nested() -> int:
    try:
        raise ExceptionGroup(
            "outer",
            (
                ValueError("value"),
                ExceptionGroup("inner", (TypeError("type"),)),
            ),
        )
    except* (ValueError, TypeError):
        pass
    return 1


@efct.pure()
def recover_naked() -> int:
    try:
        raise ValueError("value")
    except* ValueError:
        pass
    return 1


@efct.pure()
def recover_whole_group() -> int:
    try:
        raise ExceptionGroup("errors", (ValueError("value"),))
    except ExceptionGroup:
        return 1


@efct.pure(efct.partial.RaiseGroup(TypeError))
def preserve_unmatched() -> None:
    try:
        raise ExceptionGroup(
            "errors",
            (ValueError("value"), TypeError("type")),
        )
    except* ValueError:
        pass
