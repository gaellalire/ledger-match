#!/usr/bin/env python3
"""Verify a ledger-match signature off-device.

Usage: verify_signature.py <pubkey-response-hex> <payload-hex> <sign-response-hex>
- pubkey response: [0x41][65B uncompressed pubkey][0x20][32B chaincode]
  (a trailing 9000 status word is ignored)
- payload: the raw TLV bytes that were signed (canonical form)
- sign response: [sigLen][DER sig][v] (a trailing 9000 is ignored)

Requires: pycryptodome, ecdsa.
"""
import sys

from Crypto.Hash import keccak
from ecdsa import SECP256k1, VerifyingKey
from ecdsa.util import sigdecode_der

pub_resp = bytes.fromhex(sys.argv[1])
payload = bytes.fromhex(sys.argv[2])
sign_resp = bytes.fromhex(sys.argv[3])

assert pub_resp[0] == 0x41, "unexpected pubkey length"
pubkey = pub_resp[1 : 1 + 0x41]

siglen = sign_resp[0]
sig = sign_resp[1 : 1 + siglen]
v = sign_resp[1 + siglen]

k = keccak.new(digest_bits=256)
k.update(payload)
digest = k.digest()

vk = VerifyingKey.from_string(pubkey[1:], curve=SECP256k1)
ok = vk.verify_digest(sig, digest, sigdecode=sigdecode_der)
print("keccak256(payload):", digest.hex())
print("pubkey:", pubkey.hex())
print("signature valid:", ok, "| recovery id v =", v)
sys.exit(0 if ok else 1)
