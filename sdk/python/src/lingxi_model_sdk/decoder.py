"""Decoder for published schema 1.x / 2.x catalog JSON documents."""

from __future__ import annotations

import json
from datetime import datetime, timezone
from typing import Any

from .catalog import LingXiModelCatalog
from .errors import InvalidCatalogError, UnsupportedSchemaVersionError
from .ordering import models_in_stable_order, sort_providers
from .types import (
    CatalogModel,
    CatalogModelStatus,
    CatalogProvider,
    CatalogRevision,
    ModelCapabilities,
    ModelLimits,
    ModelPricing,
)

SUPPORTED_MAJOR_VERSIONS = {"1", "2"}


def _non_empty(value: Any) -> str | None:
    if not isinstance(value, str):
        return None
    return value if value.strip() else None


def _opt_str(value: Any) -> str | None:
    return value if isinstance(value, str) else None


def _opt_int(value: Any) -> int | None:
    if isinstance(value, bool):
        return None
    if isinstance(value, int):
        return value
    if isinstance(value, float) and value.is_integer():
        return int(value)
    return None


def _opt_float(value: Any) -> float | None:
    if isinstance(value, bool):
        return None
    if isinstance(value, (int, float)):
        return float(value)
    return None


def _opt_bool(value: Any) -> bool | None:
    return value if isinstance(value, bool) else None


def _str_tuple(value: Any) -> tuple[str, ...]:
    if not isinstance(value, list):
        return ()
    return tuple(item for item in value if isinstance(item, str))


def _parse_timestamp(text: str | None) -> datetime | None:
    if not text:
        return None
    try:
        if text.endswith("Z"):
            return datetime.fromisoformat(text[:-1] + "+00:00")
        dt = datetime.fromisoformat(text)
        return dt if dt.tzinfo is not None else dt.replace(tzinfo=timezone.utc)
    except ValueError:
        return None


def _decode_model(provider_id: str, key: str, fields: dict[str, Any]) -> CatalogModel | None:
    model_id = _non_empty(key) or _non_empty(fields.get("id"))
    if model_id is None:
        return None

    limits_obj = fields.get("limit") if isinstance(fields.get("limit"), dict) else {}
    cost_obj = fields.get("cost") if isinstance(fields.get("cost"), dict) else {}
    mod_obj = fields.get("modalities") if isinstance(fields.get("modalities"), dict) else {}

    in_mods = _str_tuple(mod_obj.get("input"))
    out_mods = _str_tuple(mod_obj.get("output"))
    attachment = fields.get("attachment") is True
    vision = attachment or any(m.lower().startswith("image") for m in in_mods)

    efforts: list[str] = []
    raw_opts = fields.get("reasoning_options")
    if isinstance(raw_opts, list):
        for opt in raw_opts:
            if isinstance(opt, dict):
                efforts.extend(_str_tuple(opt.get("values")))

    return CatalogModel(
        id=model_id,
        provider_id=provider_id,
        name=_opt_str(fields.get("name")) or model_id,
        family=_non_empty(fields.get("family")),
        overview=_non_empty(fields.get("description")),
        release_date=_non_empty(fields.get("release_date")),
        last_updated=_non_empty(fields.get("last_updated")),
        knowledge_cutoff=_non_empty(fields.get("knowledge")),
        canonical_model_id=_non_empty(fields.get("canonical_model_id")),
        status=CatalogModelStatus.from_raw(_opt_str(fields.get("status"))),
        limits=ModelLimits(
            context=_opt_int(limits_obj.get("context")),
            input=_opt_int(limits_obj.get("input")),
            output=_opt_int(limits_obj.get("output")),
        ),
        capabilities=ModelCapabilities(
            reasoning=fields.get("reasoning") is True,
            reasoning_efforts=tuple(efforts),
            tool_calling=fields.get("tool_call") is True,
            structured_output=fields.get("structured_output") is True,
            attachments=attachment,
            vision=vision,
            audio_input=any(m.lower().startswith("audio") for m in in_mods),
            audio_output=any(m.lower().startswith("audio") for m in out_mods),
            input_modalities=in_mods,
            output_modalities=out_mods,
            open_weights=fields.get("open_weights") is True,
            supports_temperature=_opt_bool(fields.get("temperature")),
        ),
        pricing=ModelPricing(
            input=_opt_float(cost_obj.get("input")),
            output=_opt_float(cost_obj.get("output")),
            cache_read=_opt_float(cost_obj.get("cache_read")),
            cache_write=_opt_float(cost_obj.get("cache_write")),
            reasoning=_opt_float(cost_obj.get("reasoning")),
            input_audio=_opt_float(cost_obj.get("input_audio")),
            output_audio=_opt_float(cost_obj.get("output_audio")),
        ),
        fields=dict(fields),
    )


def _decode_provider(key: str, fields: dict[str, Any]) -> CatalogProvider | None:
    raw_models = fields.get("models") if isinstance(fields.get("models"), dict) else {}
    models: list[CatalogModel] = []
    for mid, raw_model in raw_models.items():
        if not isinstance(raw_model, dict):
            continue
        decoded = _decode_model(key, mid, raw_model)
        if decoded is not None:
            models.append(decoded)
    if not models and "id" not in fields:
        return None

    return CatalogProvider(
        id=_opt_str(fields.get("id")) or key,
        name=_opt_str(fields.get("name")) or key,
        base_url=_non_empty(fields.get("baseURL")),
        documentation_url=_non_empty(fields.get("doc")),
        environment_variable_names=_str_tuple(fields.get("env")),
        models=tuple(models_in_stable_order(models)),
        fields=dict(fields),
    )


def decode_catalog(data: bytes | str) -> LingXiModelCatalog:
    raw_bytes = data.encode("utf-8") if isinstance(data, str) else data
    try:
        root = json.loads(raw_bytes.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise InvalidCatalogError(f"Invalid JSON: {exc}") from exc

    if not isinstance(root, dict):
        raise InvalidCatalogError("Catalog root is not an object")

    providers_obj = root.get("providers")
    if not isinstance(providers_obj, dict) or not providers_obj:
        raise InvalidCatalogError("providers missing or empty")

    schema_version = _opt_str(root.get("schemaVersion")) or _opt_str(root.get("version")) or "1.0"
    major = schema_version.split(".", 1)[0]
    if major not in SUPPORTED_MAJOR_VERSIONS:
        raise UnsupportedSchemaVersionError(schema_version)

    generated_at = _parse_timestamp(_opt_str(root.get("generatedAt")) or _opt_str(root.get("updatedAt")))

    providers: list[CatalogProvider] = []
    for pid, raw_prov in providers_obj.items():
        if not isinstance(raw_prov, dict):
            continue
        prov = _decode_provider(pid, raw_prov)
        if prov is not None:
            providers.append(prov)
    if not providers:
        raise InvalidCatalogError("No valid provider entries")

    sorted_providers = tuple(sort_providers(providers))
    cat_rev = (
        _opt_str(root.get("catalogRevision"))
        or _opt_str(root.get("catalogHash"))
        or (generated_at.isoformat() if generated_at else "")
    )
    total_providers = _opt_int(root.get("totalProviders")) or len(sorted_providers)
    total_models = _opt_int(root.get("totalModels")) or sum(p.model_count for p in sorted_providers)

    revision = CatalogRevision(
        schema_version=schema_version,
        catalog_revision=cat_rev,
        catalog_hash=_opt_str(root.get("catalogHash")),
        source=_opt_str(root.get("source")),
        source_url=_opt_str(root.get("sourceURL")),
        source_hash=_opt_str(root.get("sourceHash")),
        generated_at=generated_at,
        source_fetched_at=_parse_timestamp(_opt_str(root.get("sourceFetchedAt"))),
        total_providers=total_providers,
        total_models=total_models,
    )
    return LingXiModelCatalog(revision=revision, providers=sorted_providers, raw_document=raw_bytes)
