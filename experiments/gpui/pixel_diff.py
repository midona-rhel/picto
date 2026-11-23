#!/usr/bin/env python3
"""Compare unscaled, aligned UI captures. Crop coordinates are physical pixels."""
import argparse
import json
from pathlib import Path
from PIL import Image, ImageChops, ImageEnhance, ImageStat

parser = argparse.ArgumentParser()
parser.add_argument("reference", type=Path)
parser.add_argument("candidate", type=Path)
parser.add_argument("output", type=Path)
parser.add_argument("--reference-crop", nargs=4, type=int)
parser.add_argument("--candidate-crop", nargs=4, type=int)
parser.add_argument("--threshold", type=int, default=16)
args = parser.parse_args()
reference = Image.open(args.reference).convert("RGB")
candidate = Image.open(args.candidate).convert("RGB")
if args.reference_crop:
    reference = reference.crop(tuple(args.reference_crop))
if args.candidate_crop:
    candidate = candidate.crop(tuple(args.candidate_crop))
if reference.size != candidate.size:
    parser.error(f"Capture sizes differ: {reference.size} vs {candidate.size}; align or crop, never stretch.")
args.output.mkdir(parents=True, exist_ok=True)
diff = ImageChops.difference(reference, candidate)
channels = diff.split()
maximum = ImageChops.lighter(ImageChops.lighter(channels[0], channels[1]), channels[2])
mask = maximum.point(lambda value: 255 if value > args.threshold else 0)
report = {
    "size": reference.size,
    "threshold": args.threshold,
    "different_pixel_percent": 100 * mask.histogram()[255] / (reference.width * reference.height),
    "mean_absolute_channel_error": sum(ImageStat.Stat(diff).mean) / 3,
    "note": "Requires identical content, viewport, scroll, selection, focus and scale for a parity verdict.",
}
Image.blend(reference, candidate, 0.5).save(args.output / "overlay.png")
ImageEnhance.Brightness(diff).enhance(4).save(args.output / "difference.png")
mask.save(args.output / "mask.png")
comparison = Image.new("RGB", (reference.width * 3, reference.height))
for index, image in enumerate((reference, candidate, ImageEnhance.Brightness(diff).enhance(4))):
    comparison.paste(image, (reference.width * index, 0))
comparison.save(args.output / "comparison.png")
(args.output / "metrics.json").write_text(json.dumps(report, indent=2) + "\n")
print(json.dumps(report, indent=2))
