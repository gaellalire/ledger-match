# ledger-match APDU protocol

This document is the stable contract between the `ledger-match` device app
("Game Signer") and its companion clients (expo-web, Android, ...). It is
transport-agnostic: any Ledger transport (USB HID, WebUSB, BLE, Device
Management Kit) can carry these APDUs.

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
[0x04] [0x8000002C] [0x80082B34] [0x80000000] [0x00000000]
  len     44'        535348'      0'           0
```

(1 length byte, then big-endian u32 components; hardened = `| 0x80000000`).
Default path: `m/44'/535348'/0'/0`.

The OS enforces the derivation prefixes declared in the app manifest: a
requested path may extend a declared prefix by at most one level; anything
deeper is rejected with status `0x6E03`. Declared prefixes:
`44'/535348'`, `44'/535348'/0'`, `44'/535348'/0'/0'` (so up to
`m/44'/535348'/0'/0'/x`).

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

After the last chunk the device parses the payload, shows the game result for
review, and on approval returns:

```
[sigLen] [DER-encoded ECDSA signature] [v: recovery id (0 or 1)]
```

The signed message is `keccak256(raw_tlv_payload)` exactly as received —
clients must serialize in the canonical form below for signatures to verify.

**Post-sign modal status window:** after the response, the device shows a
modal "signed/rejected" status page for ~3 seconds. APDUs sent during that
window may be answered with `0x6901` (command not accepted) — clients should
wait for the home screen (or ~3 s) before issuing the next command.

## Result TLV format (v1)

Every field is `tag (1 byte) | length (u16 BE) | value`. Unknown tags must be
skipped by parsers (forward compatibility). Nested blocks (score, referees,
player lists, participants) are themselves TLV sequences.

Canonical form (required for signing): fields in ascending tag order, each tag
at most once, UTF-8 strings, no extra padding.

Two layouts share the format: **versus** (two sides) and **free-for-all**
(flat participant list). A payload is free-for-all if and only if it contains
a participants block; versus-only fields must then be absent.

### Common fields

| Tag  | Name        | Value                               | Required |
|------|-------------|-------------------------------------|----------|
| 0x01 | version     | 1 byte, `0x01`                      | yes      |
| 0x02 | activity    | UTF-8, capitalized, e.g. `"Volleyball"`, `"Tarot"` | no |
| 0x03 | timestamp   | u64 BE, Unix seconds                | yes      |
| 0x04 | location    | UTF-8 (max 64 B)                    | no       |
| 0x05 | competition | UTF-8 (max 64 B), e.g. `"Roland Garros 2026 QF"` | no |

### Versus layout

| Tag  | Name           | Value             | Required |
|------|----------------|-------------------|----------|
| 0x06 | side 1 name    | UTF-8             | no*      |
| 0x07 | side 2 name    | UTF-8             | no*      |
| 0x08 | score          | score block       | no       |
| 0x09 | referees       | name-list block   | no       |
| 0x0A | side 1 players | name-list block   | no       |
| 0x0B | side 2 players | name-list block   | no       |
| 0x0D | result flag    | 1 byte, see below | no       |

*At least one side designation (a name or a player list) is required.

Score block: repeated `0x01` fields, each 8 bytes = side 1 points (i32 BE) +
side 2 points (i32 BE). One period for single-score sports, several for
set/quarter-based sports. Points are **signed** (golf, card games). Decimal
scores are scaled by the client (e.g. ×100) — the format carries integers.

Result flag (forfeit and winner share this field): `0x01` = side 1 forfeited,
`0x02` = side 2 forfeited, `0x03` = both sides forfeited, `0x04` = side 1
won, `0x05` = side 2 won, `0x06` = draw. Any other value is rejected. A
winner/draw flag (`0x04`-`0x06`) is verified against the score: with a
decisive score it must name the same outcome, otherwise the payload is
rejected. With a tied score (same periods won and a tied last period) any of the three is
accepted, which tells a draw apart from a tie-break win (extra time, penalty
shoot-out...). Without a score it is accepted as is.

**Outcome deduction.** The device derives the displayed outcome and rejects
inconsistent flags:

1. If a forfeit flag is present it wins: "X won by forfeit" (the
   non-forfeiting side) or "Both sides forfeited".
2. Otherwise, if a winner/draw flag is present it is displayed (it has been
   checked against the score, see above).
3. Otherwise, with a score: the side winning more periods wins; on a tie, only
   the last period counts (earlier points are ignored); if it is tied too the
   game is a draw. E.g. `25-10 15-25 11-11` is a tied score.
4. Without a score and without flag, no outcome is shown.

Because ties on periods fall back to the last period, sports decided by **total
points across periods** (basketball quarters, two-leg aggregates...) must be
encoded as a **single period** carrying the final totals; per-period encoding
is reserved for sports decided by periods won (volleyball/tennis sets).

Name-list block: repeated `0x01` fields, each a UTF-8 name.

### Free-for-all layout

| Tag  | Name         | Value              | Required |
|------|--------------|--------------------|----------|
| 0x09 | referees     | name-list block    | no       |
| 0x0C | participants | participants block | yes (≥1) |

Participants block: repeated `0x01` fields, each a nested TLV sequence:

| Tag  | Name  | Value           | Required |
|------|-------|-----------------|----------|
| 0x01 | name  | UTF-8           | yes      |
| 0x02 | score | i32 BE (signed) | no       |

The device displays each participant with their score; ranking/winner
computation is left to the client (highest or lowest wins depends on the
activity).

### Limits

| Limit                                                    | Value  |
|----------------------------------------------------------|--------|
| Total payload                                            | 3072 B |
| Name / text field (activity, sides, players, referees)   | 32 B   |
| Location, competition                                    | 64 B   |
| Items per list (players, participants, referees)         | 24     |
| Score periods                                            | 16     |
| Chunks (incl. path)                                      | 16     |

Long player/referee lists are truncated on the review display but are still
fully covered by the signature.

### Example (versus)

Soccer: side 1 "ABC", side 2 "XYZ", score 2-1, timestamp 1780000000 (the
device deduces "ABC beats XYZ"):

```
01 0001 01                                      version = 1
02 0006 536F63636572                            activity "Soccer"
03 0008 000000006A18A900                        timestamp
06 0003 414243                                  side 1 "ABC"
07 0003 58595A                                  side 2 "XYZ"
08 000B 0100080000000200000001                  score: one period 2-1
```

Full payload (54 bytes):

```
01000101020006536f63636572030008000000006a18a90006000341424307000358595a08000b0100080000000200000001
```

A forfeit of side 2 would add `0D 0001 02` ("ABC won by forfeit"); a 2-2 score
would display "Draw between ABC and XYZ", unless the flag `0D 0001 04` says
side 1 won (e.g. on penalties).

### Example (free-for-all)

Tarot, 4 participants, timestamp 1780000000 (95 bytes):

```
01 0001 01                                      version = 1
02 0005 5461726F74                              activity "Tarot"
03 0008 000000006A18A900                        timestamp
0C 0045                                         participants block (69 bytes):
  01 000F 010005416C6963650200040000026C        Alice, 620
  01 000D 010003426F62020004FFFFFF88            Bob, -120
  01 000F 0100054361726F6C0200040000002D        Carol, 45
  01 000E 01000444617665020004FFFFFDDF          Dave, -545
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
| 0xB004 | Result payload too long              |
| 0xB005 | Result payload parsing failed        |
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
  APDU exchange in a small `GameSigner` class so the transport can be swapped.
- **android (Kotlin)** — a thin transport layer over USB/BLE sending the same
  byte sequences; mirror the TLV encoder in Kotlin.

Verifying a signature off-device: recompute `keccak256(canonical_payload)`,
then verify the DER signature against the uncompressed public key from
GET_PUBLIC_KEY (or recover the signer Ethereum-style using `v`).
