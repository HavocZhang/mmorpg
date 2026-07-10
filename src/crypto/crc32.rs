//! CRC32 校验模块
//!
//! 用于游戏私有协议的数据完整性校验
//! - 编码时计算 body 的 CRC32 放入包头
//! - 解码时校验 CRC32 防篡改、防损坏

use crc32fast::Hasher;

/// 计算 data 的 CRC32 校验值
pub fn checksum(data: &[u8]) -> u32 {
    let mut hasher = Hasher::new();
    hasher.update(data);
    hasher.finalize()
}

/// 验证 data 的 CRC32 是否与期望值匹配
pub fn verify(data: &[u8], expected: u32) -> bool {
    checksum(data) == expected
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checksum_known_value() {
        // CRC32 of "123456789" is 0xCBF43926
        assert_eq!(checksum(b"123456789"), 0xCBF43926);
    }

    #[test]
    fn test_checksum_empty() {
        assert_eq!(checksum(b""), 0);
    }

    #[test]
    fn test_verify_valid() {
        let data = b"hello world";
        let crc = checksum(data);
        assert!(verify(data, crc));
    }

    #[test]
    fn test_verify_tampered() {
        let data = b"hello world";
        let crc = checksum(data);
        let tampered = b"hello warld";
        assert!(!verify(tampered, crc));
    }

    #[test]
    fn test_checksum_consistency() {
        let data = vec![0xFFu8; 1024];
        let c1 = checksum(&data);
        let c2 = checksum(&data);
        assert_eq!(c1, c2);
    }

    #[test]
    fn test_checksum_different_data_different_crc() {
        let d1 = b"data1";
        let d2 = b"data2";
        assert_ne!(checksum(d1), checksum(d2));
    }
}
