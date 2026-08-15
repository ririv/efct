import http.client

import efct


@efct.pure
def connect(host: str) -> None:
    http.client.HTTPConnection(host)
