from socket import create_connection

import efct


@efct.pure
def connect(host: str) -> None:
    create_connection((host, 443))
