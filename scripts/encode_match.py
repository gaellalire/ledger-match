#!/usr/bin/env python3
"""Encode a game result (JSON on stdin) into the canonical v1 TLV hex.

This is the reference encoder for the format specified in docs/APDU.md —
client implementations (TypeScript, Kotlin) should produce byte-identical
output for the same input.

Versus JSON shape:
{
  "timestamp": 1792778400,                // Unix seconds (required)
  "activity": "volleyball",               // optional; also location/competition
  "side1": "Team Dragons",                // optional (but at least one side
  "side2": "Team Sharks",                 //   name or player list is required)
  "score": [[25, 10], [25, 14]],          // optional, signed (side1, side2)
                                          //   points per period
  "referees": ["Pep"],                    // optional
  "side1_players": ["Alice", "Bob"],      // optional
  "side2_players": ["Grace", "Heidi"],    // optional
  "forfeit": "side1"                      // optional: "side1", "side2", "both"
}

The winner (or a draw) is deduced on-device from the score; only forfeits are
explicit. See docs/APDU.md for the deduction rules.

Free-for-all JSON shape (tarot, bridge, races...):
{
  "timestamp": 1792778400,
  "activity": "tarot",
  "participants": [{"name": "Alice", "score": 620}, {"name": "Bob"}]
}

Usage: python3 scripts/encode_match.py < match.json
"""
import json
import sys

FORFEIT_VALUES = {"side1": 0x01, "side2": 0x02, "both": 0x03}


def field(tag: int, value: bytes) -> bytes:
    return bytes([tag]) + len(value).to_bytes(2, "big") + value


def name(s: str) -> bytes:
    return s.encode("utf-8")


def names_block(items: list) -> bytes:
    out = b""
    for n in items:
        out += field(0x01, name(n))
    return out


def score_block(periods: list) -> bytes:
    out = b""
    for a, b in periods:
        out += field(
            0x01,
            int(a).to_bytes(4, "big", signed=True) + int(b).to_bytes(4, "big", signed=True),
        )
    return out


def participants_block(participants: list) -> bytes:
    out = b""
    for part in participants:
        entry = field(0x01, name(part["name"]))
        if part.get("score") is not None:
            entry += field(0x02, int(part["score"]).to_bytes(4, "big", signed=True))
        out += field(0x01, entry)
    return out


def encode(m: dict) -> bytes:
    payload = field(0x01, b"\x01")  # format version
    if m.get("activity"):
        payload += field(0x02, name(m["activity"]))
    payload += field(0x03, int(m["timestamp"]).to_bytes(8, "big"))
    if m.get("location"):
        payload += field(0x04, name(m["location"]))
    if m.get("competition"):
        payload += field(0x05, name(m["competition"]))

    if "participants" in m:  # free-for-all layout
        if m.get("referees"):
            payload += field(0x09, names_block(m["referees"]))
        payload += field(0x0C, participants_block(m["participants"]))
    else:  # versus layout
        if m.get("side1"):
            payload += field(0x06, name(m["side1"]))
        if m.get("side2"):
            payload += field(0x07, name(m["side2"]))
        if m.get("score"):
            payload += field(0x08, score_block(m["score"]))
        if m.get("referees"):
            payload += field(0x09, names_block(m["referees"]))
        if m.get("side1_players"):
            payload += field(0x0A, names_block(m["side1_players"]))
        if m.get("side2_players"):
            payload += field(0x0B, names_block(m["side2_players"]))
        if m.get("forfeit"):
            payload += field(0x0D, bytes([FORFEIT_VALUES[m["forfeit"]]]))
    return payload


if __name__ == "__main__":
    print(encode(json.load(sys.stdin)).hex())
