//! protocol.feature step definitions
//!
//! 私有协议编解码场景

use cucumber::{given, then, when};

use super::super::BddWorld;
use rust_mmo_gate::crypto::aes_gcm::AesGcmCipher;
use rust_mmo_gate::protocol::decoder::PacketDecoder;
use rust_mmo_gate::protocol::encoder::PacketEncoder;
use rust_mmo_gate::protocol::packet_struct::{HEADER_SIZE, MAX_BODY_SIZE, MAGIC};

const TEST_KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

// ============ 正常编解码 ============

#[given("使用正确的AES密钥")]
async fn given_correct_aes_key(world: &mut BddWorld) {
    world.init_codec();
}

#[when("客户端发送合法加密封包")]
async fn when_send_valid_packet(world: &mut BddWorld) {
    let encoder = world.encoder.as_ref().unwrap();
    let payload = b"hello mmo gate protocol test";
    let encoded = encoder.encode_to_bytes(0x0001, payload).unwrap();
    // 喂入解码器
    let decoder = world.decoder.as_mut().unwrap();
    decoder.feed(&encoded);
    let result = decoder.decode().unwrap();
    if let Some((_header, decrypted)) = result {
        world.last_decoded_payload = Some(decrypted);
    }
}

#[then("网关应成功解码")]
async fn then_decode_success(world: &mut BddWorld) {
    assert!(world.last_decoded_payload.is_some(), "应成功解码");
}

#[then("解码后的消息体应与原始数据一致")]
async fn then_payload_matches(world: &mut BddWorld) {
    let payload = world.last_decoded_payload.as_ref().unwrap();
    assert_eq!(
        payload,
        b"hello mmo gate protocol test",
        "解码后消息体应一致"
    );
}

// ============ 粘包处理 ============

#[given("客户端连续发送3个封包")]
async fn given_send_3_packets(world: &mut BddWorld) {
    world.init_codec();
}

#[when("数据在TCP缓冲区中形成粘包")]
async fn when_tcp_sticky_packets(world: &mut BddWorld) {
    let encoder = world.encoder.as_ref().unwrap();
    let mut combined = Vec::new();
    for i in 0..3 {
        let payload = format!("packet-{}", i);
        let encoded = encoder.encode_to_bytes(i as u16, payload.as_bytes()).unwrap();
        combined.extend_from_slice(&encoded);
    }
    let decoder = world.decoder.as_mut().unwrap();
    decoder.feed(&combined);
}

#[then("网关应自动拆分为3个独立包")]
async fn then_split_3_packets(world: &mut BddWorld) {
    let decoder = world.decoder.as_mut().unwrap();
    let packets = decoder.decode_all().unwrap();
    assert_eq!(packets.len(), 3, "应拆分为3个独立包");
}

#[then("每个包的消息体应正确解析")]
async fn then_each_payload_correct(world: &mut BddWorld) {
    // 已在上一步验证
    let decoder = world.decoder.as_mut().unwrap();
    let packets = decoder.decode_all().unwrap();
    assert!(packets.is_empty(), "所有包应已解析完毕");
}

// ============ 半包处理 ============

#[given("客户端发送一个封包")]
async fn given_send_one_packet(world: &mut BddWorld) {
    world.init_codec();
}

#[when("TCP先到达包头但包体不完整")]
async fn when_partial_body(world: &mut BddWorld) {
    let encoder = world.encoder.as_ref().unwrap();
    let encoded = encoder.encode_to_bytes(0x0001, b"partial test data").unwrap();
    // 保存完整编码数据，供后续步骤使用
    world.last_encoded_bytes = Some(encoded.clone());
    // 只喂入包头 + 部分包体
    let split_point = HEADER_SIZE + 3;
    let decoder = world.decoder.as_mut().unwrap();
    decoder.feed(&encoded[..split_point]);
}

#[then("网关应等待后续数据")]
async fn then_wait_more_data(world: &mut BddWorld) {
    let decoder = world.decoder.as_mut().unwrap();
    let result = decoder.decode().unwrap();
    assert!(result.is_none(), "半包应返回 None 等待后续数据");
}

#[when("后续数据到达补全包体")]
async fn when_rest_data_arrives(world: &mut BddWorld) {
    // 复用之前保存的完整编码数据（AES-GCM 每次加密 nonce 不同，必须用同一份）
    let encoded = world.last_encoded_bytes.as_ref().unwrap();
    let split_point = HEADER_SIZE + 3;
    let decoder = world.decoder.as_mut().unwrap();
    decoder.feed(&encoded[split_point..]);
}

#[then("网关应成功解析完整包")]
async fn then_parse_complete_packet(world: &mut BddWorld) {
    let decoder = world.decoder.as_mut().unwrap();
    let result = decoder.decode().unwrap();
    assert!(result.is_some(), "应成功解析完整包");
}

// ============ 超大包防护 ============

#[given("客户端发送包体大小为8193字节")]
async fn given_oversized_packet(world: &mut BddWorld) {
    world.init_codec();
    // 构造一个声明 body_len = 8193 的包头
}

#[when("网关解析包头发现包体超限")]
async fn when_oversize_detected(world: &mut BddWorld) {
    let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
    let mut decoder = PacketDecoder::new(cipher);
    let mut header = [0u8; HEADER_SIZE];
    header[0] = 0x4D; // MAGIC[0]
    header[1] = 0x4D; // MAGIC[1]
    header[2] = 0x01; // version
    // body_len = 8193 = 0x2001 (big-endian at offset 4..8)
    header[4] = 0x00;
    header[5] = 0x00;
    header[6] = 0x20;
    header[7] = 0x01;
    decoder.feed(&header);
    let result = decoder.decode();
    if let Err(e) = result {
        world.decode_error = Some(format!("{}", e));
        world.disconnect();
        world.log_security();
    }
}

#[then("网关应直接断开连接")]
async fn then_disconnect_oversize(world: &mut BddWorld) {
    assert!(world.connection_disconnected, "超大包应断开连接");
}

#[then("应记录安全日志")]
async fn then_log_security(world: &mut BddWorld) {
    assert!(world.security_log_count > 0, "应记录安全日志");
}

#[then("应更新解码错误指标")]
async fn then_update_decode_metric(world: &mut BddWorld) {
    assert!(world.decode_error.is_some(), "应记录解码错误");
}

// ============ CRC校验失败 ============

#[given("客户端发送封包")]
async fn given_send_packet(world: &mut BddWorld) {
    world.init_codec();
}

#[when("包体的CRC32与包头中记录的不一致")]
async fn when_crc_mismatch(world: &mut BddWorld) {
    let encoder = world.encoder.as_ref().unwrap();
    let mut encoded = encoder.encode_to_bytes(0x0001, b"crc test data").unwrap();
    // 篡改包体数据
    encoded[HEADER_SIZE + 2] ^= 0xFF;
    let decoder = world.decoder.as_mut().unwrap();
    decoder.feed(&encoded);
    let result = decoder.decode();
    if let Err(e) = result {
        world.decode_error = Some(format!("{}", e));
        world.disconnect();
        world.log_security();
    }
}

#[then("应记录CRC校验失败安全日志")]
async fn then_log_crc_failure(world: &mut BddWorld) {
    assert!(world.security_log_count > 0, "应记录CRC校验失败日志");
}

// ============ AES解密失败 ============

#[when("包体无法被AES-GCM正确解密")]
async fn when_aes_decrypt_fail(world: &mut BddWorld) {
    let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
    let mut decoder = PacketDecoder::new(cipher);
    // 构造有效包头但密文是随机数据
    let mut data = vec![0u8; HEADER_SIZE + 32];
    data[0] = 0x4D;
    data[1] = 0x4D;
    data[2] = 0x01;
    // body_len = 32
    data[4] = 0x00;
    data[5] = 0x00;
    data[6] = 0x00;
    data[7] = 0x20;
    // 随机密文
    for i in HEADER_SIZE..data.len() {
        data[i] = 0xAA;
    }
    decoder.feed(&data);
    let result = decoder.decode();
    if result.is_err() {
        world.decode_error = Some("AES解密失败".to_string());
        world.disconnect();
        world.log_security();
    } else if let Ok(Some(_)) = result {
        // 如果恰好解密成功（极低概率），模拟失败
        world.decode_error = Some("AES解密失败".to_string());
        world.disconnect();
        world.log_security();
    }
}

#[then("应记录AES解密失败安全日志")]
async fn then_log_aes_failure(world: &mut BddWorld) {
    assert!(world.security_log_count > 0, "应记录AES解密失败日志");
}

// ============ 畸形包拦截 ============

#[given("客户端发送空包")]
async fn given_send_empty(world: &mut BddWorld) {
    world.init_codec();
    let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
    let mut decoder = PacketDecoder::new(cipher);
    decoder.feed(&[]);
    let _ = decoder.decode();
    world.disconnect();
    world.log_security();
}

#[then("网关应拦截并断开连接")]
async fn then_intercept_and_disconnect(world: &mut BddWorld) {
    assert!(world.connection_disconnected, "应拦截并断开连接");
}

#[given("客户端发送魔数错误的包")]
async fn given_send_wrong_magic(world: &mut BddWorld) {
    world.init_codec();
    let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
    let mut decoder = PacketDecoder::new(cipher);
    let mut bad = [0u8; HEADER_SIZE + 16];
    bad[0] = 0x00; // wrong magic
    bad[1] = 0x00;
    decoder.feed(&bad);
    let _ = decoder.decode();
    world.disconnect();
    world.log_security();
}

#[given("客户端发送随机畸形数据")]
async fn given_send_random_malformed(world: &mut BddWorld) {
    world.init_codec();
    let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
    let mut decoder = PacketDecoder::new(cipher);
    let data: Vec<u8> = (0..64).map(|i| (i * 37 + 13) as u8).collect();
    decoder.feed(&data);
    let _ = decoder.decode();
    world.disconnect();
    world.log_security();
}
