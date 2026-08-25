#!/usr/bin/env python3
"""Measure warm Core ML inference for one converted Picto tagger."""

from __future__ import annotations

import argparse
import json
import statistics
import time
from pathlib import Path

import coremltools as ct
import numpy as np
from PIL import Image

CATALOG_PATH = Path(__file__).with_name("model-catalog.json")


def image_inputs(path: Path, spec: dict) -> dict[str, np.ndarray]:
    size = spec["input_size"]
    image = Image.open(path).convert("RGB")
    scale = size / max(image.size)
    image = image.resize(
        tuple(max(1, round(axis * scale)) for axis in image.size),
        Image.Resampling.LANCZOS,
    )
    background = {
        "oppai_oracle": 114,
        "danbooru_tag_query": 0,
    }.get(spec["adapter"], 255)
    canvas = Image.new("RGB", (size, size), (background,) * 3)
    offset = ((size - image.width) // 2, (size - image.height) // 2)
    canvas.paste(image, offset)
    mask = np.ones((1, size, size), dtype=np.float32)
    mask[:, offset[1] : offset[1] + image.height, offset[0] : offset[0] + image.width] = 0
    image = canvas
    values = np.asarray(image, dtype=np.float32)
    if spec["adapter"] in {"wd_timm", "onnx_trace"}:
        values = values[:, :, ::-1]
    inputs = {"input": np.expand_dims(values, 0).copy()}
    if spec["adapter"] == "oppai_oracle":
        inputs["padding_mask"] = mask
    return inputs


def percentile(samples: list[float], fraction: float) -> float:
    index = min(len(samples) - 1, int((len(samples) - 1) * fraction + 0.999999))
    return sorted(samples)[index]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("slug")
    parser.add_argument("package", type=Path)
    parser.add_argument("image", type=Path)
    parser.add_argument("--warmups", type=int, default=5)
    parser.add_argument("--runs", type=int, default=30)
    parser.add_argument("--max-median-ms", type=float, default=500.0)
    args = parser.parse_args()

    catalog = {
        item["slug"]: item
        for item in json.loads(CATALOG_PATH.read_text())["models"]
    }
    if args.slug not in catalog:
        parser.error(f"unknown model: {args.slug}")
    model = ct.models.MLModel(str(args.package), compute_units=ct.ComputeUnit.ALL)
    values = image_inputs(args.image, catalog[args.slug])
    for _ in range(args.warmups):
        model.predict(values)

    samples = []
    for _ in range(args.runs):
        started = time.perf_counter()
        model.predict(values)
        samples.append((time.perf_counter() - started) * 1000.0)

    result = {
        "slug": args.slug,
        "runs": args.runs,
        "median_ms": round(statistics.median(samples), 2),
        "p95_ms": round(percentile(samples, 0.95), 2),
        "maximum_ms": round(max(samples), 2),
        "limit_ms": args.max_median_ms,
    }
    result["passed"] = result["median_ms"] <= args.max_median_ms
    print(json.dumps(result, indent=2))
    if not result["passed"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
