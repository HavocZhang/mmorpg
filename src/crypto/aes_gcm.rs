//! AES-GCM 加解密模块
//!
//! 用于游戏私有协议的加密与解密
//! - 加密：明文 + nonce + key -> 密文 + tag
//! - 解密：密文 + tag + nonce + key -> 明文
//! - 认证加密：防篡改、防重放

use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose, Engine};

/// AES-GCM 加密器
#[derive(Clone)]
pub struct AesGcmCipher {
    cipher: Aes256Gcm,
}

impl AesGcmCipher {
    /// 从 hex 格式的 32 字节密钥创建加密器
    pub fn from_hex_key(hex_key: &str) -> Result<Self, crate::foundation::GateError> {
        let key_bytes = hex::decode(hex_key)
            .map_err(|e| crate::foundation::GateError::Config(format!("AES密钥hex解码失败: {}", e)))?;

        if key_bytes.len() != 32 {
            return Err(crate::foundation::GateError::Config(format!(
                "AES-256密钥必须32字节, 当前{}字节",
                key_bytes.len()
            )));
        }

        let key = aes_gcm::Key::<Aes256Gcm>::from_slice(&key_bytes);
        Ok(Self {
            cipher: Aes256Gcm::new(key),
        })
    }

    /// 从原始字节密钥创建加密器
    pub fn from_bytes(key_bytes: &[u8]) -> Result<Self, crate::foundation::GateError> {
        if key_bytes.len() != 32 {
            return Err(crate::foundation::GateError::Config(format!(
                "AES-256密钥必须32字节, 当前{}字节",
                key_bytes.len()
            )));
        }
        let key = aes_gcm::Key::<Aes256Gcm>::from_slice(key_bytes);
        Ok(Self {
            cipher: Aes256Gcm::new(key),
        })
    }

    /// 加密数据，返回 nonce + 密文（拼接）
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, crate::foundation::GateError> {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng); // 12 bytes
        let ciphertext = self
            .cipher
            .encrypt(&nonce, plaintext)
            .map_err(|_| crate::foundation::GateError::AesDecryptFailed)?;

        let mut result = Vec::with_capacity(12 + ciphertext.len());
        result.extend_from_slice(&nonce);
        result.extend_from_slice(&ciphertext);
        Ok(result)
    }

    /// 使用指定 nonce 加密（用于协议固定 nonce 场景）
    pub fn encrypt_with_nonce(
        &self,
        plaintext: &[u8],
        nonce_bytes: &[u8],
    ) -> Result<Vec<u8>, crate::foundation::GateError> {
        let nonce = Nonce::from_slice(nonce_bytes);
        self.cipher
            .encrypt(nonce, plaintext)
            .map_err(|_| crate::foundation::GateError::AesDecryptFailed)
    }

    /// 解密数据（输入为 nonce + 密文拼接）
    pub fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>, crate::foundation::GateError> {
        if data.len() < 12 {
            return Err(crate::foundation::GateError::AesDecryptFailed);
        }
        let nonce = Nonce::from_slice(&data[..12]);
        let ciphertext = &data[12..];
        self.cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| crate::foundation::GateError::AesDecryptFailed)
    }

    /// 使用指定 nonce 解密
    pub fn decrypt_with_nonce(
        &self,
        ciphertext: &[u8],
        nonce_bytes: &[u8],
    ) -> Result<Vec<u8>, crate::foundation::GateError> {
        let nonce = Nonce::from_slice(nonce_bytes);
        self.cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| crate::foundation::GateError::AesDecryptFailed)
    }
}

/// Base64 编码辅助
pub fn b64_encode(data: &[u8]) -> String {
    general_purpose::STANDARD.encode(data)
}

/// Base64 解码辅助
pub fn b64_decode(s: &str) -> Result<Vec<u8>, crate::foundation::GateError> {
    general_purpose::STANDARD
        .decode(s)
        .map_err(|e| crate::foundation::GateError::Internal(format!("Base64解码失败: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

    fn create_cipher() -> AesGcmCipher {
        AesGcmCipher::from_hex_key(TEST_KEY).unwrap()
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let cipher = create_cipher();
        let plaintext = b"Hello, MMO Gate!";
        let encrypted = cipher.encrypt(plaintext).unwrap();
        let decrypted = cipher.decrypt(&encrypted).unwrap();
        assert_eq!(plaintext.as_slice(), decrypted.as_slice());
    }

    #[test]
    fn test_encrypt_decrypt_large_data() {
        let cipher = create_cipher();
        let plaintext = vec![0xABu8; 8192];
        let encrypted = cipher.encrypt(&plaintext).unwrap();
        let decrypted = cipher.decrypt(&encrypted).unwrap();
        assert_eq!(plaintext, decrypted);
    }

    #[test]
    fn test_encrypt_produces_different_ciphertext() {
        let cipher = create_cipher();
        let plaintext = b"same data";
        let c1 = cipher.encrypt(plaintext).unwrap();
        let c2 = cipher.encrypt(plaintext).unwrap();
        // nonce 随机，密文不同
        assert_ne!(c1, c2);
    }

    #[test]
    fn test_decrypt_tampered_fails() {
        let cipher = create_cipher();
        let plaintext = b"tamper test";
        let mut encrypted = cipher.encrypt(plaintext).unwrap();
        encrypted[15] ^= 0xFF; // 篡改密文
        assert!(cipher.decrypt(&encrypted).is_err());
    }

    #[test]
    fn test_decrypt_short_data_fails() {
        let cipher = create_cipher();
        let short = vec![0u8; 5];
        assert!(cipher.decrypt(&short).is_err());
    }

    #[test]
    fn test_invalid_key_length() {
        assert!(AesGcmCipher::from_hex_key("0011").is_err());
        assert!(AesGcmCipher::from_bytes(&[0u8; 16]).is_err());
    }

    #[test]
    fn test_b64_roundtrip() {
        let data = vec![0u8, 1, 2, 3, 255, 254];
        let encoded = b64_encode(&data);
        let decoded = b64_decode(&encoded).unwrap();
        assert_eq!(data, decoded);
    }

    #[test]
    fn test_fixed_nonce_roundtrip() {
        let cipher = create_cipher();
        let nonce = [0u8; 12];
        let plaintext = b"fixed nonce test";
        let encrypted = cipher.encrypt_with_nonce(plaintext, &nonce).unwrap();
        let decrypted = cipher.decrypt_with_nonce(&encrypted, &nonce).unwrap();
        assert_eq!(plaintext.as_slice(), decrypted.as_slice());
    }
}
