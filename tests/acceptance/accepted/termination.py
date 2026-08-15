import efct


@efct.pure(efct.partial.Diverge())
def wait_forever() -> None:
    while True:
        pass


@efct.pure()
def finite_loop() -> int:
    while True:
        break
    return 1


@efct.pure(efct.partial.Diverge())
def countdown(value: int) -> int:
    if value == 0:
        return 0
    return countdown(value - 1)


@efct.pure()
def guarded_countdown(value: int) -> int:
    if value <= 0:
        return 0
    return guarded_countdown(value - 1)


@efct.pure()
def skip_unreachable_cycle() -> int:
    if False:
        return skip_unreachable_cycle()
    return 1
