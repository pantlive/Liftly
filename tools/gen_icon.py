#!/usr/bin/env python3
"""Generate Liftly's icon assets without platform-dependent ICO encoders."""

from __future__ import annotations

import argparse
import io
import math
import struct
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
ASSETS_DIR = ROOT / "assets"

SIZES = (16, 20, 24, 32, 40, 48, 64, 256)
DESIGN_SIZE = 256
SUPERSAMPLE = 8

START_COLOR = (0x3D, 0xDC, 0x97)
END_COLOR = (0x0B, 0x7A, 0x75)
WHITE = (255, 255, 255, 255)
ARC_ALPHA = 90


def _scaled(value: float, canvas_size: int) -> float:
    return value * canvas_size / DESIGN_SIZE


def _gradient(canvas_size: int) -> Image.Image:
    """Build the top-left to bottom-right gradient at the working resolution."""
    r0, g0, b0 = START_COLOR
    r1, g1, b1 = END_COLOR
    denominator = 2 * canvas_size
    data = bytearray(canvas_size * canvas_size * 3)

    offset = 0
    for y in range(canvas_size):
        for x in range(canvas_size):
            position = x + y
            data[offset] = (
                r0 * (denominator - position) + r1 * position + denominator // 2
            ) // denominator
            data[offset + 1] = (
                g0 * (denominator - position) + g1 * position + denominator // 2
            ) // denominator
            data[offset + 2] = (
                b0 * (denominator - position) + b1 * position + denominator // 2
            ) // denominator
            offset += 3

    return Image.frombytes("RGB", (canvas_size, canvas_size), data)


def _base_layer(target_size: int, canvas_size: int) -> Image.Image:
    base = _gradient(canvas_size)

    mask = Image.new("L", (canvas_size, canvas_size), 0)
    draw = ImageDraw.Draw(mask)
    if target_size >= 32:
        left = _scaled(6, canvas_size)
        top = _scaled(6, canvas_size)
        right = _scaled(250, canvas_size)
        bottom = _scaled(250, canvas_size)
        radius = _scaled(56, canvas_size)
    else:
        left, top = 0, 0
        right, bottom = canvas_size, canvas_size
        radius = _scaled(52, canvas_size)

    draw.rounded_rectangle(
        (left, top, right, bottom),
        radius=radius,
        fill=255,
    )
    base.putalpha(mask)
    return base


def _round_line(
    draw: ImageDraw.ImageDraw,
    start: tuple[float, float],
    end: tuple[float, float],
    width: float,
    scale: float,
) -> None:
    x1, y1 = start[0] * scale, start[1] * scale
    x2, y2 = end[0] * scale, end[1] * scale
    line_width = max(1, round(width))
    draw.line((x1, y1, x2, y2), fill=WHITE, width=line_width)

    radius = line_width / 2
    for x, y in ((x1, y1), (x2, y2)):
        draw.ellipse(
            (x - radius, y - radius, x + radius, y + radius),
            fill=WHITE,
        )


def _person_layer(target_size: int, canvas_size: int) -> Image.Image:
    layer = Image.new("RGBA", (canvas_size, canvas_size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(layer)
    scale = canvas_size / DESIGN_SIZE

    stroke_width = max(24.0, 2.1 * DESIGN_SIZE / target_size) * scale

    if target_size >= 32:
        # Large sizes: compact figure whose feet clear the timer arc.
        head_center_y = 66.0
        head_radius = max(22.0, 1.9 * DESIGN_SIZE / target_size) * scale
        torso = ((128, 116), (128, 160))
        arms = (((128, 124), (82, 80)), ((128, 124), (174, 80)))
        legs = (((128, 158), (102, 204)), ((128, 158), (154, 204)))
    else:
        # Small sizes: lower, wider arms so hands do not merge with the head.
        head_center_y = 58.0
        head_radius = max(30.0, 1.9 * DESIGN_SIZE / target_size) * scale
        torso = ((128, 118), (128, 170))
        arms = (((128, 130), (64, 98)), ((128, 130), (192, 98)))
        legs = (((128, 168), (96, 222)), ((128, 168), (160, 222)))

    _round_line(draw, torso[0], torso[1], stroke_width, scale)
    for start, end in arms:
        _round_line(draw, start, end, stroke_width, scale)
    for start, end in legs:
        _round_line(draw, start, end, stroke_width, scale)

    center_x = 128 * scale
    center_y = head_center_y * scale
    draw.ellipse(
        (
            center_x - head_radius,
            center_y - head_radius,
            center_x + head_radius,
            center_y + head_radius,
        ),
        fill=WHITE,
    )
    return layer


def _timer_arc_layer(canvas_size: int) -> Image.Image:
    layer = Image.new("RGBA", (canvas_size, canvas_size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(layer)
    scale = canvas_size / DESIGN_SIZE
    center_x = 128 * scale
    center_y = 138 * scale
    radius = 96 * scale
    arc_width = max(1, round(10 * scale))

    # Pillow grows thick arcs inwards, so offset the guide ellipse by half the
    # stroke to keep the arc's centre line on the requested radius.
    outer_radius = radius + arc_width / 2
    draw.arc(
        (
            center_x - outer_radius,
            center_y - outer_radius,
            center_x + outer_radius,
            center_y + outer_radius,
        ),
        start=300,
        end=540,
        fill=WHITE,
        width=arc_width,
    )

    cap_radius = arc_width / 2
    # Start at the 1 o'clock mark, sweep clockwise through the bottom, and stop
    # at the 9 o'clock mark; the upper-left 120 degrees stays open.
    for angle in (300, 540):
        radians = math.radians(angle)
        x = center_x + radius * math.cos(radians)
        y = center_y + radius * math.sin(radians)
        draw.ellipse(
            (x - cap_radius, y - cap_radius, x + cap_radius, y + cap_radius),
            fill=WHITE,
        )

    # The arc and both caps are one opaque layer; dimming the whole layer here
    # prevents the overlapping caps from becoming darker than the arc.
    alpha_lut = [value * ARC_ALPHA // 255 for value in range(256)]
    layer.putalpha(layer.getchannel("A").point(alpha_lut))
    return layer


def _render_frame(target_size: int) -> Image.Image:
    canvas_size = target_size * SUPERSAMPLE
    frame = _base_layer(target_size, canvas_size)

    if target_size >= 40:
        frame = Image.alpha_composite(frame, _timer_arc_layer(canvas_size))

    frame = Image.alpha_composite(frame, _person_layer(target_size, canvas_size))
    return frame.resize((target_size, target_size), Image.Resampling.LANCZOS)


def _png_bytes(image: Image.Image) -> bytes:
    output = io.BytesIO()
    image.save(output, format="PNG", optimize=False)
    return output.getvalue()


def _bmp_frame(image: Image.Image) -> bytes:
    """Encode a DIB icon frame: BITMAPINFOHEADER + BGRA data + AND mask."""
    width, height = image.size
    rgba = image.tobytes()

    header = struct.pack(
        "<IiiHHIIiiII",
        40,
        width,
        height * 2,
        1,
        32,
        0,
        0,
        0,
        0,
        0,
        0,
    )

    xor_data = bytearray(width * height * 4)
    and_stride = ((width + 31) // 32) * 4
    and_mask = bytearray(and_stride * height)

    destination = 0
    for y in range(height - 1, -1, -1):
        source = y * width * 4
        and_row = (height - 1 - y) * and_stride
        for x in range(width):
            red, green, blue, alpha = rgba[source : source + 4]
            xor_data[destination : destination + 4] = bytes(
                (blue, green, red, alpha)
            )
            if alpha == 0:
                and_mask[and_row + (x // 8)] |= 0x80 >> (x % 8)
            source += 4
            destination += 4

    return header + xor_data + and_mask


def _build_ico(frames: dict[int, Image.Image], png_256: bytes) -> bytes:
    payloads: list[bytes] = []
    for size in SIZES:
        if size == 256:
            payloads.append(png_256)
        else:
            payloads.append(_bmp_frame(frames[size]))

    data_offset = 6 + 16 * len(SIZES)
    directory = bytearray(struct.pack("<HHH", 0, 1, len(SIZES)))

    for size, payload in zip(SIZES, payloads):
        dimension = 0 if size == 256 else size
        directory.extend(
            struct.pack(
                "<BBBBHHII",
                dimension,
                dimension,
                0,
                0,
                1,
                32,
                len(payload),
                data_offset,
            )
        )
        data_offset += len(payload)

    return bytes(directory) + b"".join(payloads)


def _write_preview(path: Path, frames: dict[int, Image.Image]) -> None:
    scaled = {
        size: image.resize(
            (size * 4, size * 4),
            Image.Resampling.NEAREST,
        )
        for size, image in frames.items()
    }

    gap = 16
    margin = 16
    row_height = max(image.height for image in scaled.values())
    width = margin * 2 + sum(image.width for image in scaled.values())
    width += gap * (len(scaled) - 1)
    half_height = row_height + margin * 2

    preview = Image.new("RGB", (width, half_height * 2), "#F3F3F3")
    draw = ImageDraw.Draw(preview)
    draw.rectangle((0, half_height, width, half_height * 2), fill="#202020")

    for row in range(2):
        x = margin
        for size in SIZES:
            image = scaled[size]
            y = row * half_height + margin + (row_height - image.height) // 2
            preview.paste(image, (x, y), image)
            x += image.width + gap

    path.parent.mkdir(parents=True, exist_ok=True)
    preview.save(path, format="PNG", optimize=False)


def main() -> None:
    parser = argparse.ArgumentParser(description="Generate Liftly icon assets.")
    parser.add_argument(
        "--preview",
        type=Path,
        metavar="PATH",
        help="also write a light/dark contact sheet at PATH",
    )
    args = parser.parse_args()

    frames = {size: _render_frame(size) for size in SIZES}
    png_256 = _png_bytes(frames[256])
    ico_data = _build_ico(frames, png_256)

    ASSETS_DIR.mkdir(parents=True, exist_ok=True)
    (ASSETS_DIR / "liftly.ico").write_bytes(ico_data)
    (ASSETS_DIR / "liftly.png").write_bytes(png_256)

    if args.preview is not None:
        _write_preview(args.preview, frames)

    print(f"wrote {ASSETS_DIR / 'liftly.ico'}")
    print(f"wrote {ASSETS_DIR / 'liftly.png'}")
    if args.preview is not None:
        print(f"wrote {args.preview}")


if __name__ == "__main__":
    main()
