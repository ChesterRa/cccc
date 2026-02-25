from __future__ import annotations

from dataclasses import dataclass


@dataclass
class NotebookLMProviderError(RuntimeError):
    code: str
    message: str
    transient: bool = False
    degrade_provider: bool = True

    def __post_init__(self) -> None:
        super().__init__(self.message)

