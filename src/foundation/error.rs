//! 网关全局错误体系
//!
//! 所有模块统一错误类型，禁止裸 panic 泄露到线上

use thiserror::Error;

/// 网关统一错误类型
#[derive(Debug, Error)]
pub enum GateError {
    #[error("配置错误: {0}")]
    Config(String),

    #[error("协议错误: {0}")]
    Protocol(String),

    #[error("协议解码错误: {0}")]
    Decode(String),

    #[error("协议编码错误: {0}")]
    Encode(String),

    #[error("CRC校验失败")]
    CrcMismatch,

    #[error("AES解密失败")]
    AesDecryptFailed,

    #[error("包过大: {size} > {max}")]
    PacketTooLarge { size: usize, max: usize },

    #[error("空包或畸形包")]
    MalformedPacket,

    #[error("会话不存在: {0}")]
    SessionNotFound(u64),

    #[error("会话已关闭: {0}")]
    SessionClosed(u64),

    #[error("Token无效")]
    InvalidToken,

    #[error("Token已过期")]
    ExpiredToken,

    #[error("客户端版本不匹配: client={client} server={server}")]
    VersionMismatch { client: u32, server: u32 },

    #[error("IP已被封禁: {0}")]
    IpBlocked(String),

    #[error("限流触发: {0}")]
    RateLimited(String),

    #[error("Redis错误: {0}")]
    Redis(String),

    #[error("gRPC错误: {0}")]
    Grpc(String),

    #[error("IO错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("内部错误: {0}")]
    Internal(String),
}

impl GateError {
    /// 是否为安全类错误（需要记录安全审计日志）
    pub fn is_security(&self) -> bool {
        matches!(
            self,
            GateError::InvalidToken
                | GateError::ExpiredToken
                | GateError::IpBlocked(_)
                | GateError::CrcMismatch
                | GateError::AesDecryptFailed
                | GateError::MalformedPacket
                | GateError::RateLimited(_)
        )
    }

    /// 是否为可恢复错误
    pub fn is_recoverable(&self) -> bool {
        !matches!(
            self,
            GateError::PacketTooLarge { .. }
                | GateError::MalformedPacket
                | GateError::CrcMismatch
                | GateError::AesDecryptFailed
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_classification() {
        assert!(GateError::InvalidToken.is_security());
        assert!(GateError::CrcMismatch.is_security());
        assert!(GateError::RateLimited("test".into()).is_security());
        assert!(!GateError::Config("test".into()).is_security());

        assert!(!GateError::MalformedPacket.is_recoverable());
        assert!(GateError::SessionNotFound(1).is_recoverable());
    }

    #[test]
    fn test_error_display() {
        let e = GateError::PacketTooLarge { size: 10000, max: 8192 };
        assert!(format!("{}", e).contains("10000"));
        assert!(format!("{}", e).contains("8192"));
    }
}
