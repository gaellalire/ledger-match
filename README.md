# ledger-match

A [Ledger](https://www.ledger.com/) device app ("Game Signer"), written in
Rust with the
[ledger-device-rust-sdk](https://github.com/LedgerHQ/ledger-device-rust-sdk),
that **signs game results** after an on-device review (clear signing).

Primary target: **Ledger Flex**. Also builds for Stax, Nano X and Nano S+.

The signed payload is deliberately activity-agnostic. Two layouts share one
TLV format (version 1):

- **Versus**: `(activity?, date, location?, competition?, side1?, side2?,
  score?, referees?, side1Players?, side2Players?, forfeit?)` — names, score
  and all lists optional (tennis doubles have no team name, chess needs no
  score). The winner (or a draw) is deduced from the score; forfeits and an explicit winner/draw flag (checked
  against the score, it settles tied scores) are carried explicitly.
- **Free-for-all**: `(activity?, date, location?, participants[{name, score?}],
  referees?)` — tarot, bridge, races.

The versus score is a list of periods (sets / quarters / innings / ...) with
signed points, so a single format covers soccer, volleyball, basketball, golf,
etc. See [docs/APDU.md](docs/APDU.md) for the exact binary format.

## Flow

1. The companion app (expo-web / Android, future work) sends the game result
   to the device over the APDU protocol described in
   [docs/APDU.md](docs/APDU.md).
2. The device shows the date, both sides, the score and the deduced outcome
   for review.
3. On approval, the device signs the Keccak-256 hash of the canonical payload
   with the key at the requested BIP32 path (secp256k1) and returns the
   signature.

## Project layout

```
ledger-match/
├── Cargo.toml            # crate + cargo-ledger metadata (per-device icons)
├── ledger_app.toml       # Ledger Dev Tools extension manifest
├── rust-toolchain.toml   # pinned nightly toolchain
├── .cargo/config.toml    # default target = flex, build-std, release profile
├── build.rs              # generates the Nano home glyph (icon + mask)
├── icons/                # app icons per device (generated, see scripts/)
├── src/
│   ├── main.rs           # entry point, APDU instruction parsing, main loop
│   ├── handlers.rs       # GET_VERSION / GET_PUBLIC_KEY / SIGN_MATCH handlers
│   ├── match_result.rs   # TLV model + parser (the activity-agnostic format)
│   ├── ui.rs             # NBGL home / review / address screens
│   └── utils.rs          # BIP32 path, key derivation, timestamp formatting
├── scripts/make_icons.py # regenerates icons/ (stdlib only)
├── tests/                # speculos/ragger integration tests (future)
└── docs/APDU.md          # protocol & client integration specification
```

## Build

### With the Ledger Dev Tools VS Code extension (recommended)

Install the `ledgerhq.ledger-dev-tools` extension and make sure Docker is
running. Open this folder in VS Code and use the extension's sidebar:
**Build app** → select **flex**. The build runs in Ledger's official builder
container, so no local toolchain setup is needed.

### With the CLI

```bash
cargo install --locked cargo-ledger
cargo ledger setup          # installs the custom target specs
cargo ledger build flex     # or: nanosplus | nanox | stax
```

The sideloadable package is generated under `target/flex/release/`.

### Load on a physical device

On macOS, Docker containers cannot access USB, so loading runs on the host
(the device must be connected, unlocked and on the dashboard):

```bash
pip install ledgerblue        # provides the .apdu script runner
cd target/flex/release
python3 -m ledgerblue.runScript --scp --targetId 0x33300004 --fileName ledger-match.apdu
```

`cargo ledger build` generates `ledger-match.apdu`: a ready-to-replay install
script containing the correct CREATE_APP/LOAD/COMMIT commands for this exact
binary. Accept the "unverified app" warning on the device.

Do **not** use `ledgerctl install` with a hand-written `app.json` manifest:
its code-length computation (`hex span − dataSize`) does not match the
cargo-ledger hex layout (which ends with an `.nvm_data` section), producing a
broken install that freezes the device at launch.

## Regenerating the icons

The committed icons are placeholder art (checkmark in a ring):

```bash
python3 scripts/make_icons.py
```

Replace them with real artwork keeping the same names/sizes (Nano icons and
mask must stay 1-bit GIFs; `build.rs` turns them into the Nano home glyph).

## Client integration (roadmap)

Not implemented yet — the APDU protocol in [docs/APDU.md](docs/APDU.md) is the
stable contract. Planned consumers:

- **expo-web** — TypeScript client over Ledger's Device Management Kit /
  WebUSB transport.
- **android** — Kotlin client over USB/BLE transport.

Both clients only need to implement the TLV encoder + APDU exchange described
in the spec; the signature can be verified off-device against the public key
returned by `GET_PUBLIC_KEY`.
