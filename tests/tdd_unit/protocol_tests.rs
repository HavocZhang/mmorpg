//! TDD 单元测试 - 协议编解码
//!
//! 测试协议编码、解码、粘包处理、边界条件

use rust_mmo_gate::crypto::aes_gcm::AesGcmCipher;
use rust_mmo_gate::protocol::decoder::PacketDecoder;
use rust_mmo_gate::protocol::encoder::PacketEncoder;
use rust_mmo_gate::protocol::packet_struct::{HEADER_SIZE, MAX_BODY_SIZE, MAGIC};

const TEST_KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

fn create_encoder_decoder() -> (PacketEncoder, PacketDecoder) {
    let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
    let encoder = PacketEncoder::new(cipher);
    let cipher2 = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
    let decoder = PacketDecoder::new(cipher2);
    (encoder, decoder)
}

#[test]
fn test_encode_decode_roundtrip() {
    let (encoder, mut decoder) = create_encoder_decoder();
    let payload = b"hello mmo gate";
    let bytes = encoder.encode_to_bytes(0x0001, payload).unwrap();

    decoder.feed(&bytes);
    let result = decoder.decode().unwrap().unwrap();
    let (_, decrypted) = result;
    assert_eq!(decrypted, payload);
}

#[test]
fn test_header_magic() {
    let (encoder, mut decoder) = create_encoder_decoder();
    let bytes = encoder.encode_to_bytes(0x0001, b"test").unwrap();
    assert_eq!(&bytes[0..2], &MAGIC);
}

#[test]
fn test_max_body_size_constant() {
    assert_eq!(MAX_BODY_SIZE, 8192);
    assert_eq!(HEADER_SIZE, 16);
}

#[test]
fn test_multiple_packets_decode() {
    let (encoder, mut decoder) = create_encoder_decoder();
    let mut combined = Vec::new();
    for i in 0..10 {
        let payload = format!("msg-{}", i);
        let bytes = encoder.encode_to_bytes(i as u16, payload.as_bytes()).unwrap();
        combined.extend_from_slice(&bytes);
    }
    decoder.feed(&combined);
    let packets = decoder.decode_all().unwrap();
    assert_eq!(packets.len(), 10);
}

#[test]
fn test_crc32_verification() {
    use rust_mmo_gate::crypto::crc32;

    let data = b"test data for crc";
    let crc = crc32::checksum(data);
    assert!(crc32::verify(data, crc));
    assert!(!crc32::verify(b"tampered", crc));
}

#[test]
fn test_snowflake_id_generation() {
    use rust_mmo_gate::foundation::SnowflakeIdGen;

    let mut gen = SnowflakeIdGen::new(1).unwrap();
    let id1 = gen.next_id().unwrap();
    let id2 = gen.next_id().unwrap();
    assert_ne!(id1, id2);
}

#[test]
fn test_error_types() {
    use rust_mmo_gate::foundation::GateError;

    let e = GateError::InvalidToken;
    assert!(e.is_security());
    assert!(format!("{}", e).contains("Token"));

    let e2 = GateError::PacketTooLarge { size: 10000, max: 8192 };
    assert!(!e2.is_recoverable());
}
