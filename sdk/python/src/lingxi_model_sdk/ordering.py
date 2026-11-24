"""Deterministic digit-aware ordering matching ModelCatalogOrdering."""

from __future__ import annotations

from typing import Iterable

from .types import CatalogModel, CatalogProvider


def natural_key(text: str) -> tuple[tuple[int, int, str, str], ...]:
    """Build a total-order key: numbers (0, len, trimmed, raw) before text (1, 0, lower, '')."""
    tokens: list[tuple[int, int, str, str]] = []
    digits: list[str] = []
    words: list[str] = []

    def flush_digits() -> None:
        if not digits:
            return
        raw = "".join(digits)
        trimmed = raw.lstrip("0") or "0"
        tokens.append((0, len(trimmed), trimmed, raw))
        digits.clear()

    def flush_words() -> None:
        if not words:
            return
        tokens.append((1, 0, "".join(words).lower(), ""))
        words.clear()

    for ch in text:
        if "0" <= ch <= "9":
            flush_words()
            digits.append(ch)
        else:
            flush_digits()
            words.append(ch)
    flush_digits()
    flush_words()
    return tuple(tokens)


class _ReverseDate:
    __slots__ = ("value",)

    def __init__(self, value: str) -> None:
        self.value = value

    def __lt__(self, other: "_ReverseDate") -> bool:
        return self.value > other.value

    def __eq__(self, other: object) -> bool:
        return isinstance(other, _ReverseDate) and self.value == other.value


def sort_providers(
    providers: Iterable[CatalogProvider], featured: Iterable[str] = ()
) -> list[CatalogProvider]:
    ranks = {pid.strip().lower(): idx for idx, pid in enumerate(featured)}
    max_rank = len(ranks) + 1
    return sorted(
        providers,
        key=lambda p: (
            ranks.get(p.id.strip().lower(), max_rank),
            natural_key(p.name),
            p.id,
        ),
    )


def models_in_stable_order(models: Iterable[CatalogModel]) -> list[CatalogModel]:
    return sorted(
        models,
        key=lambda m: (
            0 if m.release_date else 1,
            _ReverseDate(m.release_date or ""),
            natural_key(m.family or ""),
            natural_key(m.name),
            m.id,
        ),
    )
