from __future__ import annotations

import sys
from typing import Optional

from .launcher import main as launcher_main


def main(argv: Optional[list[str]] = None) -> int:
    """Compatibility entrypoint that follows the selected implementation."""
    args = list(sys.argv[1:] if argv is None else argv)
    return int(launcher_main(["daemon", *args]))


if __name__ == "__main__":
    raise SystemExit(main())
