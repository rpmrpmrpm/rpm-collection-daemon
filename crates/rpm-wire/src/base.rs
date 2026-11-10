//! The two text encodings the protocol uses for bytes.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;

/// Base64url without padding. Keys, signatures, challenges and tokens are written this way.
pub fn to_base64url(data: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(data)
}

/// Reads base64url, with or without padding. It also accepts `+` and `/`, as the C# reader does,
/// because that reader turns `-` and `_` into them before it decodes. Returns `None` for text that is
/// empty or is not base64.
pub fn from_base64url(text: &str) -> Option<Vec<u8>> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let normal: String = text
        .trim_end_matches('=')
        .chars()
        .map(|c| match c {
            '+' => '-',
            '/' => '_',
            other => other,
        })
        .collect();
    URL_SAFE_NO_PAD.decode(normal).ok()
}

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Crockford base32, most significant bit first, no padding. A device id is this encoding of the first
/// 16 bytes of the SHA-256 of the device's public key.
pub fn crockford32(data: &[u8]) -> String {
    let mut text = String::with_capacity((data.len() * 8).div_ceil(5));
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for &byte in data {
        buffer = (buffer << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            text.push(CROCKFORD[((buffer >> (bits - 5)) & 31) as usize] as char);
            bits -= 5;
        }
        // Keep only the bits that are still to be written, so the buffer cannot overflow.
        buffer &= (1 << bits) - 1;
    }
    if bits > 0 {
        text.push(CROCKFORD[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    text
}
