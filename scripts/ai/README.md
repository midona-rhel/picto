# Picto Core ML model artifacts

Picto converts supported taggers to fixed-shape Core ML programs so macOS can
run inference on the GPU and Neural Engine. The application verifies every
download against `coreml-artifacts.json` before activating it.

Core ML archives are not bundled with Picto. Model weights are optional and
downloaded only when the user requests them. Optimized archives are published
under immutable versioned releases and pinned by exact size and SHA-256 in
`coreml-artifacts.json`. Picto downloads one only when the user explicitly
chooses **Optimize for this Mac**; the portable ONNX model remains the fallback.

All product models use the same portable ONNX download path on macOS, Windows,
and Linux. Candidate optimized macOS archives are built by the separate model
workflow:

- OppaiOracle V1.1 uses the same image plus padding-mask contract as its
  portable ONNX model. The corrected `ai-models-v2` package is registered only
  after schema, ONNX parity, and warm-performance verification. Invalid local
  Core ML packages fall back to ONNX rather than failing a tagging run.
- DanbooruTagQuery B16 uses a fixed-shape DINOv3 graph and retains at least 99%
  of the top 100 ONNX tags across the parity fixture set. It remains tooling-only
  until Picto has an explicit DINOv3 agreement acceptance and retention flow.

AnimeTimm is deliberately excluded from the catalog.

Z3D E621 ConvNext is supported by the converter and benchmark for local
testing and is downloaded directly from its upstream repository. Picto does not
redistribute the model or publish a derived Core ML artifact because upstream
does not state redistribution terms.

Run the `Core ML Models` workflow manually to rebuild, verify, and publish the
registered archives. The conversion dependencies are pinned and the archive
writer normalizes Core ML metadata and protobuf ordering for reproducible
checksums.

Every candidate package must pass both `verify-coreml-parity.py` and
`benchmark-coreml.py`. The performance check rejects a warm median above
500 ms, preventing the slow unfused graph from being published again.

## Reference performance

Warm single-image inference, 448 px input, 30 measured runs on an 18-core
Apple M5 Pro using `ComputeUnit.ALL`:

| Model | Median | p95 | Relative time |
| --- | ---: | ---: | ---: |
| DanbooruTagQuery B16 | 13.23 ms | 14.88 ms | 1.00× |
| E621 ConvNext (Z3D) | 14.71 ms | 16.11 ms | 1.11× |
| WD14 SwinV2 v3 | 17.74 ms | 20.41 ms | 1.34× |
| WD14 EVA02-Large v3 | 48.38 ms | 50.03 ms | 3.66× |
| OppaiOracle V1.1 | 86.45 ms | 92.54 ms | 6.53× |

These numbers compare model execution, not download, cold compilation, image
decode, or post-processing. Picto may select multiple models and media items,
but uses one inference lane: one model stays loaded while it processes the
complete selected media batch, then it is unloaded before the next model loads.
