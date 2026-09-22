use anyhow::{bail, Result};
use hmac::{Mac, SimpleHmac};
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::{Sha256, Sha512};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Algorithm {
    Sha1,
    Sha256,
    Sha512,
}

impl Algorithm {
    pub fn label(self) -> &'static str {
        match self {
            Self::Sha1 => "SHA-1",
            Self::Sha256 => "SHA-256",
            Self::Sha512 => "SHA-512",
        }
    }
}

pub fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub fn generate_code(
    secret: &str,
    timestamp: u64,
    digits: u8,
    period: u32,
    algorithm: Algorithm,
) -> Result<String> {
    if !(6..=8).contains(&digits) {
        bail!("Digits must be between 6 and 8");
    }
    if period == 0 {
        bail!("Period must be greater than zero");
    }

    let secret_bytes = decode_base32(secret)?;
    let counter = timestamp / u64::from(period);
    let message = counter.to_be_bytes();

    match algorithm {
        Algorithm::Sha1 => generate_hmac::<Sha1>(&secret_bytes, &message, digits),
        Algorithm::Sha256 => generate_hmac::<Sha256>(&secret_bytes, &message, digits),
        Algorithm::Sha512 => generate_hmac::<Sha512>(&secret_bytes, &message, digits),
    }
}

pub fn remaining_seconds(timestamp: u64, period: u32) -> u64 {
    if period == 0 {
        return 0;
    }
    period as u64 - (timestamp % u64::from(period))
}

pub fn normalize_secret(input: &str) -> Result<String> {
    let mut normalized = String::new();
    let mut padding_started = false;

    for character in input.chars() {
        if character.is_whitespace() || character == '-' {
            continue;
        }
        if character == '=' {
            if padding_started {
                bail!("Invalid Base32 padding");
            }
            padding_started = true;
            continue;
        }
        if padding_started {
            bail!("Secret contains characters after Base32 padding");
        }

        let upper = character.to_ascii_uppercase();
        if !upper.is_ascii_uppercase() && !('2'..='7').contains(&upper) {
            bail!("Secret contains an invalid Base32 character");
        }
        normalized.push(upper);
    }

    if normalized.is_empty() {
        bail!("Secret cannot be empty");
    }

    decode_base32(&normalized)?;
    Ok(normalized)
}

pub fn encode_base32(input: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut output = String::with_capacity(((input.len() + 4) / 5) * 8);
    let mut buffer: u32 = 0;
    let mut bits: u32 = 0;
    for byte in input {
        buffer = (buffer << 8) | u32::from(*byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            output.push(ALPHABET[((buffer >> bits) & 0x1f) as usize] as char);
        }
    }
    if bits > 0 {
        output.push(ALPHABET[((buffer << (5 - bits)) & 0x1f) as usize] as char);
    }
    output
}

pub fn decode_base32(input: &str) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut buffer = 0_u32;
    let mut bits = 0_u32;
    let mut padding_started = false;

    for character in input.chars() {
        if character.is_whitespace() || character == '-' {
            continue;
        }
        if character == '=' {
            if padding_started {
                bail!("Invalid Base32 padding");
            }
            padding_started = true;
            continue;
        }
        if padding_started {
            bail!("Secret contains characters after Base32 padding");
        }

        let upper = character.to_ascii_uppercase();
        let value = match upper {
            'A'..='Z' => u32::from(upper as u8 - b'A'),
            '2'..='7' => u32::from(upper as u8 - b'2') + 26,
            _ => bail!("Secret contains an invalid Base32 character"),
        };

        buffer = (buffer << 5) | value;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            output.push(((buffer >> bits) & 0xff) as u8);
            buffer &= (1_u32 << bits) - 1;
        }
    }

    if bits >= 5 {
        bail!("Invalid Base32 secret length");
    }
    if bits > 0 && (buffer & ((1_u32 << bits) - 1)) != 0 {
        bail!("Invalid Base32 padding bits");
    }

    if output.is_empty() {
        bail!("Secret cannot be empty");
    }
    Ok(output)
}

fn generate_hmac<D>(secret: &[u8], message: &[u8], digits: u8) -> Result<String>
where
    D: digest::Digest + digest::core_api::BlockSizeUser,
{
    let mut hmac = SimpleHmac::<D>::new_from_slice(secret)?;
    hmac.update(message);
    let digest = hmac.finalize().into_bytes();
    let offset = (digest[digest.len() - 1] & 0x0f) as usize;
    let binary = (((u32::from(digest[offset]) & 0x7f) as u64) << 24)
        | ((u32::from(digest[offset + 1]) as u64) << 16)
        | ((u32::from(digest[offset + 2]) as u64) << 8)
        | u32::from(digest[offset + 3]) as u64;
    let modulus = 10_u64.pow(u32::from(digits));
    Ok(format!(
        "{:0width$}",
        binary % modulus,
        width = usize::from(digits)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_rfc_test_vector() {
        assert_eq!(
            generate_code(
                "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ",
                59,
                8,
                30,
                Algorithm::Sha1
            )
            .unwrap(),
            "94287082"
        );
    }

    #[test]
    fn normalizes_and_rejects_invalid_secrets() {
        assert_eq!(
            normalize_secret("jbsw y3dp-ehpk3pxp").unwrap(),
            "JBSWY3DPEHPK3PXP"
        );
        assert!(normalize_secret("JBSWY3DPEHPK3PXP=").is_ok());
        assert!(normalize_secret("JBSWY3DPEHPK3PXP!").is_err());
        assert!(normalize_secret("JBSWY3DPEHPK3PXP====").is_err());
    }

    #[test]
    fn rejects_noncanonical_base32_trailing_bits() {
        assert!(decode_base32("MZ").is_err());
    }
}
