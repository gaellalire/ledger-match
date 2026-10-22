//! APDU command handlers: GET_VERSION, GET_PUBLIC_KEY and SIGN_MATCH.

use alloc::vec::Vec;
use core::str::FromStr;

use crate::match_result::{self, MAX_MATCH_LEN};
use crate::ui;
use crate::utils::{get_address_hash_from_pubkey, get_pubkey_from_path, Bip32Path};
use crate::AppSW;
use ledger_device_sdk::ecc::{Secp256k1, SeedDerive};
use ledger_device_sdk::hash::{sha3::Keccak256, HashInit};
use ledger_device_sdk::io::{Command, CommandResponse};
use ledger_device_sdk::nbgl::NbglHomeAndSettings;

/// Handler for the GET_VERSION APDU: returns [major, minor, patch].
pub fn handler_get_version(command: Command<'_>) -> Result<CommandResponse<'_>, AppSW> {
    if let Some((major, minor, patch)) = parse_version_string(env!("CARGO_PKG_VERSION")) {
        let mut response = command.into_response();
        response.append(&[major, minor, patch])?;
        Ok(response)
    } else {
        Err(AppSW::VersionParsingFail)
    }
}

fn parse_version_string(input: &str) -> Option<(u8, u8, u8)> {
    // Input should be of the form "major.minor.patch".
    let mut parts = input.split('.');
    let major = u8::from_str(parts.next()?).ok()?;
    let minor = u8::from_str(parts.next()?).ok()?;
    let patch = u8::from_str(parts.next()?).ok()?;
    Some((major, minor, patch))
}

/// Handler for the GET_PUBLIC_KEY APDU command.
///
/// Derives and returns the public key for a given BIP32 path, optionally
/// displaying the corresponding signer address on the device for user
/// verification (P1 = 0x01).
///
/// Response: [pubkey len (0x41), 65-byte uncompressed pubkey, 0x20, 32-byte chaincode].
pub fn handler_get_public_key(
    command: Command<'_>,
    display: bool,
) -> Result<CommandResponse<'_>, AppSW> {
    let data = command.get_data();
    let path: Bip32Path = data.try_into()?;

    let pubkey = get_pubkey_from_path(&path)?;
    let (_, cc) = Secp256k1::derive_from(path.as_ref());

    let comm = command.into_comm();
    if display {
        let address_hash = get_address_hash_from_pubkey(&pubkey);
        if !ui::ui_display_address(comm, &address_hash)? {
            return Err(AppSW::Deny);
        }
    }

    // Return public key (65 bytes uncompressed) then chaincode.
    let mut response = comm.begin_response();
    response.append(&[pubkey.len() as u8])?;
    response.append(&pubkey)?;

    const CHAINCODE_LEN: u8 = 32;
    let code = cc.ok_or(AppSW::KeyDeriveFail)?;
    response.append(&[CHAINCODE_LEN])?;
    response.append(&code.value)?;

    Ok(response)
}

/// Signing context holding state between SIGN_MATCH APDU chunks.
pub struct SignContext {
    raw_match: Vec<u8>,
    path: Bip32Path,
    review_finished: bool,
    pub home: NbglHomeAndSettings,
}

impl SignContext {
    pub fn new() -> SignContext {
        SignContext {
            raw_match: Vec::new(),
            path: Default::default(),
            review_finished: false,
            home: Default::default(),
        }
    }

    /// Whether a review reached its end (approved or rejected).
    pub fn finished(&self) -> bool {
        self.review_finished
    }

    fn reset(&mut self) {
        self.raw_match.clear();
        self.path = Default::default();
        self.review_finished = false;
    }
}

/// Handler for the SIGN_MATCH APDU command.
///
/// Chunk 0 carries the BIP32 derivation path. Chunks 1..N carry the TLV
/// encoded match result; P2 = 0x80 while more chunks follow, 0x00 on the last
/// one. On the last chunk the match is parsed, displayed for clear signing,
/// and its Keccak-256 hash is signed if the user approves.
pub fn handler_sign_match<'a>(
    command: Command<'a>,
    chunk: u8,
    more: bool,
    ctx: &mut SignContext,
) -> Result<CommandResponse<'a>, AppSW> {
    let data = command.get_data();

    // First chunk: BIP32 path.
    if chunk == 0 {
        ctx.reset();
        // This will propagate the error if the path is invalid.
        ctx.path = data.try_into()?;
        return Ok(command.into_response());
    }

    // Next chunks: append data to raw_match.
    if ctx.raw_match.len() + data.len() > MAX_MATCH_LEN {
        ctx.reset();
        return Err(AppSW::WrongLength);
    }
    ctx.raw_match.extend(data);

    if more {
        ctx.review_finished = false;
        return Ok(command.into_response());
    }

    // Last chunk: parse, display, sign.
    let m = match_result::parse(&ctx.raw_match)?;

    let comm = command.into_comm();
    if ui::ui_display_match(comm, &m)? {
        ctx.review_finished = true;
        compute_signature_and_append(comm.begin_response(), ctx)
    } else {
        ctx.review_finished = true;
        Err(AppSW::Deny)
    }
}

fn compute_signature_and_append<'a>(
    mut response: CommandResponse<'a>,
    ctx: &mut SignContext,
) -> Result<CommandResponse<'a>, AppSW> {
    // Hash the raw TLV payload as received; the client must build it in the
    // canonical form documented in docs/APDU.md for signatures to verify.
    let mut keccak256 = Keccak256::new();
    let mut message_hash: [u8; 32] = [0u8; 32];
    keccak256
        .hash(&ctx.raw_match, &mut message_hash)
        .map_err(|_| AppSW::HashFail)?;

    let (sig, siglen, parity) = Secp256k1::derive_from_path(ctx.path.as_ref())
        .deterministic_sign(&message_hash)
        .map_err(|_| AppSW::SignFail)?;

    response
        .append(&[siglen as u8])?
        .append(&sig[..siglen as usize])?
        .append(&[parity as u8])?;
    Ok(response)
}
