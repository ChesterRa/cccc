"""Runtime Python version check.

Ensures users on unsupported Python versions (< 3.10) get a clear error
message instead of a cryptic TypeError from PEP 604 union syntax (str | None).

See: https://github.com/teng-lin/notebooklm-py/issues/117
"""

import sys

MIN_VERSION = (3, 10)


def check_python_version():
    if sys.version_info[:2] < MIN_VERSION:
        sys.exit(
            f"notebooklm-py requires Python {MIN_VERSION[0]}.{MIN_VERSION[1]} or later. "
            f"You are using Python {sys.version_info[0]}.{sys.version_info[1]}.{sys.version_info[2]}.\n"
            f"Please upgrade: https://www.python.org/downloads/"
        )
