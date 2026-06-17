#!/usr/bin/env python3
"""No-network smoke checks for automatic local-model source routing."""

from __future__ import annotations

import importlib.util
import os
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[1]
SIDECAR_PATH = REPO_ROOT / "mlx-sidecar" / "cerul_mlx_sidecar.py"


def load_sidecar_module() -> Any:
    spec = importlib.util.spec_from_file_location("cerul_mlx_sidecar", SIDECAR_PATH)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"failed to load {SIDECAR_PATH}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def restore_env(previous: dict[str, str | None]) -> None:
    for key, value in previous.items():
        if value is None:
            os.environ.pop(key, None)
        else:
            os.environ[key] = value


def run_case(
    sidecar: Any,
    *,
    name: str,
    region: str | None,
    probes: dict[str, dict[str, Any]],
    expected_source: str,
) -> None:
    previous_env = {
        "CERUL_MODEL_DOWNLOAD_SOURCE": os.environ.get("CERUL_MODEL_DOWNLOAD_SOURCE"),
        "CERUL_MODEL_DOWNLOAD_REGION": os.environ.get("CERUL_MODEL_DOWNLOAD_REGION"),
    }
    original_probe_url_for_source = sidecar.probe_url_for_source
    original_probe_url = sidecar.probe_url
    original_write_prepare_status = sidecar.write_prepare_status
    statuses: list[dict[str, Any]] = []

    try:
        os.environ["CERUL_MODEL_DOWNLOAD_SOURCE"] = "auto"
        if region is None:
            os.environ.pop("CERUL_MODEL_DOWNLOAD_REGION", None)
        else:
            os.environ["CERUL_MODEL_DOWNLOAD_REGION"] = region

        def fake_probe_url_for_source(source: str, model_id_or_path: str, revision: str) -> str:
            return f"https://probe.invalid/{source}/{model_id_or_path}/{revision}"

        def fake_probe_url(source: str, url: str) -> dict[str, Any]:
            result = dict(probes[source])
            result.setdefault("source", source)
            result.setdefault("ttfb_ms", 10)
            result.setdefault("bytes", 4096)
            return result

        def fake_write_prepare_status(**fields: Any) -> None:
            statuses.append(fields)

        sidecar.probe_url_for_source = fake_probe_url_for_source
        sidecar.probe_url = fake_probe_url
        sidecar.write_prepare_status = fake_write_prepare_status
        sidecar._LAST_PROBE_RESULTS = None

        selected = sidecar.select_download_source(
            sidecar.DEFAULT_ASR_MODEL,
            sidecar.PINNED_MODEL_REVISIONS[sidecar.DEFAULT_ASR_MODEL],
        )
        if selected != expected_source:
            raise AssertionError(f"{name}: expected {expected_source}, got {selected}")

        final_status = next((status for status in reversed(statuses) if "probes" in status), None)
        if final_status is None:
            raise AssertionError(f"{name}: source probes were not persisted")
        recorded_sources = {probe.get("source") for probe in final_status["probes"]}
        missing_sources = set(probes) - recorded_sources
        if missing_sources:
            raise AssertionError(f"{name}: missing probe diagnostics for {sorted(missing_sources)}")
    finally:
        sidecar.probe_url_for_source = original_probe_url_for_source
        sidecar.probe_url = original_probe_url
        sidecar.write_prepare_status = original_write_prepare_status
        restore_env(previous_env)


def main() -> None:
    sidecar = load_sidecar_module()
    success = {"ok": True}
    failed = {"ok": False, "bytes_per_second": 0, "error": "synthetic timeout"}

    run_case(
        sidecar,
        name="modelscope fastest wins",
        region=None,
        probes={
            sidecar.SOURCE_CERUL_CDN: {**success, "bytes_per_second": 8_000_000},
            sidecar.SOURCE_HUGGINGFACE: {**success, "bytes_per_second": 5_000_000},
            sidecar.SOURCE_MODELSCOPE: {**success, "bytes_per_second": 18_000_000},
        },
        expected_source=sidecar.SOURCE_MODELSCOPE,
    )
    run_case(
        sidecar,
        name="region order does not override measured throughput",
        region="cn",
        probes={
            sidecar.SOURCE_CERUL_CDN: {**success, "bytes_per_second": 8_000_000},
            sidecar.SOURCE_HUGGINGFACE: {**success, "bytes_per_second": 20_000_000},
            sidecar.SOURCE_MODELSCOPE: {**success, "bytes_per_second": 6_000_000},
        },
        expected_source=sidecar.SOURCE_HUGGINGFACE,
    )
    run_case(
        sidecar,
        name="failed ModelScope probe remains diagnosable",
        region=None,
        probes={
            sidecar.SOURCE_CERUL_CDN: {**success, "bytes_per_second": 4_000_000},
            sidecar.SOURCE_HUGGINGFACE: {**success, "bytes_per_second": 9_000_000},
            sidecar.SOURCE_MODELSCOPE: failed,
        },
        expected_source=sidecar.SOURCE_HUGGINGFACE,
    )
    print("model source routing smoke passed")


if __name__ == "__main__":
    main()
