"""Catalog data types preserving unknown fields and three-state semantics."""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime
from enum import Enum
from typing import Any


class CatalogModelStatus(str, Enum):
    ACTIVE = "active"
    PREVIEW = "preview"
    DEPRECATED = "deprecated"
    RETIRED = "retired"
    UNKNOWN = "unknown"

    @classmethod
    def from_raw(cls, raw: str | None) -> "CatalogModelStatus":
        if raw in ("deprecated", "retired"):
            return cls.DEPRECATED
        if raw in ("beta", "preview"):
            return cls.PREVIEW
        if raw == "active":
            return cls.ACTIVE
        return cls.UNKNOWN

    @property
    def is_selectable(self) -> bool:
        return self not in (CatalogModelStatus.DEPRECATED, CatalogModelStatus.RETIRED)


@dataclass(frozen=True)
class ModelLimits:
    context: int | None = None
    input: int | None = None
    output: int | None = None


@dataclass(frozen=True)
class ModelCapabilities:
    reasoning: bool = False
    reasoning_efforts: tuple[str, ...] = ()
    tool_calling: bool = False
    structured_output: bool = False
    attachments: bool = False
    vision: bool = False
    audio_input: bool = False
    audio_output: bool = False
    input_modalities: tuple[str, ...] = ()
    output_modalities: tuple[str, ...] = ()
    open_weights: bool = False
    supports_temperature: bool | None = None


@dataclass(frozen=True)
class ModelPricing:
    input: float | None = None
    output: float | None = None
    cache_read: float | None = None
    cache_write: float | None = None
    reasoning: float | None = None
    input_audio: float | None = None
    output_audio: float | None = None

    @property
    def is_unknown(self) -> bool:
        return (
            self.input is None
            and self.output is None
            and self.cache_read is None
            and self.cache_write is None
            and self.reasoning is None
            and self.input_audio is None
            and self.output_audio is None
        )


@dataclass(frozen=True)
class CatalogModel:
    id: str
    provider_id: str
    name: str
    family: str | None = None
    overview: str | None = None
    release_date: str | None = None
    last_updated: str | None = None
    knowledge_cutoff: str | None = None
    canonical_model_id: str | None = None
    status: CatalogModelStatus = CatalogModelStatus.UNKNOWN
    limits: ModelLimits = field(default_factory=ModelLimits)
    capabilities: ModelCapabilities = field(default_factory=ModelCapabilities)
    pricing: ModelPricing = field(default_factory=ModelPricing)
    fields: dict[str, Any] = field(default_factory=dict)

    @property
    def qualified_id(self) -> str:
        return f"{self.provider_id}/{self.id}"

    @property
    def context_window(self) -> int | None:
        return self.limits.context

    @property
    def max_output_tokens(self) -> int | None:
        return self.limits.output


@dataclass(frozen=True)
class CatalogProvider:
    id: str
    name: str
    base_url: str | None = None
    documentation_url: str | None = None
    environment_variable_names: tuple[str, ...] = ()
    models: tuple[CatalogModel, ...] = ()
    fields: dict[str, Any] = field(default_factory=dict)

    @property
    def model_count(self) -> int:
        return len(self.models)


@dataclass(frozen=True)
class CatalogRevision:
    schema_version: str
    catalog_revision: str
    catalog_hash: str | None = None
    source: str | None = None
    source_url: str | None = None
    source_hash: str | None = None
    generated_at: datetime | None = None
    source_fetched_at: datetime | None = None
    total_providers: int = 0
    total_models: int = 0
