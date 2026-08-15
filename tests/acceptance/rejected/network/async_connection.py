import asyncio

import efct


@efct.pure
def connect(host: str) -> None:
    asyncio.open_connection(host, 443)
