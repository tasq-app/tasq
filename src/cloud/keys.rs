//! The keys an account syncs with, all made on the device from the email
//! and the password, none of them ever needing the password again:
//!
//! - **account**: `SHA-256` of the email — the server's name for the
//!   account, so it never learns the email itself.
//! - **master**: `Argon2id(password, salt from the email)`, slow on purpose
//!   so a stolen copy of the server can't try passwords quickly.
//! - **auth** = `HKDF(master, "auth")`: what the device shows the server.
//!   The server keeps only its hash.
//! - **enc** = `HKDF(master, "enc")`: what changes are encrypted with
//!   (XChaCha20-Poly1305). It never leaves the device: the server holds
//!   only ciphertext it can't read.
//!
//! The same email and password give the same keys on every device, so
//! signing in is all a new device needs. Forget the password and nobody,
//! the server included, can read the data again.

use anyhow::{Result, anyhow, bail};
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use sha2::{Digest, Sha256};

use crate::gcal::http;

/// Argon2id cost: 64 MiB and three passes (about half a second), as OWASP
/// suggests. Tests use a cheap setting so they stay fast.
#[cfg(not(test))]
const MEMORY_KIB: u32 = 64 * 1024;
#[cfg(test)]
const MEMORY_KIB: u32 = 64;
const PASSES: u32 = 3;

#[derive(Clone, PartialEq, Eq)]
pub struct Keys {
    /// The account's name on the server (hex).
    pub account: String,
    auth: [u8; 32],
    enc: [u8; 32],
}

impl std::fmt::Debug for Keys {
    // Never print the secrets.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Keys")
            .field("account", &self.account)
            .finish_non_exhaustive()
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

/// How an email is written before it's hashed: trimmed, lower case.
pub fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

impl Keys {
    /// The keys for `email` and `password`.
    pub fn derive(email: &str, password: &str) -> Result<Self> {
        let email = normalize_email(email);
        if !email.contains('@') {
            bail!("that doesn't look like an email");
        }
        if password.chars().count() < 10 {
            bail!("use a password of at least 10 characters");
        }
        let account = hex(&sha256(&[b"tasq account v1\0", email.as_bytes()]));
        let salt = sha256(&[b"tasq salt v1\0", email.as_bytes()]);
        let params =
            Params::new(MEMORY_KIB, PASSES, 1, Some(32)).map_err(|e| anyhow!("argon2: {e}"))?;
        let mut master = [0u8; 32];
        Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
            .hash_password_into(password.as_bytes(), &salt[..16], &mut master)
            .map_err(|e| anyhow!("argon2: {e}"))?;
        let hk = Hkdf::<Sha256>::new(None, &master);
        let mut auth = [0u8; 32];
        let mut enc = [0u8; 32];
        hk.expand(b"tasq auth v1", &mut auth)
            .map_err(|_| anyhow!("hkdf"))?;
        hk.expand(b"tasq enc v1", &mut enc)
            .map_err(|_| anyhow!("hkdf"))?;
        Ok(Self { account, auth, enc })
    }

    /// The `Authorization: Bearer …` value: the account and the auth key.
    pub fn bearer(&self) -> String {
        format!("{}.{}", self.account, hex(&self.auth))
    }

    /// Encrypt `plain` for this account: base64 of a random nonce and the
    /// sealed bytes, bound to the account (a blob can't be replayed into
    /// another one).
    pub fn seal(&self, plain: &[u8]) -> Result<String> {
        let mut nonce = [0u8; 24];
        getrandom::getrandom(&mut nonce).map_err(|e| anyhow!("no randomness: {e}"))?;
        let cipher = XChaCha20Poly1305::new((&self.enc).into());
        let sealed = cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: plain,
                    aad: self.account.as_bytes(),
                },
            )
            .map_err(|_| anyhow!("encryption failed"))?;
        let mut out = nonce.to_vec();
        out.extend_from_slice(&sealed);
        Ok(http::b64(&out))
    }

    /// Decrypt a blob from [`Keys::seal`]; an error when it was sealed with
    /// other keys or changed on the way.
    pub fn open(&self, blob: &str) -> Result<Vec<u8>> {
        let bytes = http::b64url_decode(blob).ok_or_else(|| anyhow!("not base64"))?;
        if bytes.len() < 24 + 16 {
            bail!("too short");
        }
        let (nonce, sealed) = bytes.split_at(24);
        XChaCha20Poly1305::new((&self.enc).into())
            .decrypt(
                XNonce::from_slice(nonce),
                Payload {
                    msg: sealed,
                    aad: self.account.as_bytes(),
                },
            )
            .map_err(|_| anyhow!("can't decrypt: another password, or the data was changed"))
    }

    /// The keys as one line, to keep on the device (not the password).
    pub fn to_secret(&self) -> String {
        format!("{}.{}.{}", self.account, hex(&self.auth), hex(&self.enc))
    }

    pub fn from_secret(s: &str) -> Option<Self> {
        let mut parts = s.trim().split('.');
        let account = parts.next()?.to_string();
        let auth: [u8; 32] = unhex(parts.next()?)?.try_into().ok()?;
        let enc: [u8; 32] = unhex(parts.next()?)?.try_into().ok()?;
        (account.len() == 64 && parts.next().is_none()).then_some(Self { account, auth, enc })
    }
}

/// Seal `secret` under `token` (a random 256-bit token handed to an
/// assistant): the server keeps only this and the token's hash, so what it
/// stores can't be read without the token the assistant shows.
pub fn wrap(secret: &str, token: &str) -> Result<String> {
    let key = sha256(&[b"tasq wrap v1\0", token.as_bytes()]);
    let mut nonce = [0u8; 24];
    getrandom::getrandom(&mut nonce).map_err(|e| anyhow!("no randomness: {e}"))?;
    let sealed = XChaCha20Poly1305::new((&key).into())
        .encrypt(XNonce::from_slice(&nonce), secret.as_bytes())
        .map_err(|_| anyhow!("encryption failed"))?;
    let mut out = nonce.to_vec();
    out.extend_from_slice(&sealed);
    Ok(http::b64url(&out))
}

/// Open what [`wrap`] sealed under `token`.
pub fn unwrap(wrapped: &str, token: &str) -> Option<String> {
    let key = sha256(&[b"tasq wrap v1\0", token.as_bytes()]);
    let bytes = http::b64url_decode(wrapped)?;
    if bytes.len() < 24 + 16 {
        return None;
    }
    let (nonce, sealed) = bytes.split_at(24);
    let plain = XChaCha20Poly1305::new((&key).into())
        .decrypt(XNonce::from_slice(nonce), sealed)
        .ok()?;
    String::from_utf8(plain).ok()
}

/// What the server keeps of a token: its SHA-256, hex.
pub fn token_hash(token: &str) -> String {
    hex(&sha256(&[b"tasq token v1\0", token.as_bytes()]))
}

/// A fresh random token: 32 bytes, hex.
pub fn random_token() -> Result<String> {
    let mut b = [0u8; 32];
    getrandom::getrandom(&mut b).map_err(|e| anyhow!("no randomness: {e}"))?;
    Ok(hex(&b))
}

impl Keys {
    /// Whether these keys sign in to an account stored with `auth_hash`.
    pub fn matches(&self, stored_hash: &str) -> bool {
        auth_hash(&hex(&self.auth)) == stored_hash
    }
}

/// What the server keeps of an auth key: its SHA-256, hex.
pub fn auth_hash(auth_hex: &str) -> String {
    hex(&sha256(&[b"tasq auth hash v1\0", auth_hex.as_bytes()]))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn the_same_email_and_password_give_the_same_keys() {
        let a = Keys::derive("Jose@Example.com ", "correct horse battery").unwrap();
        let b = Keys::derive("jose@example.com", "correct horse battery").unwrap();
        assert_eq!(a, b);
        let other = Keys::derive("jose@example.com", "correct horse staple").unwrap();
        assert_eq!(other.account, a.account);
        assert_ne!(other.bearer(), a.bearer());
        assert!(Keys::derive("jose", "correct horse battery").is_err());
        assert!(Keys::derive("jose@example.com", "short").is_err());
        // Kept and read back.
        assert_eq!(Keys::from_secret(&a.to_secret()), Some(a.clone()));
        assert!(!format!("{a:?}").contains(&a.to_secret()[65..]));
    }

    #[test]
    fn only_the_same_keys_open_a_blob() {
        let k = Keys::derive("a@b.c", "a long password").unwrap();
        let blob = k.seal(b"buy milk").unwrap();
        assert!(!blob.contains("milk"));
        assert_eq!(k.open(&blob).unwrap(), b"buy milk");
        // Sealed twice, never the same bytes.
        assert_ne!(k.seal(b"buy milk").unwrap(), blob);
        let wrong = Keys::derive("a@b.c", "another long password").unwrap();
        assert!(wrong.open(&blob).is_err());
        // One bit changed on the way: refused.
        let mut bytes = http::b64url_decode(&blob).unwrap();
        bytes[30] ^= 1;
        assert!(k.open(&http::b64(&bytes)).is_err());
    }
}
