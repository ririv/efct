import urllib.request

import efct


@efct.pure
def request(url: str) -> None:
    urllib.request.urlopen(url)
