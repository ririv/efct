import efct


@efct.pure(efct.partial.Raise(SystemExit))
def stop(message: str) -> None:
    raise SystemExit(message)
