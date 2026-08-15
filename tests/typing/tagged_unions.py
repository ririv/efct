from __future__ import annotations

from typing import Optional

import efct


def accept_optional(value: int | None) -> None:
    pass


def accept_result(value: efct.Result[int, str]) -> None:
    pass


ok = efct.Ok(1)
err = efct.Err("failed")

accept_optional(1)
accept_optional(None)
accept_result(ok)
accept_result(err)

optional_from_value: int | None = 1
optional_from_none: int | None = None
optional_from_typing: Optional[int] = None
result_from_ok: efct.Result[int, str] = ok
result_from_err: efct.Result[int, str] = err
