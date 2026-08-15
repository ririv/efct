import efct


@efct.effects("raise:builtins.ValueError")
def reject(message: str) -> None:
    raise ValueError(message)


@efct.pure
def recover(message: str) -> int:
    try:
        reject(message)
    except ValueError:
        return 0
    return 1


@efct.pure()
def exception_message(message: str) -> str:
    try:
        raise ValueError(message)
    except ValueError as error:
        return str(error)


@efct.pure()
def selected_exception_message(use_value_error: bool) -> str:
    try:
        if use_value_error:
            raise ValueError("value")
        raise TypeError("type")
    except (ValueError, TypeError) as error:
        return str(error)


@efct.pure(efct.partial.Raise(TypeError))
def chained_error(message: str) -> None:
    try:
        raise ValueError(message)
    except ValueError as error:
        raise TypeError("wrapped") from error


@efct.pure(efct.partial.Raise(TypeError))
def suppressed_context(message: str) -> None:
    try:
        raise ValueError(message)
    except ValueError:
        raise TypeError("wrapped") from None


@efct.pure()
def recover_chained_error(message: str) -> str:
    try:
        chained_error(message)
    except TypeError as error:
        return str(error)
    return message


@efct.pure()
def recover_suppressed_context(message: str) -> str:
    try:
        suppressed_context(message)
    except TypeError as error:
        return str(error)
    return message


@efct.effects("console", "raise:builtins.OSError", "raise:builtins.ValueError")
def log(message: str) -> None:
    print(message)


@efct.pure()
def identity(value: int) -> int:
    return value


@efct.pure()
def unreachable_handler() -> int:
    try:
        return identity(1)
    except ValueError:
        log("unreachable")
        return 0


@efct.pure(efct.partial.Raise(IndexError))
def reraised_item(values: tuple[int, ...], index: int) -> int:
    try:
        return values[index]
    except LookupError:
        raise


@efct.pure()
def recover_reraised_item(values: tuple[int, ...], index: int) -> int:
    try:
        return reraised_item(values, index)
    except IndexError:
        return 0


@efct.pure()
def item_or_zero(values: tuple[int, ...], index: int) -> int:
    try:
        value = values[index]
    except IndexError:
        return 0
    else:
        return value


@efct.pure(efct.partial.Raise(TypeError))
def overridden_by_finally() -> None:
    try:
        raise ValueError("value")
    finally:
        raise TypeError("cleanup")


@efct.pure()
def recover_finally_override() -> int:
    try:
        overridden_by_finally()
    except TypeError:
        return 0
    return 1


@efct.pure(efct.partial.Raise(ValueError))
def reraised_by_finally() -> None:
    try:
        raise ValueError("pending")
    finally:
        raise


@efct.pure()
def recover_finally_rethrow() -> str:
    try:
        reraised_by_finally()
    except ValueError as error:
        return str(error)
    return "missing"


@efct.pure(efct.partial.Raise(RuntimeError))
def raise_without_current_exception() -> None:
    try:
        pass
    finally:
        raise


@efct.pure()
def recover_missing_current_exception() -> str:
    try:
        raise_without_current_exception()
    except RuntimeError as error:
        return str(error)
    return "missing"


@efct.pure(efct.partial.Raise(ValueError))
def rethrow_enclosing_handler_exception() -> None:
    try:
        raise ValueError("outer")
    except ValueError:
        try:
            pass
        finally:
            raise
