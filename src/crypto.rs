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
const FILE_VERSION: u8 = 1;
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

fn derive_key(
    password: &str,
    salt: &[u8; SALT_LENGTH],
    kdf: KdfParams,
) -> Result<[u8; KEY_LENGTH]> {
    let argon2 = kdf.argon2()?;
    let mut key = [0_u8; KEY_LENGTH];
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|error| anyhow::anyhow!("Failed to derive the master key: {}", error))?;
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
}
