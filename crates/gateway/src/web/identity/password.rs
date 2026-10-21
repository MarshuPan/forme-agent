//! Argon2id through RustCrypto's public KDF API; fixed, bounded storage format.
use argon2::{Algorithm, Argon2, Params, Version};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use super::{AuthError, AuthResult};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PasswordRecord {
    schema_version: forme_protocol::SchemaVersion,
    algorithm: String,
    memory_kib: u32,
    iterations: u32,
    parallelism: u32,
    salt: [u8; 16],
    digest: [u8; 32],
}

impl PasswordRecord {
    pub(super) fn new(password: &str) -> AuthResult<Self> {
        validate_password(password)?;
        let mut salt = [0; 16];
        getrandom::fill(&mut salt).map_err(|_| AuthError::Unavailable)?;
        let mut record = Self {
            schema_version: forme_protocol::SchemaVersion(1),
            algorithm: "argon2id-v19".into(),
            memory_kib: 65_536,
            iterations: 3,
            parallelism: 1,
            salt,
            digest: [0; 32],
        };
        record.digest = *record.derive(password)?;
        Ok(record)
    }

    fn derive(&self, password: &str) -> AuthResult<Zeroizing<[u8; 32]>> {
        if self.schema_version != forme_protocol::SchemaVersion(1)
            || self.algorithm != "argon2id-v19"
            || self.memory_kib != 65_536
            || self.iterations != 3
            || self.parallelism != 1
        {
            return Err(AuthError::RecoveryRequired);
        }
        let params = Params::new(self.memory_kib, self.iterations, self.parallelism, Some(32))
            .map_err(|_| AuthError::Unavailable)?;
        let mut output = Zeroizing::new([0; 32]);
        Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
            .hash_password_into(password.as_bytes(), &self.salt, output.as_mut())
            .map_err(|_| AuthError::Unavailable)?;
        Ok(output)
    }

    pub(super) fn verify(&self, password: &str) -> AuthResult<bool> {
        // Keep invalid/unrecognised login attempts on the same expensive path.
        let valid = validate_password(password).is_ok();
        let input = if password.len() <= 1024 { password } else { "" };
        let output = self.derive(input)?;
        Ok(equal_bytes(output.as_ref(), &self.digest) && valid)
    }
}

pub(super) fn equal_bytes(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

pub(super) fn validate_password(password: &str) -> AuthResult<()> {
    if password.len() > 1024 || !(12..=128).contains(&password.chars().count()) {
        return Err(AuthError::InvalidInput);
    }
    Ok(())
}
