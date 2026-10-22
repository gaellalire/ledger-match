# Tests

Integration tests will live here, using
[ragger](https://github.com/LedgerHQ/ragger) against the
[speculos](https://github.com/LedgerHQ/speculos) emulator — the setup the
Ledger Dev Tools extension runs with the `pytest` configuration declared in
`ledger_app.toml`.

Planned coverage:

- GET_VERSION / GET_APP_NAME responses
- GET_PUBLIC_KEY with and without on-device confirmation
- SIGN_MATCH happy path (single and multi-chunk payloads) with snapshot
  review navigation on Flex
- Parser edge cases: unknown tags, oversized names, too many players/periods,
  truncated TLV
