import efct
import random


@efct.effects("random")
def sample(low: int, high: int) -> int:
    return random.randint(low, high)
