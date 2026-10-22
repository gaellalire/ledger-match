#!/usr/bin/env python3
"""Generate the ledger-match app icons. Stdlib only, deterministic output.

Draws a simple "checkmark in a ring" mark at several sizes and writes it as
2-color GIF89a files, plus the rounded-rect mask used by build.rs to produce
the Nano S+/X home glyph.

Usage: python3 scripts/make_icons.py
"""

import os

ICONS_DIR = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "icons"))


def write_gif(path, width, height, pixels):
    """Write pixels (bytearray of 0/1 palette indices) as a 2-color GIF89a.

    The LZW stream emits a CLEAR code every 200 literals, so every emitted
    code stays 9 bits wide: a valid, decoder-friendly "uncompressed" GIF.
    """
    min_code_size = 8
    clear = 1 << min_code_size  # 256
    end = clear + 1  # 257

    codes = []
    for i in range(0, len(pixels), 200):
        codes.append(clear)
        codes.extend(pixels[i : i + 200])
    codes.append(end)

    packed = bytearray()
    acc, bits = 0, 0
    for code in codes:
        acc |= code << bits
        bits += 9
        while bits >= 8:
            packed.append(acc & 0xFF)
            acc >>= 8
            bits -= 8
    if bits:
        packed.append(acc & 0xFF)

    data = bytearray([min_code_size])
    for i in range(0, len(packed), 255):
        block = packed[i : i + 255]
        data.append(len(block))
        data.extend(block)
    data.append(0x00)

    out = bytearray()
    out += b"GIF89a"
    out += width.to_bytes(2, "little") + height.to_bytes(2, "little")
    out += bytes([0x80, 0, 0])  # global color table, 2 entries
    out += bytes([0, 0, 0, 255, 255, 255])  # palette: 0 = black, 1 = white
    out += b","  # image descriptor
    out += (0).to_bytes(2, "little") * 2  # left, top
    out += width.to_bytes(2, "little") + height.to_bytes(2, "little")
    out += bytes([0x00])  # no local color table
    out += data
    out += b";"
    with open(path, "wb") as f:
        f.write(out)


def blank(size):
    return bytearray(size * size)


def draw_ring(px, size, thick):
    cx = cy = (size - 1) / 2
    r_out = size / 2 - 1
    r_in = r_out - thick
    for y in range(size):
        for x in range(size):
            d = ((x - cx) ** 2 + (y - cy) ** 2) ** 0.5
            if r_in <= d <= r_out:
                px[y * size + x] = 1


def draw_segment(px, size, x0, y0, x1, y1, thick):
    dx, dy = x1 - x0, y1 - y0
    seg_len2 = dx * dx + dy * dy
    for y in range(size):
        for x in range(size):
            t = 0.0
            if seg_len2:
                t = max(0.0, min(1.0, ((x - x0) * dx + (y - y0) * dy) / seg_len2))
            if ((x - (x0 + t * dx)) ** 2 + (y - (y0 + t * dy)) ** 2) ** 0.5 <= thick / 2:
                px[y * size + x] = 1


def make_icon(size):
    px = blank(size)
    thick = max(1.0, size / 12)
    draw_ring(px, size, thick)
    # checkmark: short down-right stroke then long up-right stroke
    draw_segment(px, size, 0.24 * size, 0.55 * size, 0.44 * size, 0.72 * size, thick)
    draw_segment(px, size, 0.44 * size, 0.72 * size, 0.76 * size, 0.30 * size, thick)
    return px


def make_mask(size):
    """Filled rounded-rect mask used by build.rs for the Nano home glyph."""
    px = blank(size)
    r = size / 4
    for y in range(size):
        for x in range(size):
            cx = min(max(x, r), size - 1 - r)
            cy = min(max(y, r), size - 1 - r)
            if (x - cx) ** 2 + (y - cy) ** 2 <= r * r:
                px[y * size + x] = 1
    return px


def main():
    os.makedirs(ICONS_DIR, exist_ok=True)
    jobs = [
        ("match_14x14.gif", make_icon(14), 14),  # Nano X / Nano S+ app icon
        ("mask_14x14.gif", make_mask(14), 14),  # mask for the Nano home glyph
        ("match_32x32.gif", make_icon(32), 32),  # Stax app icon
        ("match_40x40.gif", make_icon(40), 40),  # Flex app icon
        ("match_64x64.gif", make_icon(64), 64),  # Stax/Flex review glyph
    ]
    for name, px, size in jobs:
        write_gif(os.path.join(ICONS_DIR, name), size, size, px)
        print("wrote", name)


if __name__ == "__main__":
    main()
