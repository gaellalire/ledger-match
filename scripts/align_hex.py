#!/usr/bin/env python3
"""Pad the cargo-ledger hex export so the device accepts the install.

The Ledger loader requires the application code length to be flash-page
aligned (512 bytes), and ledgerctl additionally asserts 64-byte alignment.
The hex produced by `cargo ledger build` is not always aligned (the C SDK
normally pads at link time). This appends padding bytes past the last hex
record: they land beyond both the code and data regions the device programs,
so the app image is unchanged. Idempotent.

Usage: python3 scripts/align_hex.py [target/flex/release/app.json]
"""

import json
import os
import sys

from intelhex import IntelHex

# The loader wants the code region aligned to a flash page.
FLASH_PAGE = 512


def main():
    manifest_path = (
        sys.argv[1]
        if len(sys.argv) > 1
        else os.path.join(
            os.path.dirname(__file__), "..", "target", "flex", "release", "app.json"
        )
    )
    with open(manifest_path) as f:
        app = json.load(f)

    release_dir = os.path.dirname(os.path.abspath(manifest_path))
    hex_path = os.path.join(release_dir, app["binary"])
    data_size = int(app.get("dataSize", 0))

    hex_file = IntelHex(hex_path)
    span = hex_file.maxaddr() - hex_file.minaddr() + 1
    pad = (-(span - data_size)) % FLASH_PAGE
    if pad:
        hex_file.puts(hex_file.maxaddr() + 1, bytes(pad))
        hex_file.write_hex_file(hex_path)
        print(f"{hex_path}: padded {pad} byte(s) for {FLASH_PAGE}-byte page alignment")
    else:
        print(f"{hex_path}: already aligned")


if __name__ == "__main__":
    main()
