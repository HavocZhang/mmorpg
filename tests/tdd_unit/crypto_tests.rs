//! TDD 单元测试 — 加密校验模块
//!
//! 测试 AES-256-GCM 加解密、CRC32 校验、边界条件、错误处理

use rust_mmo_gate::crypto::aes_gcm::AesGcmCipher;
use rust_mmo_gate::crypto::crc32;

const TEST_KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

#[test]
fn test_aes_encrypt_decrypt_roundtrip() {
    let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
    let plaintext = b"hello world test payload";
    let encrypted = cipher.encrypt(plaintext).unwrap();
    assert_ne!(encrypted, plaintext, "加密后应与原文不同");
    let decrypted = cipher.decrypt(&encrypted).unwrap();
    assert_eq!(decrypted, plaintext);
}

#[test]
fn test_aes_different_plaintexts_produce_different_ciphertexts() {
    let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
    let ct1 = cipher.encrypt(b"message_a").unwrap();
    let ct2 = cipher.encrypt(b"message_b").unwrap();
    assert_ne!(ct1, ct2);
}

#[test]
fn test_aes_tampered_ciphertext_fails() {
    let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
    let mut encrypted = cipher.encrypt(b"test").unwrap();
    if encrypted.len() > 13 {
        encrypted[13] ^= 0xFF;
    }
    assert!(cipher.decrypt(&encrypted).is_err(), "篡改后解密应失败");
}

#[test]
fn test_aes_invalid_key_length() {
    assert!(AesGcmCipher::from_hex_key("short").is_err());
}

#[test]
fn test_aes_empty_payload() {
    let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
    let encrypted = cipher.encrypt(b"").unwrap();
    let decrypted = cipher.decrypt(&encrypted).unwrap();
    assert!(decrypted.is_empty());
}

#[test]
fn test_aes_large_payload() {
    let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
    let data = vec![0xAB; 8192];
    let encrypted = cipher.encrypt(&data).unwrap();
    let decrypted = cipher.decrypt(&encrypted).unwrap();
    assert_eq!(decrypted, data);
}

#[test]
fn test_crc32_deterministic() {
    let data = b"consistent checksum test";
    let c1 = crc32::checksum(data);
    let c2 = crc32::checksum(data);
    assert_eq!(c1, c2, "相同输入应产生相同 CRC");
}

#[test]
fn test_crc32_different_data_produces_different_checksum() {
    let c1 = crc32::checksum(b"hello");
    let c2 = crc32::checksum(b"world");
    assert_ne!(c1, c2);
}

#[test]
fn test_crc32_empty_input() {
    let result = crc32::checksum(b"");
    assert_eq!(result, 0, "空数据 CRC 应为 0");
}

#[test]
fn test_crc32_known_vector() {
    // "123456789" 的 CRC32 已知值
    let result = crc32::checksum(b"123456789");
    assert_eq!(result, 0xcbf43926);
}
