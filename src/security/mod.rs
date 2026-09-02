//! 機微データ（API キー等）の保存時暗号化
//!
//! `ENCRYPTION_KEY`（32 バイト = 64 桁 hex）が設定されている場合、AES-256-GCM で
//! API キーを暗号化して保存する。未設定の場合は平文のまま保存する（開発用途、
//! 起動時に警告ログを出力する）。

use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{Result, anyhow};
use base64::{Engine, engine::general_purpose::STANDARD as B64};

#[derive(Clone)]
pub struct Crypt {
    cipher: Aes256Gcm,
}

impl Crypt {
    pub fn from_env() -> Option<Result<Self>> {
        let raw = std::env::var("ENCRYPTION_KEY").ok()?;
        Some(Self::from_hex(&raw))
    }

    pub fn from_hex(hex: &str) -> Result<Self> {
        let bytes = hex_decode(hex).ok_or_else(|| anyhow!("invalid hex in ENCRYPTION_KEY"))?;
        if bytes.len() != 32 {
            return Err(anyhow!(
                "ENCRYPTION_KEY must be 32 bytes (64 hex chars), got {}",
                bytes.len()
            ));
        }
        let key: [u8; 32] =
            bytes.try_into().map_err(|_| anyhow!("invalid ENCRYPTION_KEY length"))?;
        Ok(Self::new(key))
    }

    pub fn new(key: [u8; 32]) -> Self {
        Self { cipher: Aes256Gcm::new_from_slice(&key).expect("AES-256-GCM key length is valid") }
    }

    /// 平文を base64(nonce || ciphertext) に暗号化する
    pub fn encrypt(&self, plaintext: &str) -> Result<String> {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng); // 12 bytes nonce
        let ciphertext = self
            .cipher
            .encrypt(&nonce, plaintext.as_bytes())
            .map_err(|_| anyhow!("encryption failed"))?;

        let mut buf = nonce.to_vec();
        buf.extend_from_slice(&ciphertext);
        Ok(B64.encode(buf))
    }

    /// base64(nonce || ciphertext) を復号する
    pub fn decrypt(&self, encoded: &str) -> Result<String> {
        let raw = B64.decode(encoded).map_err(|_| anyhow!("invalid encrypted value encoding"))?;
        if raw.len() < 12 {
            return Err(anyhow!("encrypted value too short"));
        }
        let (nonce_bytes, ciphertext) = raw.split_at(12);
        let plaintext = self
            .cipher
            .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
            .map_err(|_| anyhow!("decryption failed"))?;
        String::from_utf8(plaintext).map_err(|e| anyhow!("decrypted data is not UTF-8: {e}"))
    }
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    for i in (0..s.len()).step_by(2) {
        let byte = u8::from_str_radix(&s[i..i + 2], 16).ok()?;
        out.push(byte);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let crypt = Crypt::new([7u8; 32]);
        let enc = crypt.encrypt("sk-secret-key").unwrap();
        assert_ne!(enc, "sk-secret-key");
        assert!(!enc.contains("sk-secret-key"));
        assert_eq!(crypt.decrypt(&enc).unwrap(), "sk-secret-key");
    }

    #[test]
    fn rejects_short_key() {
        assert!(Crypt::from_hex("abcd").is_err());
    }

    #[test]
    fn rejects_invalid_hex() {
        assert!(Crypt::from_hex("zz").is_err());
    }

    #[test]
    fn different_nonces_produce_different_ciphertext() {
        let crypt = Crypt::new([7u8; 32]);
        let a = crypt.encrypt("same").unwrap();
        let b = crypt.encrypt("same").unwrap();
        assert_ne!(a, b);
        assert_eq!(crypt.decrypt(&a).unwrap(), crypt.decrypt(&b).unwrap());
    }
}
