#!/usr/bin/env python3
"""Build Picto's fixed-shape Core ML tagger artifacts.

The published ONNX graphs retain dynamic conversion debris that Core ML can
only partially accelerate. This script reconstructs fixed 448px programs from
the original weights, then writes deterministic release archives.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import tempfile
import zipfile
from pathlib import Path

import coremltools as ct
import numpy as np
import onnx
import onnxruntime as ort
import timm
import torch
from coremltools.proto import Model_pb2
from huggingface_hub import hf_hub_download
from onnx2torch import convert as onnx_to_torch
from onnx2torch.node_converters.registry import add_converter, get_converter
from onnx2torch.node_converters.split import OnnxSplit13
from onnx2torch.utils.common import OperationConverterResult, onnx_mapping_from_node
from onnxruntime.tools.onnx_model_utils import fix_output_shapes, make_dim_param_fixed
from torch import nn


CATALOG_PATH = Path(__file__).with_name("model-catalog.json")
FIXED_TIMESTAMP = (2026, 1, 1, 0, 0, 0)


class WdContract(nn.Module):
    def __init__(self, model: nn.Module):
        super().__init__()
        self.model = model

    def forward(self, image: torch.Tensor) -> torch.Tensor:
        # Picto supplies NHWC BGR values in [0, 255], matching WD's ONNX export.
        image = image.permute(0, 3, 1, 2) / 127.5 - 1.0
        return torch.sigmoid(self.model(image))


class OppaiAttentionBlock(nn.Module):
    def __init__(self, hidden_size: int, heads: int, intermediate_size: int):
        super().__init__()
        self.heads = heads
        self.head_size = hidden_size // heads
        self.norm1 = nn.LayerNorm(hidden_size, eps=1e-6)
        self.qkv = nn.Linear(hidden_size, hidden_size * 3)
        self.proj = nn.Linear(hidden_size, hidden_size)
        self.norm2 = nn.LayerNorm(hidden_size, eps=1e-6)
        self.mlp = nn.Sequential(
            nn.Linear(hidden_size, intermediate_size),
            nn.GELU(),
            nn.Dropout(),
            nn.Linear(intermediate_size, hidden_size),
        )

    def forward(
        self, hidden: torch.Tensor, padding_mask: torch.Tensor | None
    ) -> torch.Tensor:
        batch, tokens, width = hidden.shape
        normalized = self.norm1(hidden)
        qkv = self.qkv(normalized).reshape(
            batch, tokens, 3, self.heads, self.head_size
        )
        qkv = qkv.permute(2, 0, 3, 1, 4)
        query, key, value = qkv.unbind(0)
        attended = torch.nn.functional.scaled_dot_product_attention(
            query,
            key,
            value,
            attn_mask=(~padding_mask)[:, None, None, :]
            if padding_mask is not None
            else None,
            dropout_p=0.0,
        )
        attended = attended.transpose(1, 2).reshape(batch, tokens, width)
        hidden = hidden + self.proj(attended)
        return hidden + self.mlp(self.norm2(hidden))


class OppaiOracle(nn.Module):
    def __init__(self, config: dict):
        super().__init__()
        hidden = config["hidden_size"]
        patch = config["patch_size"]
        self.patch_size = patch
        self.patch_embed = nn.Conv2d(3, hidden, kernel_size=patch, stride=patch)
        self.cls_token = nn.Parameter(torch.zeros(1, 1, hidden))
        patches = (config["image_size"] // patch) ** 2
        self.pos_embed = nn.Parameter(torch.zeros(1, patches + 1, hidden))
        self.blocks = nn.ModuleList(
            OppaiAttentionBlock(
                hidden, config["num_attention_heads"], config["intermediate_size"]
            )
            for _ in range(config["num_hidden_layers"])
        )
        self.norm = nn.LayerNorm(hidden, eps=1e-6)
        self.tag_head = nn.Linear(hidden, config["num_labels"])

    def forward(
        self, image: torch.Tensor, padding_mask: torch.Tensor | None
    ) -> torch.Tensor:
        hidden = self.patch_embed(image).flatten(2).transpose(1, 2)
        cls = self.cls_token.expand(image.shape[0], -1, -1)
        hidden = torch.cat((cls, hidden), dim=1) + self.pos_embed
        token_mask = None
        if padding_mask is not None:
            patch_mask = torch.nn.functional.avg_pool2d(
                padding_mask[:, None].to(hidden.dtype),
                self.patch_size,
                self.patch_size,
            ).flatten(1) >= 0.9
            token_mask = torch.cat(
                (torch.zeros_like(patch_mask[:, :1]), patch_mask), dim=1
            )
        for block in self.blocks:
            hidden = block(hidden, token_mask)
        logits = self.tag_head(self.norm(hidden[:, 0])).clamp(-15.0, 15.0)
        return torch.sigmoid(logits)


class OppaiContract(nn.Module):
    def __init__(self, model: nn.Module):
        super().__init__()
        self.model = model

    def forward(
        self, image: torch.Tensor, padding_mask: torch.Tensor
    ) -> torch.Tensor:
        image = image.permute(0, 3, 1, 2) / 127.5 - 1.0
        return self.model(image, padding_mask > 0.5)


class DanbooruTagQueryContract(nn.Module):
    def __init__(self, model: nn.Module):
        super().__init__()
        self.model = model
        self.register_buffer(
            "mean", torch.tensor((0.485, 0.456, 0.406)).reshape(1, 3, 1, 1)
        )
        self.register_buffer(
            "std", torch.tensor((0.229, 0.224, 0.225)).reshape(1, 3, 1, 1)
        )

    def forward(self, image: torch.Tensor) -> torch.Tensor:
        image = image.permute(0, 3, 1, 2) / 255.0
        return torch.sigmoid(self.model((image - self.mean) / self.std))


def register_split18_converter() -> None:
    try:
        get_converter("Split", 18)
        return
    except NotImplementedError:
        pass

    @add_converter(operation_type="Split", version=18)
    def split18(node, graph) -> OperationConverterResult:
        del graph
        return OperationConverterResult(
            torch_module=OnnxSplit13(
                axis=node.attributes.get("axis", 0),
                num_splits=len(node.output_values),
            ),
            onnx_mapping=onnx_mapping_from_node(node=node),
        )


def fixed_onnx_torch(source_path: Path) -> nn.Module:
    register_split18_converter()
    source = onnx.load(source_path)
    make_dim_param_fixed(source.graph, "batch_size", 1)
    fix_output_shapes(source)
    with tempfile.TemporaryDirectory(prefix="picto-fixed-onnx-") as temporary:
        temporary = Path(temporary)
        fixed = temporary / "fixed.onnx"
        optimized = temporary / "optimized.onnx"
        onnx.save(source, fixed)
        options = ort.SessionOptions()
        options.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_BASIC
        options.optimized_model_filepath = str(optimized)
        ort.InferenceSession(
            str(fixed), options, providers=["CPUExecutionProvider"]
        )
        return onnx_to_torch(onnx.load(optimized)).eval()


def oppai_state_dict(source: onnx.ModelProto) -> dict[str, torch.Tensor]:
    from onnx import numpy_helper

    initializers = {
        item.name: numpy_helper.to_array(item).copy() for item in source.graph.initializer
    }
    producers = {output: node for node in source.graph.node for output in node.output}
    state: dict[str, torch.Tensor] = {}
    for name in (
        "patch_embed.weight",
        "patch_embed.bias",
        "cls_token",
        "pos_embed",
        "norm.weight",
        "norm.bias",
        "tag_head.weight",
        "tag_head.bias",
    ):
        state[name] = torch.from_numpy(initializers[f"model.{name}"])
    for block in range(18):
        prefix = f"blocks.{block}"
        for layer_norm in ("norm1", "norm2"):
            for parameter in ("weight", "bias"):
                name = f"{prefix}.{layer_norm}.{parameter}"
                state[name] = torch.from_numpy(initializers[f"model.{name}"])
        for layer in ("qkv", "proj", "mlp.0", "mlp.3"):
            bias_name = f"model.{prefix}.{layer}.bias"
            add = next(node for node in source.graph.node if bias_name in node.input)
            matmul_output = next(
                item
                for item in add.input
                if item in producers and producers[item].op_type == "MatMul"
            )
            matmul = producers[matmul_output]
            weight_name = next(item for item in matmul.input if item in initializers)
            state[f"{prefix}.{layer}.weight"] = torch.from_numpy(
                initializers[weight_name].T.copy()
            )
            state[f"{prefix}.{layer}.bias"] = torch.from_numpy(initializers[bias_name])
    return state


def load_catalog() -> dict[str, dict]:
    catalog = json.loads(CATALOG_PATH.read_text())
    return {model["slug"]: model for model in catalog["models"]}


def source_file(spec: dict, filename: str, source: Path | None) -> Path:
    if source is not None:
        path = source / filename
        if not path.is_file():
            raise FileNotFoundError(f"missing staged model source: {path}")
        return path
    return Path(
        hf_hub_download(
            spec["repository"], filename, revision=spec["revision"]
        )
    )


def export_model(
    spec: dict,
    destination: Path,
    source: Path | None = None,
    precision_override: str = "auto",
) -> None:
    adapter = spec["adapter"]
    repository = spec["repository"]
    revision = spec["revision"]
    input_size = spec["input_size"]
    example = torch.zeros((1, input_size, input_size, 3), dtype=torch.float32)
    input_types = [
        ct.TensorType(name="input", shape=example.shape, dtype=np.float32)
    ]
    if adapter == "wd_timm":
        source = WdContract(
            timm.create_model(f"hf-hub:{repository}@{revision}", pretrained=True).eval()
        ).eval()
        graph = torch.export.export(source, (example,)).run_decompositions({})
    elif adapter == "onnx_trace":
        onnx_path = source_file(spec, spec["weights"], source)
        source = onnx_to_torch(onnx.load(onnx_path)).eval()
        # Freezing folds TensorFlow-exported Shape/Prod/Pad debris into the fixed
        # 448px contract. A direct ONNX Core ML conversion leaves CPU islands.
        graph = torch.jit.freeze(torch.jit.trace(source, example, strict=False).eval())
        graph = torch.jit.optimize_for_inference(graph)
    elif adapter == "oppai_oracle":
        mask = torch.zeros((1, input_size, input_size), dtype=torch.float32)
        input_types.append(
            ct.TensorType(name="padding_mask", shape=mask.shape, dtype=np.float32)
        )
        config_path = source_file(spec, "V1.1_onnx/config.json", source)
        onnx_path = source_file(spec, spec["weights"], source)
        config = json.loads(Path(config_path).read_text())
        onnx_model = onnx.load(onnx_path)
        model = OppaiOracle(config).eval()
        model.load_state_dict(oppai_state_dict(onnx_model), strict=True)
        source = OppaiContract(model).eval()
        graph = torch.export.export(source, (example, mask)).run_decompositions({})
    elif adapter == "danbooru_tag_query":
        onnx_path = source_file(spec, spec["weights"], source)
        source = DanbooruTagQueryContract(fixed_onnx_torch(onnx_path)).eval()
        graph = torch.jit.freeze(torch.jit.trace(source, example, strict=False).eval())
        graph = torch.jit.optimize_for_inference(graph)
    else:
        raise ValueError(
            f"{spec['slug']} uses adapter '{adapter}', which has not passed conversion validation"
        )

    precision = ct.precision.FLOAT16
    if adapter == "oppai_oracle":
        # Oppai's official float32 ONNX weights are unusually sensitive to
        # quantization. The fused macOS 15 graph remains GPU-fast in float32.
        precision = ct.precision.FLOAT32
    if precision_override == "float16":
        precision = ct.precision.FLOAT16
    elif precision_override == "float32":
        precision = ct.precision.FLOAT32
    minimum_macos = spec.get("minimum_macos", 14)
    deployment_target = {
        14: ct.target.macOS14,
        15: ct.target.macOS15,
    }[minimum_macos]
    converted = ct.convert(
        graph,
        convert_to="mlprogram",
        minimum_deployment_target=deployment_target,
        compute_precision=precision,
        inputs=input_types,
        compute_units=ct.ComputeUnit.ALL,
    )
    converted.save(destination)


def normalized_manifest(source: Path) -> bytes:
    manifest = json.loads(source.read_text())
    entries = sorted(manifest["itemInfoEntries"].values(), key=lambda item: item["name"])
    fixed = {
        "model.mlmodel": "00000000-0000-4000-8000-000000000001",
        "weights": "00000000-0000-4000-8000-000000000002",
    }
    normalized = {
        "fileFormatVersion": manifest["fileFormatVersion"],
        "itemInfoEntries": {fixed[item["name"]]: item for item in entries},
        "rootModelIdentifier": fixed["model.mlmodel"],
    }
    return (json.dumps(normalized, indent=2, sort_keys=True) + "\n").encode()


def normalized_model(source: Path) -> bytes:
    model = Model_pb2.Model()
    model.ParseFromString(source.read_bytes())
    return model.SerializeToString(deterministic=True)


def archive_package(package: Path, archive: Path) -> str:
    with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_STORED) as output:
        for source in sorted(path for path in package.rglob("*") if path.is_file()):
            relative = source.relative_to(package)
            name = (Path("model.mlpackage") / relative).as_posix()
            info = zipfile.ZipInfo(name, FIXED_TIMESTAMP)
            info.compress_type = zipfile.ZIP_STORED
            info.external_attr = 0o644 << 16
            if relative.as_posix() == "Manifest.json":
                data = normalized_manifest(source)
            elif relative.name == "model.mlmodel":
                data = normalized_model(source)
            else:
                data = source.read_bytes()
            output.writestr(info, data)
    return hashlib.sha256(archive.read_bytes()).hexdigest()


def main() -> None:
    models = load_catalog()
    parser = argparse.ArgumentParser()
    parser.add_argument("slugs", nargs="*", choices=sorted(models))
    parser.add_argument("--output", type=Path)
    parser.add_argument("--list", action="store_true")
    parser.add_argument(
        "--source",
        type=Path,
        help="Use an already downloaded, checksum-verified source tree; requires one slug",
    )
    parser.add_argument(
        "--package",
        type=Path,
        help="Archive an existing package; requires exactly one slug",
    )
    parser.add_argument(
        "--precision",
        choices=("auto", "float16", "float32"),
        default="auto",
        help="Override conversion precision for validation experiments",
    )
    parser.add_argument(
        "--artifact-version",
        default="v1",
        help="Immutable release suffix written into archive filenames",
    )
    args = parser.parse_args()
    if args.list:
        for model in models.values():
            print(
                f"{model['slug']}\t{model['status']}\t{model['license']}\t{model['purpose']}"
            )
        return
    if args.output is None:
        parser.error("--output is required unless --list is used")
    slugs = args.slugs or [
        slug for slug, model in models.items() if model["status"] == "proven"
    ]
    if args.package and len(slugs) != 1:
        parser.error("--package requires exactly one model slug")
    if args.source and len(slugs) != 1:
        parser.error("--source requires exactly one model slug")
    args.output.mkdir(parents=True, exist_ok=True)

    records = []
    for slug in slugs:
        model = models[slug]
        archive = args.output / (
            f"{slug}-coreml-macos{model.get('minimum_macos', 14)}-"
            f"{args.artifact_version}.zip"
        )
        if args.package:
            package = args.package
            digest = archive_package(package, archive)
        else:
            with tempfile.TemporaryDirectory(prefix=f"picto-{slug}-") as temporary:
                package = Path(temporary) / "model.mlpackage"
                export_model(model, package, args.source, args.precision)
                digest = archive_package(package, archive)
        records.append(
            {
                "slug": slug,
                "source_revision": model["revision"],
                "file": archive.name,
                "size": archive.stat().st_size,
                "sha256": digest,
            }
        )
    print(json.dumps(records, indent=2))


if __name__ == "__main__":
    main()
