import random
import time

from efct import pure


@pure()
def session_marker(low: int, high: int) -> tuple[int, int]:
    return (time.time_ns(), random.randint(low, high))
