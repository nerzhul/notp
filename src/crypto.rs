use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use anyhow::{bail, Context, Result};
use argon2::{Algorithm as Argon2Algorithm, Argon2, Params, Version};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

const MAGIC: &[u8; 5] = b"NOTP1";
const MAGIC_V3: &[u8; 5] = b"NOTP3";
const FILE_VERSION: u8 = 1;
const FILE_VERSION_V3: u8 = 3;
const SALT_LENGTH: usize = 16;
const NONCE_LENGTH: usize = 12;
const KEY_LENGTH: usize = 32;
const ARGON2_MEMORY_COST: u32 = 65_536;
const ARGON2_TIME_COST: u32 = 3;
const ARGON2_PARALLELISM: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
struct KdfParams {
    memory_cost: u32,
    time_cost: u32,
    parallelism: u32,
    output_length: u32,
}

impl KdfParams {
    fn production() -> Self {
        Self {
            memory_cost: ARGON2_MEMORY_COST,
            time_cost: ARGON2_TIME_COST,
            parallelism: ARGON2_PARALLELISM,
            output_length: KEY_LENGTH as u32,
        }
    }

    fn argon2(self) -> Result<Argon2<'static>> {
        let params = Params::new(
            self.memory_cost,
            self.time_cost,
            self.parallelism,
            Some(self.output_length as usize),
        )
        .map_err(|error| anyhow::anyhow!("Invalid Argon2 parameters: {}", error))?;
        Ok(Argon2::new(
            Argon2Algorithm::Argon2id,
            Version::V0x13,
            params,
        ))
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct EncryptedFile {
    kdf: KdfParams,
    salt: [u8; SALT_LENGTH],
    nonce: [u8; NONCE_LENGTH],
    ciphertext: Vec<u8>,
}

pub fn open_with_key_and_salt(
    encoded: &[u8],
    password: &str,
) -> Result<([u8; KEY_LENGTH], [u8; SALT_LENGTH], Vec<u8>)> {
    let file = decode_file(encoded)?;
    let mut key = derive_key(password, &file.salt, file.kdf)?;
    let aad = build_aad(&file.kdf, &file.salt, &file.nonce);
    let cipher = match Aes256Gcm::new_from_slice(&key) {
        Ok(cipher) => cipher,
        Err(_) => {
            key.zeroize();
            return Err(anyhow::anyhow!("Invalid encryption key"));
        }
    };
    let plaintext = match cipher.decrypt(
        Nonce::from_slice(&file.nonce),
        Payload {
            msg: file.ciphertext.as_slice(),
            aad: aad.as_slice(),
        },
    ) {
        Ok(plaintext) => plaintext,
        Err(_) => {
            key.zeroize();
            return Err(anyhow::anyhow!("Incorrect password or corrupted vault"));
        }
    };
    Ok((key, file.salt, plaintext))
}

pub fn seal_with_key_and_salt(
    plaintext: &[u8],
    key: &[u8; KEY_LENGTH],
    salt: &[u8; SALT_LENGTH],
) -> Result<Vec<u8>> {
    if key.len() != KEY_LENGTH {
        bail!("Invalid encryption key");
    }
    let mut nonce = [0_u8; NONCE_LENGTH];
    OsRng.fill_bytes(&mut nonce);
    let kdf = KdfParams::production();
    let encrypted = encrypt_with_key(plaintext, key, &kdf, salt, &nonce)?;
    let file = EncryptedFile {
        kdf,
        salt: *salt,
        nonce,
        ciphertext: encrypted,
    };
    let mut encoded = Vec::with_capacity(MAGIC.len() + 1 + 256);
    encoded.extend_from_slice(MAGIC);
    encoded.push(FILE_VERSION);
    encoded.extend_from_slice(&bincode::serialize(&file).context("Unable to serialize the vault")?);
    Ok(encoded)
}

pub fn derive_production_key(password: &str, salt: &[u8; SALT_LENGTH]) -> Result<[u8; KEY_LENGTH]> {
    derive_key(password, salt, KdfParams::production())
}

/// Encrypt `plaintext` with a pre-derived raw AES-256 key (no KDF), wrapping
/// the result in the `NOTP3` envelope used by the Android build. The salt is
/// only used as additional authenticated data — it carries no secret material.
pub fn seal_v3(
    plaintext: &[u8],
    key: &[u8; KEY_LENGTH],
    salt: &[u8; SALT_LENGTH],
) -> Result<Vec<u8>> {
    if key.len() != KEY_LENGTH {
        bail!("Invalid encryption key");
    }
    let mut nonce = [0_u8; NONCE_LENGTH];
    OsRng.fill_bytes(&mut nonce);
    let ciphertext = encrypt_v3(plaintext, key, salt, &nonce)
        .map_err(|_| anyhow::anyhow!("Unable to encrypt the vault"))?;
    let mut encoded =
        Vec::with_capacity(MAGIC_V3.len() + 1 + 1 + SALT_LENGTH + NONCE_LENGTH + ciphertext.len());
    encoded.extend_from_slice(MAGIC_V3);
    encoded.push(FILE_VERSION_V3);
    encoded.push(0); // flags: reserved for future use
    encoded.extend_from_slice(salt);
    encoded.extend_from_slice(&nonce);
    encoded.extend_from_slice(&ciphertext);
    Ok(encoded)
}

/// Decrypt a `NOTP3` envelope with a pre-derived raw AES-256 key. Returns the
/// decorative salt (AAD-only) and the recovered plaintext.
pub fn open_v3(encoded: &[u8], key: &[u8; KEY_LENGTH]) -> Result<([u8; SALT_LENGTH], Vec<u8>)> {
    let header_len = MAGIC_V3.len() + 1 + 1 + SALT_LENGTH + NONCE_LENGTH;
    if encoded.len() < header_len {
        bail!("Truncated v3 vault");
    }
    if &encoded[..MAGIC_V3.len()] != MAGIC_V3 {
        bail!("Unknown v3 envelope");
    }
    let version = encoded[MAGIC_V3.len()];
    if version != FILE_VERSION_V3 {
        bail!("Unsupported v3 vault version");
    }
    let flags = encoded[MAGIC_V3.len() + 1];
    if flags != 0 {
        bail!("Unsupported v3 vault flags");
    }
    let mut salt = [0_u8; SALT_LENGTH];
    salt.copy_from_slice(&encoded[MAGIC_V3.len() + 2..MAGIC_V3.len() + 2 + SALT_LENGTH]);
    let mut nonce = [0_u8; NONCE_LENGTH];
    nonce.copy_from_slice(
        &encoded[MAGIC_V3.len() + 2 + SALT_LENGTH..MAGIC_V3.len() + 2 + SALT_LENGTH + NONCE_LENGTH],
    );
    let ciphertext = &encoded[header_len..];
    let cipher =
        Aes256Gcm::new_from_slice(key).map_err(|_| anyhow::anyhow!("Invalid encryption key"))?;
    let aad = build_v3_aad(&salt, &nonce);
    let plaintext = cipher
        .decrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: ciphertext,
                aad: aad.as_slice(),
            },
        )
        .map_err(|_| anyhow::anyhow!("Incorrect key or corrupted vault"))?;
    Ok((salt, plaintext))
}

fn encrypt_v3(
    plaintext: &[u8],
    key: &[u8; KEY_LENGTH],
    salt: &[u8; SALT_LENGTH],
    nonce: &[u8; NONCE_LENGTH],
) -> Result<Vec<u8>> {
    let cipher =
        Aes256Gcm::new_from_slice(key).map_err(|_| anyhow::anyhow!("Invalid encryption key"))?;
    let aad = build_v3_aad(salt, nonce);
    cipher
        .encrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: plaintext,
                aad: aad.as_slice(),
            },
        )
        .map_err(|_| anyhow::anyhow!("Unable to encrypt the vault"))
}

fn build_v3_aad(salt: &[u8; SALT_LENGTH], nonce: &[u8; NONCE_LENGTH]) -> Vec<u8> {
    let mut aad = Vec::with_capacity(MAGIC_V3.len() + 1 + 1 + SALT_LENGTH + NONCE_LENGTH);
    aad.extend_from_slice(MAGIC_V3);
    aad.push(FILE_VERSION_V3);
    aad.push(0); // flags (must match the value written by seal_v3)
    aad.extend_from_slice(salt);
    aad.extend_from_slice(nonce);
    aad
}

fn derive_key(
    password: &str,
    salt: &[u8; SALT_LENGTH],
    kdf: KdfParams,
) -> Result<[u8; KEY_LENGTH]> {
    let argon2 = kdf.argon2()?;
    let mut key = [0_u8; KEY_LENGTH];
    if let Err(error) = argon2.hash_password_into(password.as_bytes(), salt, &mut key) {
        // Argon2 may have left a partial key buffer behind on failure; wipe it
        // before propagating so the partial secret material does not linger on
        // the stack or in heap copies of the array.
        key.zeroize();
        return Err(anyhow::anyhow!(
            "Failed to derive the master key: {}",
            error
        ));
    }
    Ok(key)
}

fn encrypt_with_key(
    plaintext: &[u8],
    key: &[u8; KEY_LENGTH],
    kdf: &KdfParams,
    salt: &[u8; SALT_LENGTH],
    nonce: &[u8; NONCE_LENGTH],
) -> Result<Vec<u8>> {
    let cipher =
        Aes256Gcm::new_from_slice(key).map_err(|_| anyhow::anyhow!("Invalid encryption key"))?;
    let aad = build_aad(kdf, salt, nonce);
    cipher
        .encrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: plaintext,
                aad: aad.as_slice(),
            },
        )
        .map_err(|_| anyhow::anyhow!("Unable to encrypt the vault"))
}

fn decode_file(encoded: &[u8]) -> Result<EncryptedFile> {
    if encoded.len() < MAGIC.len() + 1 {
        bail!("Truncated vault");
    }
    if &encoded[..MAGIC.len()] != MAGIC {
        bail!("Unknown vault format");
    }
    if encoded[MAGIC.len()] != FILE_VERSION {
        bail!("Unsupported vault version");
    }
    let file = bincode::deserialize::<EncryptedFile>(&encoded[MAGIC.len() + 1..])
        .context("Invalid vault")?;
    if file.kdf.output_length as usize != KEY_LENGTH {
        bail!("Unsupported key size");
    }
    if !(8_192..=1_048_576).contains(&file.kdf.memory_cost)
        || !(1..=10).contains(&file.kdf.time_cost)
        || !(1..=8).contains(&file.kdf.parallelism)
    {
        bail!("Unsupported KDF parameters");
    }
    Ok(file)
}

fn build_aad(kdf: &KdfParams, salt: &[u8; SALT_LENGTH], nonce: &[u8; NONCE_LENGTH]) -> Vec<u8> {
    let mut aad = Vec::with_capacity(MAGIC.len() + 1 + 16 + 4 + 4 + 4 + 4 + 12);
    aad.extend_from_slice(MAGIC);
    aad.push(FILE_VERSION);
    aad.extend_from_slice(&kdf.memory_cost.to_le_bytes());
    aad.extend_from_slice(&kdf.time_cost.to_le_bytes());
    aad.extend_from_slice(&kdf.parallelism.to_le_bytes());
    aad.extend_from_slice(&kdf.output_length.to_le_bytes());
    aad.extend_from_slice(salt);
    aad.extend_from_slice(nonce);
    aad
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_blob(plaintext: &[u8], password: &str) -> (Vec<u8>, [u8; KEY_LENGTH]) {
        let mut salt = [0_u8; SALT_LENGTH];
        OsRng.fill_bytes(&mut salt);
        let key = derive_production_key(password, &salt).unwrap();
        let encoded = seal_with_key_and_salt(plaintext, &key, &salt).unwrap();
        (encoded, key)
    }

    #[test]
    fn encrypts_and_decrypts() {
        let plaintext = b"secret data";
        let (encoded, key) = test_blob(plaintext, "correct horse battery staple");
        let (derived_key, _, decoded) =
            open_with_key_and_salt(&encoded, "correct horse battery staple").unwrap();
        assert_eq!(derived_key, key);
        assert_eq!(decoded, plaintext);
        assert!(open_with_key_and_salt(&encoded, "wrong password").is_err());
    }

    #[test]
    fn detects_tampering() {
        let (mut encoded, _) = test_blob(b"secret data", "password");
        let last = encoded.len() - 1;
        encoded[last] ^= 1;
        let result = open_with_key_and_salt(&encoded, "password");
        assert!(result.is_err());
    }

    fn encode_raw(
        plaintext: &[u8],
        kdf: KdfParams,
        salt: [u8; SALT_LENGTH],
        nonce: [u8; NONCE_LENGTH],
    ) -> Vec<u8> {
        // Derive a real key using production-grade Argon2 params so the bytes
        // are well-formed, then splice an out-of-range KdfParams into the
        // serialized envelope so we exercise decode_file()'s clamp check.
        let production = KdfParams::production();
        let mut key = [0_u8; KEY_LENGTH];
        let argon = production.argon2().unwrap();
        argon
            .hash_password_into(b"password", &salt, &mut key)
            .unwrap();
        let ciphertext = encrypt_with_key(plaintext, &key, &kdf, &salt, &nonce).unwrap();
        key.zeroize();
        let file = EncryptedFile {
            kdf,
            salt,
            nonce,
            ciphertext,
        };
        let mut encoded = Vec::new();
        encoded.extend_from_slice(MAGIC);
        encoded.push(FILE_VERSION);
        encoded.extend_from_slice(&bincode::serialize(&file).unwrap());
        encoded
    }

    #[test]
    fn rejects_out_of_range_kdf_memory_cost() {
        let salt = [0xab; SALT_LENGTH];
        let nonce = [0x42; NONCE_LENGTH];
        let mut too_low = KdfParams::production();
        too_low.memory_cost = 4_096; // below the 8_192 minimum
        let encoded = encode_raw(b"data", too_low, salt, nonce);
        assert!(
            open_with_key_and_salt(&encoded, "password").is_err(),
            "memory_cost=4096 must be rejected"
        );

        let mut too_high = KdfParams::production();
        too_high.memory_cost = 2_097_152; // above 1_048_576 max
        let encoded = encode_raw(b"data", too_high, salt, nonce);
        assert!(
            open_with_key_and_salt(&encoded, "password").is_err(),
            "memory_cost=2097152 must be rejected"
        );
    }

    #[test]
    fn rejects_out_of_range_kdf_time_cost() {
        let salt = [0xab; SALT_LENGTH];
        let nonce = [0x42; NONCE_LENGTH];
        let mut too_low = KdfParams::production();
        too_low.time_cost = 0;
        let encoded = encode_raw(b"data", too_low, salt, nonce);
        assert!(
            open_with_key_and_salt(&encoded, "password").is_err(),
            "time_cost=0 must be rejected"
        );

        let mut too_high = KdfParams::production();
        too_high.time_cost = 11;
        let encoded = encode_raw(b"data", too_high, salt, nonce);
        assert!(
            open_with_key_and_salt(&encoded, "password").is_err(),
            "time_cost=11 must be rejected"
        );
    }

    #[test]
    fn rejects_out_of_range_kdf_parallelism() {
        let salt = [0xab; SALT_LENGTH];
        let nonce = [0x42; NONCE_LENGTH];
        let mut too_low = KdfParams::production();
        too_low.parallelism = 0;
        let encoded = encode_raw(b"data", too_low, salt, nonce);
        assert!(
            open_with_key_and_salt(&encoded, "password").is_err(),
            "parallelism=0 must be rejected"
        );

        let mut too_high = KdfParams::production();
        too_high.parallelism = 9;
        let encoded = encode_raw(b"data", too_high, salt, nonce);
        assert!(
            open_with_key_and_salt(&encoded, "password").is_err(),
            "parallelism=9 must be rejected"
        );
    }

    #[test]
    fn rejects_out_of_range_kdf_output_length() {
        let salt = [0xab; SALT_LENGTH];
        let nonce = [0x42; NONCE_LENGTH];
        let mut too_short = KdfParams::production();
        too_short.output_length = 16;
        let encoded = encode_raw(b"data", too_short, salt, nonce);
        assert!(
            open_with_key_and_salt(&encoded, "password").is_err(),
            "output_length=16 must be rejected"
        );
    }

    #[test]
    fn v3_round_trip_and_rejects_wrong_key() {
        let plaintext = b"v3 secret data";
        let mut key = [0_u8; KEY_LENGTH];
        OsRng.fill_bytes(&mut key);
        let mut salt = [0_u8; SALT_LENGTH];
        OsRng.fill_bytes(&mut salt);
        let encoded = seal_v3(plaintext, &key, &salt).unwrap();
        // Header sanity: the leading 5 bytes must spell NOTP3 so storage.rs
        // can discriminate v3 vaults from v1 vaults by a single peek.
        assert_eq!(&encoded[..5], b"NOTP3");
        let (recovered_salt, decoded) = open_v3(&encoded, &key).unwrap();
        assert_eq!(recovered_salt, salt);
        assert_eq!(decoded, plaintext);

        let mut wrong_key = key;
        wrong_key[0] ^= 0x01;
        assert!(open_v3(&encoded, &wrong_key).is_err());
    }

    #[test]
    fn v3_detects_tampering() {
        let plaintext = b"v3 integrity check";
        let mut key = [0_u8; KEY_LENGTH];
        OsRng.fill_bytes(&mut key);
        let mut salt = [0_u8; SALT_LENGTH];
        OsRng.fill_bytes(&mut salt);
        let encoded = seal_v3(plaintext, &key, &salt).unwrap();
        let mut tampered = encoded.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        assert!(open_v3(&tampered, &key).is_err());
    }
}
