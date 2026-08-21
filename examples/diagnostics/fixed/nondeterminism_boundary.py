import random
import time

from efct import effect, effects, partial


@effects(
    effect.Clock(),
    effect.Random(),
    partial.Raise(ValueError),
)
def session_marker(low: int, high: int) -> tuple[int, int]:
    return (time.time_ns(), random.randint(low, high))
