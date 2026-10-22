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

## Manual end-to-end test on a real device

Done once on 2026-10-23 against a Ledger Flex (passed). With the app open on
the device, from `target/flex/release`:

1. Basic commands — `ledgerctl send` with one hex APDU per line:

   ```
   e003000000                                        -> 000100 9000   (v0.1.0)
   e004000000                                        -> "ledger-match" 9000
   e005000011048000002c80082b348000000000000000      -> 41 <65B pubkey> 20 <32B chaincode> 9000
   ```

   (last one is GET_PUBLIC_KEY for m/44'/535348'/0'/0)

2. Sign the example soccer match from docs/APDU.md (winner "ABC", loser "XYZ",
   2-1). The device shows the review; approve it:

   ```
   e006008011048000002c80082b348000000000000000      (chunk 0: BIP32 path, more)
   e00601002f01000101020008000000006a18a90004000601000341424305000601000358595a06000b0100080000000200000001
                                                   -> <sigLen> <DER sig> <v> 9000
   ```

3. Verify off-device:

   ```bash
   python3 scripts/verify_signature.py <pubkey-response> <payload-hex> <sign-response>
   ```
