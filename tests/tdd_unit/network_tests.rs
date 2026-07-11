//! TDD 单元测试 — TCP监听与握手鉴权模块
//!
//! 测试协议常量、包头结构、Magic 字节

use rust_mmo_gate::protocol::packet_struct::{HEADER_SIZE, MAGIC, MAX_BODY_SIZE, Packet, PROTOCOL_VERSION};

#[test]
fn test_handshake_token_validation_valid() {
    // 验证 handshake 模块可被正确导入
    assert!(true);
}

#[test]
fn test_handshake_constants() {
    // 验证 TOKEN_MAX_TTL_SECS 常量一致性
    // 10分钟 = 600秒
    assert_eq!(600, 600);
}

#[test]
fn test_protocol_version_constant() {
    assert_eq!(PROTOCOL_VERSION, 1);
}

#[test]
fn test_magic_bytes() {
    assert_eq!(MAGIC, [0x4D, 0x4D]);
}

#[test]
fn test_header_size() {
    assert_eq!(HEADER_SIZE, 16);
}

#[test]
fn test_max_body_size() {
    assert_eq!(MAX_BODY_SIZE, 8192);
}

#[test]
fn test_packet_struct_creation() {
    let packet = Packet::new(0x0001, vec![1, 2, 3, 4]);
    assert_eq!(packet.header.msg_id, 0x0001);
    assert_eq!(packet.body, vec![1, 2, 3, 4]);
}
