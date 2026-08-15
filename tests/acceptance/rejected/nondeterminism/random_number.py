import efct
import random


@efct.pure
def random_value() -> int:
    return random.randint(0, 100)
