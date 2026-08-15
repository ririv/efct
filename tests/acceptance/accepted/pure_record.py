from dataclasses import dataclass

import efct


@efct.pure
@dataclass(frozen=True, slots=True)
class Point:
    x: int
    y: int


@efct.pure
def shift(point: Point, amount: int) -> Point:
    return Point(point.x + amount, point.y + amount)
