import os
import time

from efct import effect, effects, pure

_efct = pure


@effects(effect.Environment())
def checkout_region() -> str:
    return os.getenv("EFCT_CHECKOUT_REGION", "standard")


@effects(effect.Clock())
def current_time_ns() -> int:
    return time.time_ns()
