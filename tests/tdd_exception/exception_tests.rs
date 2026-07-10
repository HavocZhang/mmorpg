//! TDD 异常容错测试
//!
//! 测试 IO 错误、断网、畸形包、边界条件

use rust_mmo_gate::crypto::aes_gcm::AesGcmCipher;
use rust_mmo_gate::foundation::GateError;
use rust_mmo_gate::protocol::decoder::PacketDecoder;
use rust_mmo_gate::protocol::encoder::PacketEncoder;
use rust_mmo_gate::protocol::packet_struct::{HEADER_SIZE, MAX_BODY_SIZE};

const TEST_KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

#[test]
fn test_decode_empty_buffer() {
    let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
    let mut decoder = PacketDecoder::new(cipher);
    assert!(decoder.decode().unwrap().is_none());
}

#[test]
fn test_decode_partial_header() {
    let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
    let mut decoder = PacketDecoder::new(cipher);
    // 只喂8字节（不足16字节包头）
    decoder.feed(&[0u8; 8]);
    assert!(decoder.decode().unwrap().is_none());
}

#[test]
fn test_decode_invalid_magic() {
    let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
    let mut decoder = PacketDecoder::new(cipher);
    let mut bad_header = [0u8; HEADER_SIZE];
    bad_header[0] = 0x00; // wrong magic
    decoder.feed(&bad_header);
    assert!(decoder.decode().is_err());
}

#[test]
fn test_decode_oversized_packet() {
    let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
    let mut decoder = PacketDecoder::new(cipher);
    let mut header = [0u8; HEADER_SIZE];
    header[0] = 0x4D;
    header[1] = 0x4D;
    header[2] = 0x01; // version
    // body_len = MAX_BODY_SIZE + 1 = 8193 = 0x2001
    header[6] = 0x20;
    header[7] = 0x01;
    decoder.feed(&header);
    let result = decoder.decode();
    assert!(matches!(result, Err(GateError::PacketTooLarge { .. })));
}

#[test]
fn test_decode_crc_mismatch() {
    let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
    let encoder = PacketEncoder::new(cipher);
    let cipher2 = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
    let mut decoder = PacketDecoder::new(cipher2);

    let mut bytes = encoder.encode_to_bytes(0x0001, b"crc test").unwrap();
    // 篡改包体
    bytes[HEADER_SIZE + 2] ^= 0xFF;
    decoder.feed(&bytes);
    assert!(decoder.decode().is_err());
}

#[test]
fn test_aes_decrypt_tampered() {
    let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
    let encrypted = cipher.encrypt(b"test data").unwrap();
    let mut tampered = encrypted.clone();
    tampered[15] ^= 0xFF;
    assert!(cipher.decrypt(&tampered).is_err());
}

#[test]
fn test_aes_decrypt_short_data() {
    let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
    assert!(cipher.decrypt(&[0u8; 5]).is_err());
    assert!(cipher.decrypt(&[]).is_err());
}

#[test]
fn test_invalid_aes_key() {
    assert!(AesGcmCipher::from_hex_key("invalid").is_err());
    assert!(AesGcmCipher::from_hex_key("0011").is_err());
    assert!(AesGcmCipher::from_bytes(&[0u8; 16]).is_err());
}

#[test]
fn test_error_classification() {
    assert!(GateError::InvalidToken.is_security());
    assert!(GateError::CrcMismatch.is_security());
    assert!(GateError::AesDecryptFailed.is_security());
    assert!(GateError::MalformedPacket.is_security());
    assert!(GateError::RateLimited("test".into()).is_security());

    assert!(!GateError::MalformedPacket.is_recoverable());
    assert!(!GateError::CrcMismatch.is_recoverable());
    assert!(GateError::SessionNotFound(1).is_recoverable());
}
