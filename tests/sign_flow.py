#!/usr/bin/env python3
"""Sign a TLV payload in speculos (Flex, headless), auto-approving the review.

Runs inside the Ledger dev-tools container (app mounted at /app).

Usage: python3 tests/sign_flow.py <payload-hex> [tap-count]
Prints the status word and data of each APDU; exit 0 iff signing succeeded.
"""
import sys
from pathlib import Path

from ragger.backend import SpeculosBackend
from ragger.firmware.structs import Devices
from ragger.firmware.touch.use_cases import UseCaseReview, UseCaseStatus

APP = Path("/app/target/flex/release/ledger-match")
DEVICE = Devices.get_by_name("flex")
BIP32_PATH = bytes.fromhex("048000002c80082b348000000000000000")  # m/44'/535348'/0'/0


def main() -> int:
    payload = bytes.fromhex(sys.argv[1])
    taps = int(sys.argv[2]) if len(sys.argv) > 2 else 10
    backend = SpeculosBackend(APP, DEVICE, args=["--display", "headless"])
    with backend:
        rapdu = backend.exchange(cla=0xE0, ins=0x06, p1=0, p2=0x80, data=BIP32_PATH)
        print(f"path chunk: {rapdu.status:#06x} {rapdu.data.hex()}")
        with backend.exchange_async(cla=0xE0, ins=0x06, p1=1, p2=0x00, data=payload):
            review = UseCaseReview(backend, DEVICE)
            for _ in range(taps):
                review.tap()
            review.confirm()
        rapdu = backend.last_async_response
        print(f"payload chunk: {rapdu.status:#06x} {rapdu.data.hex()}")
        if rapdu.status == 0x9000:
            UseCaseStatus(backend, DEVICE).dismiss()
        return 0 if rapdu.status == 0x9000 else 1


if __name__ == "__main__":
    sys.exit(main())
