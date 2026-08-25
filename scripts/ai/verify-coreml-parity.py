#!/usr/bin/env python3
"""Compare a converted Core ML tagger with its pinned ONNX source."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import coremltools as ct
import numpy as np
import onnxruntime as ort
from PIL import Image
from huggingface_hub import hf_hub_download


CATALOG_PATH = Path(__file__).with_name("model-catalog.json")
MEAN_ERROR_LIMIT = 1e-4
TOP_100_OVERLAP_LIMIT = 0.99
THRESHOLD_AGREEMENT_LIMIT = 0.995


def catalog() -> dict[str, dict]:
    data = json.loads(CATALOG_PATH.read_text())
    return {model["slug"]: model for model in data["models"]}


def prepare_image(
    path: Path, size: int, rgb: bool, background: int = 255
) -> tuple[np.ndarray, np.ndarray]:
    image = Image.open(path).convert("RGB")
    width, height = image.size
    scale = size / max(width, height)
    resized = image.resize(
        (max(1, round(width * scale)), max(1, round(height * scale))),
        Image.Resampling.LANCZOS,
    )
    canvas = Image.new("RGB", (size, size), (background, background, background))
    padding_mask = np.ones((size, size), dtype=bool)
    offset = ((size - resized.width) // 2, (size - resized.height) // 2)
    canvas.paste(
        resized,
        offset,
    )
    padding_mask[
        offset[1] : offset[1] + resized.height,
        offset[0] : offset[0] + resized.width,
    ] = False
    values = np.asarray(canvas, dtype=np.float32)
    if not rgb:
        values = values[:, :, ::-1]
    return np.expand_dims(values, 0).copy(), np.expand_dims(padding_mask, 0)


def verify(
    spec: dict, package: Path, fixtures: list[Path], source_root: Path | None
) -> dict:
    if spec["adapter"] not in {
        "wd_timm",
        "onnx_trace",
        "oppai_oracle",
        "danbooru_tag_query",
    }:
        raise ValueError(f"Parity preprocessing is not validated for {spec['slug']} yet")
    source = (
        source_root / spec["weights"]
        if source_root is not None
        else Path(
            hf_hub_download(
                spec["repository"], spec["weights"], revision=spec["revision"]
            )
        )
    )
    onnx = ort.InferenceSession(source, providers=["CPUExecutionProvider"])
    coreml = ct.models.MLModel(str(package), compute_units=ct.ComputeUnit.ALL)
    onnx_inputs = [item.name for item in onnx.get_inputs()]
    coreml_inputs = list(coreml.input_description)
    coreml_outputs = list(coreml.output_description)
    mean_errors = []
    max_errors = []
    top_overlaps = []
    threshold_agreements = []
    for fixture in fixtures:
        if spec["adapter"] == "oppai_oracle":
            values, padding_mask = prepare_image(
                fixture, spec["input_size"], rgb=True, background=114
            )
            expected_inputs = {
                onnx_inputs[0]: (values.transpose(0, 3, 1, 2) / 127.5 - 1.0),
                onnx_inputs[1]: padding_mask,
            }
            actual_inputs = {
                "input": values,
                "padding_mask": padding_mask.astype(np.float32),
            }
        elif spec["adapter"] == "danbooru_tag_query":
            values, _ = prepare_image(
                fixture, spec["input_size"], rgb=True, background=0
            )
            normalized = values.transpose(0, 3, 1, 2) / 255.0
            mean = np.asarray((0.485, 0.456, 0.406), dtype=np.float32).reshape(
                1, 3, 1, 1
            )
            std = np.asarray((0.229, 0.224, 0.225), dtype=np.float32).reshape(
                1, 3, 1, 1
            )
            expected_inputs = {onnx_inputs[0]: (normalized - mean) / std}
            actual_inputs = {coreml_inputs[0]: values}
        else:
            values, _ = prepare_image(fixture, spec["input_size"], rgb=False)
            expected_inputs = {onnx_inputs[0]: values}
            actual_inputs = {coreml_inputs[0]: values}
        expected = np.asarray(onnx.run(None, expected_inputs)[0]).reshape(-1)
        if spec["adapter"] == "danbooru_tag_query":
            expected = 1.0 / (1.0 + np.exp(-expected))
        prediction = coreml.predict(actual_inputs)
        actual = np.asarray(prediction[coreml_outputs[0]]).reshape(-1)
        error = np.abs(expected - actual)
        mean_errors.append(float(error.mean()))
        max_errors.append(float(error.max()))
        expected_top = set(np.argpartition(expected, -100)[-100:])
        actual_top = set(np.argpartition(actual, -100)[-100:])
        top_overlaps.append(len(expected_top & actual_top) / 100)
        threshold_agreements.append(float(np.mean((expected >= 0.35) == (actual >= 0.35))))
    result = {
        "slug": spec["slug"],
        "fixtures": len(fixtures),
        "mean_absolute_error": float(np.mean(mean_errors)),
        "max_absolute_error": max(max_errors),
        "top_100_overlap": float(np.mean(top_overlaps)),
        "threshold_agreement": float(np.mean(threshold_agreements)),
    }
    result["passed"] = (
        result["mean_absolute_error"] <= MEAN_ERROR_LIMIT
        and result["top_100_overlap"] >= TOP_100_OVERLAP_LIMIT
        and result["threshold_agreement"] >= THRESHOLD_AGREEMENT_LIMIT
    )
    return result


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("slug")
    parser.add_argument("package", type=Path)
    parser.add_argument("fixtures", type=Path)
    parser.add_argument(
        "--source",
        type=Path,
        help="Use an already downloaded, checksum-verified source tree",
    )
    args = parser.parse_args()
    models = catalog()
    spec = models.get(args.slug)
    if spec is None:
        parser.error(f"unknown model: {args.slug}")
    fixtures = sorted(
        path
        for path in args.fixtures.iterdir()
        if path.suffix.lower() in {".jpg", ".jpeg", ".png", ".webp"}
    )
    if not fixtures:
        parser.error("fixtures directory contains no supported images")
    result = verify(spec, args.package, fixtures, args.source)
    print(json.dumps(result, indent=2))
    if not result["passed"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
