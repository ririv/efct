import efct
import time


@efct.pure
def now() -> int:
    return time.time_ns()
