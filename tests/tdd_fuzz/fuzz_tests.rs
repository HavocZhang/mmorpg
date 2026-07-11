//! TDD 模糊测试 - 恶意包/边界数据
//!
//! 使用随机数据模拟恶意包，测试网关协议解码的健壮性

use rand::Rng;
use rust_mmo_gate::crypto::aes_gcm::AesGcmCipher;
use rust_mmo_gate::protocol::decoder::PacketDecoder;

const TEST_KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

#[test]
fn test_fuzz_random_data() {
    let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
    let mut rng = rand::thread_rng();

    // 喂入100组随机数据，不应 panic
    for _ in 0..100 {
        let mut decoder = PacketDecoder::new(cipher.clone());
        let len = rng.gen_range(0..256);
        let data: Vec<u8> = (0..len).map(|_| rng.gen()).collect();
        decoder.feed(&data);
        // 解码结果可能是 Ok(None)、Ok(Some) 或 Err，但不应 panic
        let _ = decoder.decode();
    }
}

#[test]
fn test_fuzz_empty_packets() {
    let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
    let mut decoder = PacketDecoder::new(cipher);

    // 空数据
    decoder.feed(&[]);
    assert!(decoder.decode().unwrap().is_none());

    // 单字节
    decoder.feed(&[0x00]);
    assert!(decoder.decode().unwrap().is_none());
}

#[test]
fn test_fuzz_partial_headers() {
    let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();

    for len in 1..16 {
        let mut decoder = PacketDecoder::new(cipher.clone());
        let data = vec![0xFF; len];
        decoder.feed(&data);
        // 不足16字节包头，应返回 None 而非 panic
        assert!(decoder.decode().unwrap().is_none());
    }
}

#[test]
fn test_fuzz_valid_magic_bad_rest() {
    let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
    let mut rng = rand::thread_rng();

    for _ in 0..50 {
        let mut decoder = PacketDecoder::new(cipher.clone());
        let mut data = vec![0u8; 32];
        data[0] = 0x4D; // valid magic
        data[1] = 0x4D;
        data[2] = 0x01; // valid version
        // 随机填充剩余部分
        for elem in &mut data[3..32] {
            *elem = rng.gen();
        }
        decoder.feed(&data);
        // 应该返回错误或 None，不应 panic
        let _ = decoder.decode();
    }
}

#[test]
fn test_fuzz_max_size_boundary() {
    let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();

    // 恰好 8192 字节包体（边界值）
    let mut header = [0u8; 16];
    header[0] = 0x4D;
    header[1] = 0x4D;
    header[2] = 0x01;
    header[6] = 0x20; // 8192 = 0x2000
    header[7] = 0x00;

    let mut decoder = PacketDecoder::new(cipher.clone());
    decoder.feed(&header);
    // 包头有效但包体不完整，应返回 None
    assert!(decoder.decode().unwrap().is_none());

    // 超过 8192 字节
    let mut header2 = [0u8; 16];
    header2[0] = 0x4D;
    header2[1] = 0x4D;
    header2[2] = 0x01;
    header2[6] = 0x20; // 8193 = 0x2001
    header2[7] = 0x01;

    let mut decoder2 = PacketDecoder::new(cipher.clone());
    decoder2.feed(&header2);
    assert!(decoder2.decode().is_err());
}
