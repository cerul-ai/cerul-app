#!/usr/bin/env python3
"""No-network smoke checks for automatic local-model source routing."""

from __future__ import annotations

import importlib.util
import os
import tempfile
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


def assert_qwen_snapshots_require_tokenizer_files(sidecar: Any) -> None:
    for model in (sidecar.DEFAULT_ASR_MODEL, sidecar.DEFAULT_FORCED_ALIGNER_MODEL):
        with tempfile.TemporaryDirectory() as temp_dir:
            snapshot = Path(temp_dir)
            (snapshot / "config.json").write_text("{}", encoding="utf-8")
            (snapshot / "model.safetensors").write_bytes(b"weights")

            missing = sidecar.pinned_snapshot_missing_reasons(snapshot, model)
            if not any(
                "vocab.json" in reason
                and "merges.txt" in reason
                and "tokenizer_config.json" in reason
                for reason in missing
            ):
                raise AssertionError(f"{model}: tokenizer files should be required, got {missing}")

            (snapshot / "tokenizer_config.json").write_text("{}", encoding="utf-8")
            (snapshot / "vocab.json").write_text("{}", encoding="utf-8")
            (snapshot / "merges.txt").write_text("#version: 0.2\n", encoding="utf-8")
            missing = sidecar.pinned_snapshot_missing_reasons(snapshot, model)
            if missing:
                raise AssertionError(f"{model}: complete tokenizer snapshot should pass, got {missing}")


def assert_complete_mirror_cache_bypasses_download_selection(sidecar: Any) -> None:
    previous_env = {"HF_HOME": os.environ.get("HF_HOME")}
    original_cache_root = sidecar.MODELS_CACHE_ROOT
    original_bundled_snapshot_dir = sidecar.bundled_snapshot_dir
    original_select_download_source = sidecar.select_download_source
    model = sidecar.DEFAULT_FORCED_ALIGNER_MODEL
    revision = sidecar.PINNED_MODEL_REVISIONS[model]

    try:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            sidecar.MODELS_CACHE_ROOT = root
            os.environ["HF_HOME"] = str(root / "huggingface")
            sidecar.bundled_snapshot_dir = lambda *_args: None

            snapshot = sidecar.mirror_snapshot_dir(model, revision)
            if snapshot is None:
                raise AssertionError("mirror snapshot path should exist when cache root is set")
            snapshot.mkdir(parents=True)
            (snapshot / "config.json").write_text("{}", encoding="utf-8")
            (snapshot / "tokenizer_config.json").write_text("{}", encoding="utf-8")
            (snapshot / "vocab.json").write_text("{}", encoding="utf-8")
            (snapshot / "merges.txt").write_text("#version: 0.2\n", encoding="utf-8")
            (snapshot / "model.safetensors").write_bytes(b"weights")

            def fail_select_download_source(*_args: Any, **_kwargs: Any) -> str:
                raise AssertionError("complete mirror cache should bypass source selection")

            sidecar.select_download_source = fail_select_download_source
            resolved = sidecar.resolve_snapshot(model)
            if resolved != snapshot:
                raise AssertionError(f"expected complete mirror snapshot {snapshot}, got {resolved}")
    finally:
        sidecar.MODELS_CACHE_ROOT = original_cache_root
        sidecar.bundled_snapshot_dir = original_bundled_snapshot_dir
        sidecar.select_download_source = original_select_download_source
        restore_env(previous_env)


def assert_qwen_asr_disables_timestamps_without_aligner(sidecar: Any) -> None:
    kwargs = sidecar.qwen_asr_transcribe_kwargs("asr-model", None, "auto")
    if kwargs.get("return_timestamps") is not False:
        raise AssertionError(f"aligner-less Qwen ASR should disable timestamps, got {kwargs}")
    if "forced_aligner" in kwargs:
        raise AssertionError(f"aligner-less Qwen ASR should omit forced_aligner, got {kwargs}")


def main() -> None:
    sidecar = load_sidecar_module()
    success = {"ok": True}
    failed = {"ok": False, "bytes_per_second": 0, "error": "synthetic timeout"}

    run_case(
        sidecar,
        name="modelscope fastest wins",
        region=None,
        probes={
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
            sidecar.SOURCE_HUGGINGFACE: {**success, "bytes_per_second": 9_000_000},
            sidecar.SOURCE_MODELSCOPE: failed,
        },
        expected_source=sidecar.SOURCE_HUGGINGFACE,
    )
    run_case(
        sidecar,
        name="primary probe failures use region fallback",
        region="cn",
        probes={
            sidecar.SOURCE_HUGGINGFACE: failed,
            sidecar.SOURCE_MODELSCOPE: failed,
        },
        expected_source=sidecar.SOURCE_MODELSCOPE,
    )
    assert_qwen_snapshots_require_tokenizer_files(sidecar)
    assert_complete_mirror_cache_bypasses_download_selection(sidecar)
    assert_qwen_asr_disables_timestamps_without_aligner(sidecar)
    print("model source routing smoke passed")


if __name__ == "__main__":
    main()
