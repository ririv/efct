import efct


@efct.pure()
def invalid(condition: bool) -> None:
    assert condition, "required"
