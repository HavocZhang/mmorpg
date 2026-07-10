//! 协议解码器
//!
//! 从网络字节流中解析完整数据包：
//! 1. 读取16字节包头
//! 2. 校验魔数、版本、包体长度
//! 3. 读取包体
//! 4. 校验 CRC32
//! 5. AES-GCM 解密
//!
//! 支持：粘包自动拆分、多包合并解析、超大包拦截、畸形包拦截

use bytes::{Buf, BytesMut};

use crate::crypto::aes_gcm::AesGcmCipher;
use crate::foundation::GateError;
use crate::protocol::packet_struct::{HEADER_SIZE, MAX_BODY_SIZE, Packet, PacketHeader};

/// 协议解码器
///
/// 维护内部缓冲区，处理粘包/半包
pub struct PacketDecoder {
    cipher: AesGcmCipher,
    buffer: BytesMut,
}

impl PacketDecoder {
    /// 创建解码器
    pub fn new(cipher: AesGcmCipher) -> Self {
        Self {
            cipher,
            buffer: BytesMut::with_capacity(8192 * 2),
        }
    }

    /// 追加接收到的网络数据
    pub fn feed(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
    }

    /// 尝试从缓冲区解码一个完整包
    ///
    /// 返回：
    /// - Ok(Some((packet, decrypted_body)))：成功解码一个包
    /// - Ok(None)：数据不足，需要更多数据
    /// - Err(...)：协议错误（CRC失败、解密失败、畸形包等）
    pub fn decode(&mut self) -> Result<Option<(Packet, Vec<u8>)>, GateError> {
        // 包头不足
        if self.buffer.len() < HEADER_SIZE {
            return Ok(None);
        }

        // 解析包头
        let header = PacketHeader::from_bytes(&self.buffer[..HEADER_SIZE])?;

        // 检查包体长度
        let body_len = header.body_len as usize;
        if body_len > MAX_BODY_SIZE {
            return Err(GateError::PacketTooLarge {
                size: body_len,
                max: MAX_BODY_SIZE,
            });
        }

        // 检查是否已收到完整包体
        let total_len = HEADER_SIZE + body_len;
        if self.buffer.len() < total_len {
            return Ok(None);
        }

        // 提取包体
        let body = self.buffer[HEADER_SIZE..total_len].to_vec();

        // 从缓冲区移除已消费的数据
        self.buffer.advance(total_len);

        // 构建Packet
        let packet = Packet {
            header: header.clone(),
            body: body.clone(),
        };

        // 校验CRC32
        if !packet.verify_crc() {
            return Err(GateError::CrcMismatch);
        }

        // AES解密
        let decrypted = self.cipher.decrypt(&body)?;

        Ok(Some((packet, decrypted)))
    }

    /// 尝试连续解码多个包（处理粘包）
    pub fn decode_all(&mut self) -> Result<Vec<(Packet, Vec<u8>)>, GateError> {
        let mut packets = vec![];
        loop {
            match self.decode()? {
                Some(pkt) => packets.push(pkt),
                None => break,
            }
        }
        Ok(packets)
    }

    /// 获取缓冲区中剩余未消费的字节数
    pub fn remaining(&self) -> usize {
        self.buffer.len()
    }

    /// 清空缓冲区
    pub fn clear(&mut self) {
        self.buffer.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::encoder::PacketEncoder;

    const TEST_KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

    fn create_encoder_decoder() -> (PacketEncoder, PacketDecoder) {
        let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
        let encoder = PacketEncoder::new(cipher);
        let cipher2 = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
        let decoder = PacketDecoder::new(cipher2);
        (encoder, decoder)
    }

    #[test]
    fn test_decode_single_packet() {
        let (encoder, mut decoder) = create_encoder_decoder();
        let payload = b"hello world";
        let bytes = encoder.encode_to_bytes(0x0001, payload).unwrap();

        decoder.feed(&bytes);
        let result = decoder.decode().unwrap();
        assert!(result.is_some());
        let (_, decrypted) = result.unwrap();
        assert_eq!(decrypted, payload);
    }

    #[test]
    fn test_decode_partial_packet() {
        let (encoder, mut decoder) = create_encoder_decoder();
        let bytes = encoder.encode_to_bytes(0x0001, b"partial test data").unwrap();

        // 只喂前半部分
        decoder.feed(&bytes[..10]);
        assert!(decoder.decode().unwrap().is_none());

        // 喂剩余部分
        decoder.feed(&bytes[10..]);
        assert!(decoder.decode().unwrap().is_some());
    }

    #[test]
    fn test_decode_multiple_packets() {
        let (encoder, mut decoder) = create_encoder_decoder();
        let mut combined = Vec::new();
        for i in 0..5 {
            let payload = format!("packet-{}", i);
            let bytes = encoder.encode_to_bytes(i as u16, payload.as_bytes()).unwrap();
            combined.extend_from_slice(&bytes);
        }
        decoder.feed(&combined);
        let packets = decoder.decode_all().unwrap();
        assert_eq!(packets.len(), 5);
        for (i, (_, decrypted)) in packets.iter().enumerate() {
            assert_eq!(decrypted, format!("packet-{}", i).as_bytes());
        }
    }

    #[test]
    fn test_decode_oversized_packet() {
        let mut decoder = {
            let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();
            PacketDecoder::new(cipher)
        };

        // 构造一个超大包头
        let mut header_bytes = [0u8; HEADER_SIZE];
        header_bytes[0] = 0x4D; // magic
        header_bytes[1] = 0x4D;
        header_bytes[2] = 0x01; // version
        header_bytes[6] = 0x20; // body_len = 8193 (big endian: 0x2001)
        header_bytes[7] = 0x01;

        decoder.feed(&header_bytes);
        let result = decoder.decode();
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_crc_mismatch() {
        let (encoder, mut decoder) = create_encoder_decoder();
        let mut bytes = encoder.encode_to_bytes(0x0001, b"crc test").unwrap();
        // 篡改包体
        bytes[HEADER_SIZE + 2] ^= 0xFF;
        decoder.feed(&bytes);
        assert!(decoder.decode().is_err());
    }

    #[test]
    fn test_decode_empty_buffer() {
        let (_, mut decoder) = create_encoder_decoder();
        assert!(decoder.decode().unwrap().is_none());
    }

    #[test]
    fn test_decode_remaining() {
        let (encoder, mut decoder) = create_encoder_decoder();
        let bytes = encoder.encode_to_bytes(0x0001, b"test").unwrap();
        decoder.feed(&bytes);
        let _ = decoder.decode().unwrap();
        assert_eq!(decoder.remaining(), 0);
    }
}
