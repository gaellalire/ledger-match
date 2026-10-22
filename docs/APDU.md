# ledger-match APDU protocol

This document is the stable contract between the `ledger-match` device app and
its companion clients (expo-web, Android, ...). It is transport-agnostic: any
Ledger transport (USB HID, WebUSB, BLE, Device Management Kit) can carry these
APDUs.

- CLA: `0xE0` for all commands
- Status words: see [Status words](#status-words)
- Signature scheme: Keccak-256 + secp256k1 deterministic ECDSA (RFC 6979)

## Commands

| INS  | Name           | P1        | P2   | Data                          |
|------|----------------|-----------|------|-------------------------------|
| 0x03 | GET_VERSION    | 0x00      | 0x00 | —                             |
| 0x04 | GET_APP_NAME   | 0x00      | 0x00 | —                             |
| 0x05 | GET_PUBLIC_KEY | see below | 0x00 | BIP32 path                    |
| 0x06 | SIGN_MATCH     | chunk     | see below | path chunk / payload chunk |

### GET_VERSION (0x03)

Response: `[major, minor, patch]` (3 bytes).

### GET_APP_NAME (0x04)

Response: ASCII app name (`ledger-match`).

### GET_PUBLIC_KEY (0x05)

- P1 = `0x00`: return the key without on-device confirmation
- P1 = `0x01`: display the signer address on-device and require user approval

Data: BIP32 path, encoded as:

```
[0x05] [0x8000002C] [0x80082B34] [0x80000000] [0x00000000] [0x00000000]
  len     44'        535348'      0'           0            0
```

(1 length byte, then big-endian u32 components; hardened = `| 0x80000000`).
Default path: `m/44'/535348'/0'/0/0`.

Response:

```
[0x41] [65-byte uncompressed public key] [0x20] [32-byte chaincode]
```

### SIGN_MATCH (0x06)

Chunked command:

1. **Chunk 0** — P1 = `0x00`, P2 = `0x80`; data = BIP32 path (same encoding as
   GET_PUBLIC_KEY). No payload in this chunk.
2. **Chunks 1..N** — P1 = chunk index (max `0x0F`), P2 = `0x80` while more
   chunks follow, `0x00` on the last chunk; data = next slice of the TLV
   payload (max 255 bytes per chunk).

After the last chunk the device parses the payload, shows the match for
review, and on approval returns:

```
[sigLen] [DER-encoded ECDSA signature] [v: recovery id (0 or 1)]
```

The signed message is `keccak256(raw_tlv_payload)` exactly as received —
clients must serialize in the canonical form below for signatures to verify.

## Match result TLV format

Every field is `tag (1 byte) | length (u16 BE) | value`. Unknown tags must be
skipped by parsers (forward compatibility). Nested values (team, score) are
themselves TLV sequences.

Canonical form (required for signing): fields in ascending tag order, each tag
at most once, UTF-8 strings, no extra padding.

### Top level

| Tag  | Name      | Value                                             | Required |
|------|-----------|---------------------------------------------------|----------|
| 0x01 | version   | 1 byte, `0x01`                                    | yes      |
| 0x02 | timestamp | u64 BE, Unix seconds                              | yes      |
| 0x03 | sport     | UTF-8, e.g. `"volleyball"`                        | no       |
| 0x04 | winner    | team TLV                                          | yes      |
| 0x05 | loser     | team TLV                                          | yes      |
| 0x06 | score     | score TLV                                         | yes      |

### Team (tags inside 0x04 / 0x05 values)

| Tag  | Name   | Value                       | Required            |
|------|--------|-----------------------------|---------------------|
| 0x01 | name   | UTF-8, ≤ 32 bytes           | yes                 |
| 0x02 | player | UTF-8, ≤ 32 bytes, repeated | no (≤ 24 per team)  |

### Score (tags inside 0x06 value)

| Tag  | Name   | Value                                        | Required |
|------|--------|----------------------------------------------|----------|
| 0x01 | period | 8 bytes: winner points u32 BE + loser u32 BE | yes, ≥ 1 (≤ 16) |

One period for single-score sports (soccer: `2-1`), several for
set/quarter-based sports (volleyball: `25-10 25-14 15-25 25-22`).

### Limits

| Limit                    | Value  |
|--------------------------|--------|
| Total payload            | 3072 B |
| Team / player name       | 32 B   |
| Players per team         | 24     |
| Score periods            | 16     |
| Chunks (incl. path)      | 16     |

Players beyond what fits the review screen are truncated on display but are
still fully covered by the signature.

### Example

Minimal soccer match, winner "ABC", loser "XYZ", score 2-1,
timestamp 1780000000:

```
01 0001 01                                        version = 1
02 0008 000000006A18A900                          timestamp
04 0006 010003414243                              winner: name "ABC"
05 0006 01000358595A                              loser:  name "XYZ"
06 000B 0100080000000200000001                    score: one period 2-1
```

Full payload (47 bytes):

```
01000101020008 000000006A18A900 04000601000341424305000601000358595A06000B0100080000000200000001
```

## Status words

| SW     | Meaning                              |
|--------|--------------------------------------|
| 0x9000 | OK                                   |
| 0x6985 | User rejected the review             |
| 0x6A86 | Wrong P1/P2                          |
| 0x6B00 | Wrong APDU length                    |
| 0x6D00 | INS not supported                    |
| 0x6E00 | CLA not supported                    |
| 0x6F00 | Communication error                  |
| 0xB004 | Match payload too long               |
| 0xB005 | Match payload parsing failed         |
| 0xB006 | Hashing failed                       |
| 0xB008 | Signing failed                       |
| 0xB009 | Key derivation failed                |
| 0xB00A | Version parsing failed               |

## Client integration guide

Companion apps only need two small modules:

1. **TLV encoder** producing the canonical form above.
2. **APDU exchange** implementing the four commands.

Recommended stacks:

- **expo-web (TypeScript)** — Ledger's
  [Device Management Kit](https://github.com/LedgerHQ/device-management-kit)
  (`@ledgerhq/device-signer-kit-*`) or, at a lower level,
  `@ledgerhq/hw-transport-webusb` in the browser and
  `@ledgerhq/react-native-hw-transport-ble` in Expo. Wrap the TLV encoder +
  APDU exchange in a small `MatchSigner` class so the transport can be swapped.
- **android (Kotlin)** — a thin transport layer over USB/BLE sending the same
  byte sequences; mirror the TLV encoder in Kotlin.

Verifying a signature off-device: recompute `keccak256(canonical_payload)`,
then verify the DER signature against the uncompressed public key from
GET_PUBLIC_KEY (or recover the signer Ethereum-style using `v`).
