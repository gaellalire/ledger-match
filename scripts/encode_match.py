#!/usr/bin/env python3
"""Encode a competition result (JSON on stdin) into the canonical v2 TLV hex.

This is the reference encoder for the format specified in docs/APDU.md —
client implementations (TypeScript, Kotlin) should produce byte-identical
output for the same input.

Versus JSON shape:
{
  "timestamp": 1792778400,                // Unix seconds (required)
  "activity": "volleyball",               // optional; also location/competition
  "winner": "Team Dragons",               // optional (but at least one side
  "loser": "Team Sharks",                 //   name or player list is required)
  "score": [[25, 10], [25, 14]],          // optional, signed points per period
  "referees": ["Pep"],                    // optional
  "winner_players": ["Alice", "Bob"],     // optional
  "loser_players": ["Grace", "Heidi"]     // optional
}

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
    for w, l in periods:
        out += field(
            0x01,
            int(w).to_bytes(4, "big", signed=True) + int(l).to_bytes(4, "big", signed=True),
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
    payload = field(0x01, b"\x02")  # format version
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
        if m.get("winner"):
            payload += field(0x06, name(m["winner"]))
        if m.get("loser"):
            payload += field(0x07, name(m["loser"]))
        if m.get("score"):
            payload += field(0x08, score_block(m["score"]))
        if m.get("referees"):
            payload += field(0x09, names_block(m["referees"]))
        if m.get("winner_players"):
            payload += field(0x0A, names_block(m["winner_players"]))
        if m.get("loser_players"):
            payload += field(0x0B, names_block(m["loser_players"]))
    return payload


if __name__ == "__main__":
    print(encode(json.load(sys.stdin)).hex())
