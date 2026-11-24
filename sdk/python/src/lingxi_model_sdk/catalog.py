"""In-memory catalog lookup, search, and filtering."""

from __future__ import annotations

from dataclasses import dataclass

from .ordering import models_in_stable_order, sort_providers
from .types import CatalogModel, CatalogProvider, CatalogRevision


def _norm(text: str) -> str:
    return text.strip().lower()


@dataclass(frozen=True)
class ModelFilter:
    provider_id: str | None = None
    query: str | None = None
    reasoning: bool | None = None
    tool_calling: bool | None = None
    structured_output: bool | None = None
    vision: bool | None = None
    attachments: bool | None = None
    open_weights: bool | None = None
    minimum_context_window: int | None = None
    minimum_max_output_tokens: int | None = None
    include_deprecated: bool = False
    featured_provider_ids: tuple[str, ...] = ()

    def matches(self, model: CatalogModel, provider: CatalogProvider) -> bool:
        if not self.include_deprecated and not model.status.is_selectable:
            return False
        if self.reasoning is not None and model.capabilities.reasoning != self.reasoning:
            return False
        if self.tool_calling is not None and model.capabilities.tool_calling != self.tool_calling:
            return False
        if self.structured_output is not None and model.capabilities.structured_output != self.structured_output:
            return False
        if self.vision is not None and model.capabilities.vision != self.vision:
            return False
        if self.attachments is not None and model.capabilities.attachments != self.attachments:
            return False
        if self.open_weights is not None and model.capabilities.open_weights != self.open_weights:
            return False
        if self.minimum_context_window is not None and (model.limits.context or 0) < self.minimum_context_window:
            return False
        if self.minimum_max_output_tokens is not None and (model.limits.output or 0) < self.minimum_max_output_tokens:
            return False
        if self.query is not None:
            q = _norm(self.query)
            if q:
                haystacks = (
                    model.id,
                    model.name,
                    model.family or "",
                    model.overview or "",
                    provider.id,
                    provider.name,
                )
                if not any(q in _norm(h) for h in haystacks):
                    return False
        return True


class LingXiModelCatalog:
    def __init__(
        self,
        revision: CatalogRevision,
        providers: tuple[CatalogProvider, ...],
        raw_document: bytes | None = None,
    ) -> None:
        self.revision = revision
        self.providers = providers
        self.raw_document = raw_document

    @classmethod
    def decoded(cls, data: bytes | str) -> "LingXiModelCatalog":
        from .decoder import decode_catalog

        return decode_catalog(data)

    @property
    def total_models(self) -> int:
        return sum(p.model_count for p in self.providers)

    def provider(self, provider_id: str) -> CatalogProvider | None:
        key = _norm(provider_id)
        for prov in self.providers:
            if _norm(prov.id) == key:
                return prov
        return None

    def model(self, id_or_provider: str, model_id: str | None = None) -> CatalogModel | None:
        if model_id is not None:
            prov = self.provider(id_or_provider)
            if prov is None:
                return None
            key = _norm(model_id)
            for m in prov.models:
                if _norm(m.id) == key:
                    return m
            return None

        if "/" in id_or_provider:
            pid, mid = id_or_provider.split("/", 1)
            return self.model(pid, mid)

        key = _norm(id_or_provider)
        for prov in self.providers:
            for m in prov.models:
                if _norm(m.id) == key:
                    return m
        return None

    def sorted_providers(self, featured: tuple[str, ...] | list[str] = ()) -> list[CatalogProvider]:
        return sort_providers(self.providers, featured)

    def models_for_provider(self, provider_id: str) -> list[CatalogModel]:
        prov = self.provider(provider_id)
        return models_in_stable_order(prov.models) if prov else []

    @property
    def all_models(self) -> list[CatalogModel]:
        result: list[CatalogModel] = []
        for prov in self.sorted_providers():
            result.extend(models_in_stable_order(prov.models))
        return result

    def search(self, query: str, limit: int | None = None) -> list[CatalogModel]:
        needle = _norm(query)
        if not needle:
            models = self.all_models
            return models[:limit] if limit is not None else models

        matches: list[CatalogModel] = []
        for prov in self.sorted_providers():
            ordered = models_in_stable_order(prov.models)
            if needle in _norm(prov.id) or needle in _norm(prov.name):
                matches.extend(ordered)
            else:
                for m in ordered:
                    if any(
                        needle in _norm(field)
                        for field in (m.id, m.name, m.family or "", m.overview or "")
                    ):
                        matches.append(m)
            if limit is not None and len(matches) >= limit:
                return matches[:limit]
        return matches[:limit] if limit is not None else matches

    def models_matching(self, filter_spec: ModelFilter) -> list[CatalogModel]:
        matches: list[CatalogModel] = []
        for prov in self.sorted_providers(filter_spec.featured_provider_ids):
            if filter_spec.provider_id is not None and _norm(prov.id) != _norm(filter_spec.provider_id):
                continue
            for m in models_in_stable_order(prov.models):
                if filter_spec.matches(m, prov):
                    matches.append(m)
        return matches
