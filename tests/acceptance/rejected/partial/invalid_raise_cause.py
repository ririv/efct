import efct


@efct.pure(efct.partial.Raise(ValueError))
def invalid() -> None:
    raise ValueError("primary") from 1  # pyright: ignore[reportGeneralTypeIssues]
