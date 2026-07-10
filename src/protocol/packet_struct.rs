//! 协议包结构定义
//!
//! 16字节固定包头 + 变长加密体

/// 协议版本
pub const PROTOCOL_VERSION: u8 = 1;

/// 包头大小（字节）
pub const HEADER_SIZE: usize = 16;

/// 包体最大大小（8KB，超过直接断连防护OOM）
pub const MAX_BODY_SIZE: usize = 8192;

/// 协议魔数（"MMOG" = 0x4D4D4F47）
pub const MAGIC: [u8; 2] = [0x4D, 0x4D];

/// 消息ID类型
pub type MsgId = u16;

/// 16字节固定包头
#[derive(Clone, Debug)]
pub struct PacketHeader {
    /// 魔数 2字节
    pub magic: [u8; 2],
    /// 协议版本 1字节
    pub version: u8,
    /// 保留字段 1字节
    pub reserved: u8,
    /// 消息ID 2字节
    pub msg_id: MsgId,
    /// 包体长度 2字节
    pub body_len: u16,
    /// CRC32 校验值 4字节（对加密后body的校验）
    pub crc32: u32,
    /// 保留/标志位 4字节（用于优先级等扩展）
    pub flags: u32,
}

impl PacketHeader {
    /// 创建新包头
    pub fn new(msg_id: MsgId, body_len: usize) -> Self {
        Self {
            magic: MAGIC,
            version: PROTOCOL_VERSION,
            reserved: 0,
            msg_id,
            body_len: body_len as u16,
            crc32: 0,
            flags: 0,
        }
    }

    /// 序列化为 16 字节
    pub fn to_bytes(&self) -> [u8; HEADER_SIZE] {
        let mut buf = [0u8; HEADER_SIZE];
        buf[0] = self.magic[0];
        buf[1] = self.magic[1];
        buf[2] = self.version;
        buf[3] = self.reserved;
        buf[4..6].copy_from_slice(&self.msg_id.to_be_bytes());
        buf[6..8].copy_from_slice(&self.body_len.to_be_bytes());
        buf[8..12].copy_from_slice(&self.crc32.to_be_bytes());
        buf[12..16].copy_from_slice(&self.flags.to_be_bytes());
        buf
    }

    /// 从 16 字节反序列化
    pub fn from_bytes(data: &[u8]) -> Result<Self, crate::foundation::GateError> {
        if data.len() < HEADER_SIZE {
            return Err(crate::foundation::GateError::MalformedPacket);
        }

        let magic = [data[0], data[1]];
        if magic != MAGIC {
            return Err(crate::foundation::GateError::MalformedPacket);
        }

        let version = data[2];
        if version != PROTOCOL_VERSION {
            return Err(crate::foundation::GateError::VersionMismatch {
                client: version as u32,
                server: PROTOCOL_VERSION as u32,
            });
        }

        let msg_id = u16::from_be_bytes([data[4], data[5]]);
        let body_len = u16::from_be_bytes([data[6], data[7]]);
        let crc32 = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
        let flags = u32::from_be_bytes([data[12], data[13], data[14], data[15]]);

        Ok(Self {
            magic,
            version,
            reserved: data[3],
            msg_id,
            body_len,
            crc32,
            flags,
        })
    }
}

/// 完整数据包（包头 + 加密包体）
#[derive(Clone, Debug)]
pub struct Packet {
    pub header: PacketHeader,
    /// 加密后的包体
    pub body: Vec<u8>,
}

impl Packet {
    /// 创建新包
    pub fn new(msg_id: MsgId, encrypted_body: Vec<u8>) -> Self {
        let mut header = PacketHeader::new(msg_id, encrypted_body.len());
        header.crc32 = crate::crypto::crc32::checksum(&encrypted_body);
        Self {
            header,
            body: encrypted_body,
        }
    }

    /// 验证CRC32
    pub fn verify_crc(&self) -> bool {
        crate::crypto::crc32::verify(&self.body, self.header.crc32)
    }

    /// 整包序列化（包头 + 包体）
    pub fn to_bytes(&self) -> Vec<u8> {
        let header_bytes = self.header.to_bytes();
        let mut buf = Vec::with_capacity(HEADER_SIZE + self.body.len());
        buf.extend_from_slice(&header_bytes);
        buf.extend_from_slice(&self.body);
        buf
    }

    /// 包体是否超过最大限制
    pub fn is_oversized(body_len: usize) -> bool {
        body_len > MAX_BODY_SIZE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_roundtrip() {
        let header = PacketHeader::new(0x1234, 256);
        let bytes = header.to_bytes();
        let restored = PacketHeader::from_bytes(&bytes).unwrap();
        assert_eq!(restored.msg_id, 0x1234);
        assert_eq!(restored.body_len, 256);
        assert_eq!(restored.version, PROTOCOL_VERSION);
    }

    #[test]
    fn test_header_invalid_magic() {
        let mut bytes = [0u8; HEADER_SIZE];
        bytes[0] = 0x00; // wrong magic
        assert!(PacketHeader::from_bytes(&bytes).is_err());
    }

    #[test]
    fn test_header_short_data() {
        let short = [0u8; 8];
        assert!(PacketHeader::from_bytes(&short).is_err());
    }

    #[test]
    fn test_header_version_mismatch() {
        let mut header = PacketHeader::new(1, 10);
        header.version = 99;
        let bytes = header.to_bytes();
        assert!(PacketHeader::from_bytes(&bytes).is_err());
    }

    #[test]
    fn test_packet_crc_verify() {
        let body = vec![0xAB; 100];
        let pkt = Packet::new(0x0001, body);
        assert!(pkt.verify_crc());
    }

    #[test]
    fn test_packet_crc_tampered() {
        let body = vec![0xAB; 100];
        let mut pkt = Packet::new(0x0001, body);
        pkt.body[10] ^= 0xFF;
        assert!(!pkt.verify_crc());
    }

    #[test]
    fn test_packet_to_bytes() {
        let body = vec![1, 2, 3, 4, 5];
        let pkt = Packet::new(0x00FF, body.clone());
        let bytes = pkt.to_bytes();
        assert_eq!(bytes.len(), HEADER_SIZE + 5);
        // 验证包头
        assert_eq!(&bytes[0..2], &MAGIC);
    }

    #[test]
    fn test_oversized_check() {
        assert!(!Packet::is_oversized(8192));
        assert!(Packet::is_oversized(8193));
        assert!(Packet::is_oversized(65535));
    }
}
