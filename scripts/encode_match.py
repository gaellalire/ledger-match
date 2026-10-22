#!/usr/bin/env python3
"""Encode a match result (JSON on stdin) into the canonical TLV payload hex.

This is the reference encoder for the format specified in docs/APDU.md —
client implementations (TypeScript, Kotlin) should produce byte-identical
output for the same input.

JSON shape:
{
  "timestamp": 1792778400,                // Unix seconds
  "sport": "volleyball",                  // optional
  "winner": {"name": "Team Dragons", "players": ["Alice", "Bob"]},
  "loser":  {"name": "Team Sharks",  "players": ["Grace", "Heidi"]},
  "score": [[25, 10], [25, 14], [15, 25], [25, 22]]  // per-period [w, l]
}

Usage: python3 scripts/encode_match.py < match.json
"""
import json
import sys


def field(tag: int, value: bytes) -> bytes:
    return bytes([tag]) + len(value).to_bytes(2, "big") + value


def team(t: dict) -> bytes:
    out = field(0x01, t["name"].encode("utf-8"))
    for player in t.get("players", []):
        out += field(0x02, player.encode("utf-8"))
    return out


def score(periods: list) -> bytes:
    out = b""
    for w, l in periods:
        out += field(0x01, w.to_bytes(4, "big") + l.to_bytes(4, "big"))
    return out


def encode(m: dict) -> bytes:
    payload = field(0x01, b"\x01")  # format version
    payload += field(0x02, int(m["timestamp"]).to_bytes(8, "big"))
    if m.get("sport"):
        payload += field(0x03, m["sport"].encode("utf-8"))
    payload += field(0x04, team(m["winner"]))
    payload += field(0x05, team(m["loser"]))
    payload += field(0x06, score(m["score"]))
    return payload


if __name__ == "__main__":
    print(encode(json.load(sys.stdin)).hex())
