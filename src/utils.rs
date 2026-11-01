use alloc::vec::Vec;

use crate::AppSW;
use ledger_device_sdk::ecc::{Secp256k1, SeedDerive};
use ledger_device_sdk::hash::{sha3::Keccak256, HashInit};

/// BIP32 derivation path stored as a vector of u32 components.
///
/// Each component represents one level in the path (e.g., m/44'/535348'/0'/0/0
/// has 5 components). Hardened derivation is indicated by setting the high bit
/// (>= 0x80000000).
#[derive(Default)]
pub struct Bip32Path(Vec<u32>);

impl AsRef<[u32]> for Bip32Path {
    fn as_ref(&self) -> &[u32] {
        &self.0
    }
}

impl TryFrom<&[u8]> for Bip32Path {
    type Error = AppSW;

    /// Constructs a [`Bip32Path`] from APDU-encoded bytes.
    ///
    /// # Format
    ///
    /// - First byte: number of path components (e.g., 5 for m/44'/535348'/0'/0/0)
    /// - Remaining bytes: big-endian u32 components (4 bytes each)
    ///
    /// # Example
    ///
    /// For path m/44'/535348'/0'/0:
    /// ```text
    /// [0x04, 0x8000002C, 0x80082B34, 0x80000000, 0x00000000]
    /// ```
    fn try_from(data: &[u8]) -> Result<Self, Self::Error> {
        // Check data length
        if data.is_empty() // At least the length byte is required
            || (data[0] as usize * 4 != data.len() - 1)
        {
            return Err(AppSW::WrongApduLength);
        }

        Ok(Bip32Path(
            data[1..]
                .chunks(4)
                .map(|chunk| u32::from_be_bytes(chunk.try_into().unwrap()))
                .collect(),
        ))
    }
}

/// Derive the raw public key from a BIP32 path.
///
/// Returns the uncompressed secp256k1 public key (65 bytes):
/// - First byte: 0x04 (uncompressed marker)
/// - Next 32 bytes: X coordinate
/// - Last 32 bytes: Y coordinate
pub fn get_pubkey_from_path(path: &Bip32Path) -> Result<[u8; 65], AppSW> {
    let (k, _) = Secp256k1::derive_from(path.as_ref());
    let pk = k.public_key().map_err(|_| AppSW::KeyDeriveFail)?;
    Ok(pk.pubkey)
}

/// Compute the Keccak-256 hash of a public key for address derivation.
///
/// Ethereum-style address computation:
/// 1. Take uncompressed pubkey (65 bytes)
/// 2. Skip first byte (0x04 marker)
/// 3. Hash the remaining 64 bytes with Keccak-256
/// 4. Take the last 20 bytes of the hash as the address
///
/// Returns the full 32-byte hash; callers use its last 20 bytes.
pub fn get_address_hash_from_pubkey(pubkey: &[u8; 65]) -> [u8; 32] {
    let mut keccak256 = Keccak256::new();
    let mut address: [u8; 32] = [0u8; 32];
    // Hash pubkey excluding first byte (0x04 uncompressed marker)
    let _ = keccak256.hash(&pubkey[1..], &mut address);
    address
}

/// Format a Unix timestamp (UTC) as "YYYY-MM-DD HH:MM" for on-device review.
///
/// Uses the civil-from-days algorithm (Howard Hinnant) so no date library is
/// needed on-device. Timestamps before 1970 or after year 9999 are clamped
/// into range for display; the signature still covers the raw value.
pub fn format_timestamp(ts: u64) -> alloc::string::String {
    let days = (ts / 86_400) as i64;
    let secs = ts % 86_400;
    let (hh, mm) = (secs / 3600, (secs % 3600) / 60);

    // civil from days
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = if m <= 2 { y + 1 } else { y };

    alloc::format!("{:04}-{:02}-{:02} {:02}:{:02} UTC", year, m, d, hh, mm)
}

/// Fold text to plain ASCII for display: the device font has no accented
/// glyphs and renders UTF-8 bytes one by one. The signature still covers the
/// original text.
pub fn ascii_fold(s: &str) -> alloc::string::String {
    let mut out = alloc::string::String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' => out.push('A'),
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => out.push('a'),
            'Æ' => out.push_str("AE"),
            'æ' => out.push_str("ae"),
            'Ç' => out.push('C'),
            'ç' => out.push('c'),
            'È' | 'É' | 'Ê' | 'Ë' => out.push('E'),
            'è' | 'é' | 'ê' | 'ë' => out.push('e'),
            'Ì' | 'Í' | 'Î' | 'Ï' => out.push('I'),
            'ì' | 'í' | 'î' | 'ï' => out.push('i'),
            'Ñ' => out.push('N'),
            'ñ' => out.push('n'),
            'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ø' => out.push('O'),
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => out.push('o'),
            'Œ' => out.push_str("OE"),
            'œ' => out.push_str("oe"),
            'Ù' | 'Ú' | 'Û' | 'Ü' => out.push('U'),
            'ù' | 'ú' | 'û' | 'ü' => out.push('u'),
            'Ý' | 'Ÿ' => out.push('Y'),
            'ý' | 'ÿ' => out.push('y'),
            'ß' => out.push_str("ss"),
            '’' | '‘' => out.push('\''),
            '“' | '”' => out.push('"'),
            '–' | '—' => out.push('-'),
            c if c.is_ascii() => out.push(c),
            _ => out.push('?'),
        }
    }
    out
}
