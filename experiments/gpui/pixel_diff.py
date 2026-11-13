#!/usr/bin/env python3
"""Compare two unscaled, aligned UI captures of the same size.

Needs only the Python standard library. PNG captures are read directly; other
formats are converted with macOS `sips`. Region coordinates are pixels of the
captures. Never stretch a capture to make sizes agree: open both windows at the
same size instead (`PICTO_GPUI_WINDOW=WxH`).

    pixel_diff.py electron.png gpui.png out/ \
        --region sidebar=0,48,300,817 --region titlebar=0,0,1213,48
"""
import argparse
import json
import struct
import subprocess
import tempfile
import zlib
from pathlib import Path


def read_png(path):
    """Returns (width, height, rows) with rows as bytes of RGB triples."""
    data = Path(path).read_bytes()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError(f"{path} is not a PNG")
    offset, chunks, header = 8, [], None
    while offset < len(data):
        (length,) = struct.unpack(">I", data[offset : offset + 4])
        kind = data[offset + 4 : offset + 8]
        body = data[offset + 8 : offset + 8 + length]
        if kind == b"IHDR":
            header = struct.unpack(">IIBBBBB", body)
        elif kind == b"IDAT":
            chunks.append(body)
        offset += 12 + length
    width, height, depth, colour, _, _, interlace = header
    if depth != 8 or colour not in (2, 6) or interlace:
        raise ValueError(f"{path}: only 8-bit RGB or RGBA, non-interlaced PNG is supported")
    step = 3 if colour == 2 else 4
    stride = width * step
    raw = zlib.decompress(b"".join(chunks))
    rows, previous = [], bytearray(stride)
    for y in range(height):
        start = y * (stride + 1)
        kind = raw[start]
        line = bytearray(raw[start + 1 : start + 1 + stride])
        if kind == 1:
            for i in range(step, stride):
                line[i] = (line[i] + line[i - step]) & 255
        elif kind == 2:
            for i in range(stride):
                line[i] = (line[i] + previous[i]) & 255
        elif kind == 3:
            for i in range(stride):
                left = line[i - step] if i >= step else 0
                line[i] = (line[i] + (left + previous[i]) // 2) & 255
        elif kind == 4:
            for i in range(stride):
                left = line[i - step] if i >= step else 0
                up = previous[i]
                corner = previous[i - step] if i >= step else 0
                estimate = left + up - corner
                a, b, c = abs(estimate - left), abs(estimate - up), abs(estimate - corner)
                line[i] = (line[i] + (left if a <= b and a <= c else up if b <= c else corner)) & 255
        previous = line
        rows.append(bytes(line) if step == 3 else bytes(b for i, b in enumerate(line) if i % 4 != 3))
    return width, height, rows


def write_png(path, width, rows):
    def chunk(kind, body):
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body))

    raw = b"".join(b"\x00" + row for row in rows)
    Path(path).write_bytes(
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, len(rows), 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 6))
        + chunk(b"IEND", b"")
    )


def load(path):
    path = Path(path)
    if path.suffix.lower() == ".png":
        try:
            return read_png(path)
        except ValueError:
            pass
    with tempfile.TemporaryDirectory() as directory:
        converted = Path(directory) / "capture.png"
        subprocess.run(
            ["sips", "-s", "format", "png", str(path), "--out", str(converted)],
            check=True,
            capture_output=True,
        )
        return read_png(converted)


def region(text):
    name, _, box = text.partition("=")
    x, y, w, h = (int(part) for part in box.split(","))
    return name, (x, y, w, h)


parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
parser.add_argument("reference", type=Path)
parser.add_argument("candidate", type=Path)
parser.add_argument("output", type=Path)
parser.add_argument("--threshold", type=int, default=16, help="largest channel difference still counted as equal")
parser.add_argument("--region", action="append", type=region, default=[], metavar="NAME=X,Y,W,H")
parser.add_argument(
    "--shift",
    type=lambda text: tuple(int(part) for part in text.split(",")),
    default=(0, 0),
    metavar="DX,DY",
    help="move the candidate by this many pixels first, to tell a uniform offset from real differences",
)
parser.add_argument(
    "--crop",
    action="store_true",
    help="compare the top-left area both captures share when their sizes differ by a few pixels",
)
args = parser.parse_args()

width, height, reference = load(args.reference)
other_width, other_height, candidate = load(args.candidate)
cropped = None
if (width, height) != (other_width, other_height):
    slack = max(abs(width - other_width), abs(height - other_height))
    if not args.crop or slack > 4:
        parser.error(
            f"Capture sizes differ: {width}x{height} vs {other_width}x{other_height}; "
            "open both windows at the same size, never stretch. "
            "--crop accepts a difference of up to 4 pixels."
        )
    cropped = [[width, height], [other_width, other_height]]
    width, height = min(width, other_width), min(height, other_height)
    reference = [row[: width * 3] for row in reference[:height]]
    candidate = [row[: width * 3] for row in candidate[:height]]

dx, dy = args.shift
if (dx, dy) != (0, 0):
    blank = bytes(width * 3)

    def moved(row):
        if dx > 0:
            return bytes(dx * 3) + row[: (width - dx) * 3]
        return row[-dx * 3 :] + bytes(-dx * 3) if dx < 0 else row

    rows = [moved(row) for row in candidate]
    candidate = ([blank] * dy + rows[: height - dy]) if dy >= 0 else (rows[-dy:] + [blank] * -dy)

# One byte per pixel: the largest channel difference.
difference = []
for a, b in zip(reference, candidate):
    difference.append(bytes(max(abs(a[i] - b[i]), abs(a[i + 1] - b[i + 1]), abs(a[i + 2] - b[i + 2])) for i in range(0, width * 3, 3)))


def measure(x, y, w, h):
    x, y = max(0, x), max(0, y)
    w, h = min(w, width - x), min(h, height - y)
    different = total = 0
    for row in difference[y : y + h]:
        part = row[x : x + w]
        different += sum(1 for value in part if value > args.threshold)
        total += sum(part)
    pixels = max(1, w * h)
    return {
        "box": [x, y, w, h],
        "different_pixel_percent": round(100 * different / pixels, 3),
        "mean_channel_difference": round(total / pixels, 3),
    }


report = {
    "size": [width, height],
    "threshold": args.threshold,
    "cropped_from": cropped,
    "whole": measure(0, 0, width, height),
    "regions": {name: measure(*box) for name, box in args.region},
    "note": "A parity verdict needs identical content, scroll, selection, focus and scale in both captures.",
}

args.output.mkdir(parents=True, exist_ok=True)
# Reference, candidate, and the differing pixels in red over a dimmed reference.
mask_rows, comparison_rows = [], []
for a, b, d in zip(reference, candidate, difference):
    marked = bytearray()
    for i, value in enumerate(d):
        if value > args.threshold:
            marked += b"\xff\x20\x20"
        else:
            j = i * 3
            marked += bytes((a[j] // 3, a[j + 1] // 3, a[j + 2] // 3))
    mask_rows.append(bytes(marked))
    comparison_rows.append(a + b + bytes(marked))
write_png(args.output / "mask.png", width, mask_rows)
write_png(args.output / "comparison.png", width * 3, comparison_rows)
(args.output / "metrics.json").write_text(json.dumps(report, indent=2) + "\n")
print(json.dumps(report, indent=2))
