//! 协议编码器
//!
//! 将逻辑消息编码为网络字节流：
//! 1. 序列化消息体
//! 2. AES-GCM 加密
//! 3. 计算 CRC32
//! 4. 组装 16字节包头 + 加密包体

use crate::crypto::aes_gcm::AesGcmCipher;
use crate::foundation::GateError;
use crate::protocol::packet_struct::{Packet, HEADER_SIZE, MsgId};

/// 协议编码器
pub struct PacketEncoder {
    cipher: AesGcmCipher,
}

impl PacketEncoder {
    /// 创建编码器
    pub fn new(cipher: AesGcmCipher) -> Self {
        Self { cipher }
    }

    /// 编码消息为完整数据包
    ///
    /// # 参数
    /// - `msg_id`：消息ID
    /// - `payload`：明文消息体
    pub fn encode(&self, msg_id: MsgId, payload: &[u8]) -> Result<Packet, GateError> {
        // 加密消息体
        let encrypted = self.cipher.encrypt(payload)?;

        // 构建完整包（含CRC32计算）
        Ok(Packet::new(msg_id, encrypted))
    }

    /// 编码并序列化为字节流
    pub fn encode_to_bytes(&self, msg_id: MsgId, payload: &[u8]) -> Result<Vec<u8>, GateError> {
        let packet = self.encode(msg_id, payload)?;
        Ok(packet.to_bytes())
    }

    /// 编码空包（用于心跳等）
    pub fn encode_empty(&self, msg_id: MsgId) -> Result<Packet, GateError> {
        self.encode(msg_id, &[])
    }

    /// 获取包头大小
    pub fn header_size(&self) -> usize {
        HEADER_SIZE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

    fn create_encoder() -> PacketEncoder {
        let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
        PacketEncoder::new(cipher)
    }

    #[test]
    fn test_encode_basic() {
        let encoder = create_encoder();
        let payload = b"hello gate";
        let packet = encoder.encode(0x0001, payload).unwrap();
        assert_eq!(packet.header.msg_id, 0x0001);
        assert!(packet.verify_crc());
        assert!(!packet.body.is_empty());
    }

    #[test]
    fn test_encode_empty_payload() {
        let encoder = create_encoder();
        let packet = encoder.encode_empty(0x0000).unwrap();
        assert_eq!(packet.header.body_len, packet.body.len() as u16);
        assert!(packet.verify_crc());
    }

    #[test]
    fn test_encode_to_bytes() {
        let encoder = create_encoder();
        let bytes = encoder.encode_to_bytes(0x1234, b"test").unwrap();
        assert!(bytes.len() > HEADER_SIZE);
        assert_eq!(&bytes[0..2], &crate::protocol::packet_struct::MAGIC);
    }

    #[test]
    fn test_encode_large_payload() {
        let encoder = create_encoder();
        let payload = vec![0xAB; 4096];
        let packet = encoder.encode(0x0001, &payload).unwrap();
        assert!(packet.verify_crc());
    }
}
