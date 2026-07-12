//! AES-256-GCM 加密 + 16B 定长包头
//!
//! 协议格式:
//! - 16 字节包头: magic(2) + version(1) + reserved(1) + msg_id(2) + body_len(2) + crc32(4) + flags(4)
//! - 加密包体: nonce(12) + ciphertext + tag(16)
//!
//! 与网关 src/protocol/packet_struct.rs 和 web-client/game.html 的加密逻辑保持一致。

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Key, Nonce,
};
use crc32fast::Hasher;

/// 协议魔数 "MM"
pub const MAGIC: [u8; 2] = [0x4D, 0x4D];

/// 协议版本
pub const VERSION: u8 = 1;

/// 包头大小
pub const HEADER_LEN: usize = 16;

/// AES-256-GCM 密钥 (hex, 32 字节)
/// 与网关和 web-client 共用同一密钥
const KEY_HEX: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

/// AES-256-GCM 加密器 + 包头打包
pub struct Crypto {
    cipher: Aes256Gcm,
}

impl Crypto {
    /// 使用固定密钥创建加密器
    pub fn new() -> Self {
        let key_bytes = hex::decode(KEY_HEX).expect("AES密钥hex解码失败");
        assert_eq!(key_bytes.len(), 32, "AES-256密钥必须32字节");
        let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
        Self {
            cipher: Aes256Gcm::new(key),
        }
    }

    /// 加密 payload 并构建完整数据包
    ///
    /// 返回: header(16) + nonce(12) + ciphertext + tag(16)
    pub fn pack(&self, msg_id: u16, payload: &[u8]) -> Vec<u8> {
        // 1. 生成随机 12 字节 nonce
        let nonce_bytes: [u8; 12] = rand::random();
        let nonce = Nonce::from_slice(&nonce_bytes);

        // 2. AES-GCM 加密 (返回 ciphertext + tag 拼接)
        let ciphertext_with_tag = self
            .cipher
            .encrypt(nonce, payload)
            .expect("AES加密失败");

        // 3. 构建加密包体: nonce(12) + ciphertext + tag(16)
        //    注意: aes-gcm crate 的 encrypt 输出 = ciphertext + tag
        //          所以 nonce + ciphertext_with_tag = nonce + ct + tag (与 web-client 格式一致)
        let mut encrypted_payload = Vec::with_capacity(12 + ciphertext_with_tag.len());
        encrypted_payload.extend_from_slice(&nonce_bytes);
        encrypted_payload.extend_from_slice(&ciphertext_with_tag);

        // 4. 计算 CRC32 (对加密后的包体)
        let crc = crc32(&encrypted_payload);

        // 5. 构建包头 (16 字节)
        let mut packet = Vec::with_capacity(HEADER_LEN + encrypted_payload.len());
        packet.extend_from_slice(&MAGIC); // [0..2] magic
        packet.push(VERSION); // [2] version
        packet.push(0); // [3] reserved
        packet.extend_from_slice(&msg_id.to_be_bytes()); // [4..6] msg_id
        packet.extend_from_slice(&(encrypted_payload.len() as u16).to_be_bytes()); // [6..8] body_len
        packet.extend_from_slice(&crc.to_be_bytes()); // [8..12] crc32
        packet.extend_from_slice(&0u32.to_be_bytes()); // [12..16] flags (保留)

        // 6. 拼接包头 + 加密包体
        packet.extend_from_slice(&encrypted_payload);
        packet
    }

    /// 解包 + 解密
    ///
    /// 输入: 完整数据包 (header + encrypted_payload)
    /// 返回: (msg_id, plaintext)
    pub fn unpack(&self, data: &[u8]) -> Option<(u16, Vec<u8>)> {
        if data.len() < HEADER_LEN {
            return None;
        }

        // 校验魔数
        if data[0] != MAGIC[0] || data[1] != MAGIC[1] {
            return None;
        }

        let msg_id = u16::from_be_bytes([data[4], data[5]]);
        let payload_len = u16::from_be_bytes([data[6], data[7]]) as usize;

        if data.len() < HEADER_LEN + payload_len {
            return None;
        }

        let encrypted_payload = &data[HEADER_LEN..HEADER_LEN + payload_len];

        // 最小长度: 12 (nonce) + 16 (tag) = 28
        if encrypted_payload.len() < 28 {
            return None;
        }

        // 拆分: nonce(12) + ciphertext_with_tag (ct + tag)
        let nonce = Nonce::from_slice(&encrypted_payload[..12]);
        let ciphertext_with_tag = &encrypted_payload[12..];

        // AES-GCM 解密
        let plaintext = self.cipher.decrypt(nonce, ciphertext_with_tag).ok()?;
        Some((msg_id, plaintext))
    }
}

/// 计算 CRC32 校验值
fn crc32(data: &[u8]) -> u32 {
    let mut h = Hasher::new();
    h.update(data);
    h.finalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pack_unpack_roundtrip() {
        let crypto = Crypto::new();
        let msg_id: u16 = 1;
        let payload = b"{\"uid\":12345,\"token\":\"tok_abcdefgh\",\"version\":1,\"timestamp\":0}";

        let packet = crypto.pack(msg_id, payload);
        let (decoded_msg_id, decoded_payload) = crypto.unpack(&packet).unwrap();

        assert_eq!(decoded_msg_id, msg_id);
        assert_eq!(decoded_payload, payload);
    }

    #[test]
    fn test_pack_header_format() {
        let crypto = Crypto::new();
        let packet = crypto.pack(5001, b"test data");

        // 检查包头
        assert_eq!(&packet[0..2], &MAGIC);
        assert_eq!(packet[2], VERSION);
        assert_eq!(packet[3], 0); // reserved
        assert_eq!(u16::from_be_bytes([packet[4], packet[5]]), 5001);
    }

    #[test]
    fn test_unpack_invalid_magic() {
        let crypto = Crypto::new();
        let mut packet = crypto.pack(1, b"test");
        packet[0] = 0x00; // 破坏魔数
        assert!(crypto.unpack(&packet).is_none());
    }

    #[test]
    fn test_unpack_short_data() {
        let crypto = Crypto::new();
        assert!(crypto.unpack(&[0u8; 10]).is_none());
    }

    #[test]
    fn test_different_nonce_each_pack() {
        let crypto = Crypto::new();
        let p1 = crypto.pack(1, b"same payload");
        let p2 = crypto.pack(1, b"same payload");
        // nonce 随机，密文不同
        assert_ne!(&p1[HEADER_LEN..HEADER_LEN + 12], &p2[HEADER_LEN..HEADER_LEN + 12]);
    }
}
