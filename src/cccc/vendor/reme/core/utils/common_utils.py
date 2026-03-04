"""Minimal common utils used by vendored file-store/chunking subset."""

from __future__ import annotations

import hashlib
from typing import Iterable


def hash_text(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def _dot(a: Iterable[float], b: Iterable[float]) -> float:
    return sum(x * y for x, y in zip(a, b))


def _norm(a: Iterable[float]) -> float:
    return sum(x * x for x in a) ** 0.5


def batch_cosine_similarity(matrix_a: list[list[float]], matrix_b: list[list[float]]) -> list[list[float]]:
    """Pure-python cosine similarity matrix.

    Returned shape: (len(matrix_a), len(matrix_b))
    """
    out: list[list[float]] = []
    norms_b = [_norm(v) for v in matrix_b]
    for va in matrix_a:
        na = _norm(va)
        row: list[float] = []
        for vb, nb in zip(matrix_b, norms_b):
            den = na * nb
            row.append(0.0 if den == 0 else (_dot(va, vb) / den))
        out.append(row)
    return out
