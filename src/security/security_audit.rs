//! 安全审计模块
//!
//! 记录安全事件：非法Token、CRC失败、AES解密失败、限流触发、攻击包等

use std::net::IpAddr;
use std::time::Instant;

use crate::foundation::GateError;

/// 安全事件类型
#[derive(Debug, Clone)]
pub struct SecurityEvent {
    pub timestamp: Instant,
    pub ip: IpAddr,
    pub event_type: SecurityEventType,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecurityEventType {
    InvalidToken,
    ExpiredToken,
    CrcMismatch,
    AesDecryptFailed,
    MalformedPacket,
    RateLimited,
    PacketTooLarge,
    VersionMismatch,
    Other,
}

impl From<&GateError> for SecurityEventType {
    fn from(e: &GateError) -> Self {
        match e {
            GateError::InvalidToken => SecurityEventType::InvalidToken,
            GateError::ExpiredToken => SecurityEventType::ExpiredToken,
            GateError::CrcMismatch => SecurityEventType::CrcMismatch,
            GateError::AesDecryptFailed => SecurityEventType::AesDecryptFailed,
            GateError::MalformedPacket => SecurityEventType::MalformedPacket,
            GateError::RateLimited(_) => SecurityEventType::RateLimited,
            GateError::PacketTooLarge { .. } => SecurityEventType::PacketTooLarge,
            GateError::VersionMismatch { .. } => SecurityEventType::VersionMismatch,
            _ => SecurityEventType::Other,
        }
    }
}

/// 安全审计
pub struct SecurityAudit;

impl SecurityAudit {
    /// 记录安全事件
    pub fn record(ip: &IpAddr, error: &GateError) {
        let event_type = SecurityEventType::from(error);
        tracing::warn!(
            "安全事件: ip={} type={} detail={}",
            ip,
            format!("{:?}", event_type),
            error
        );

        // 更新指标
        crate::foundation::metric::metrics()
            .decode_errors
            .with_label_values(&[&format!("{:?}", event_type)])
            .inc();
    }

    /// 记录自定义安全事件
    pub fn record_raw(ip: &IpAddr, event_type: SecurityEventType, detail: &str) {
        tracing::warn!(
            "安全事件: ip={} type={} detail={}",
            ip,
            format!("{:?}", event_type),
            detail
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_type_mapping() {
        assert_eq!(
            SecurityEventType::from(&GateError::InvalidToken),
            SecurityEventType::InvalidToken
        );
        assert_eq!(
            SecurityEventType::from(&GateError::CrcMismatch),
            SecurityEventType::CrcMismatch
        );
        assert_eq!(
            SecurityEventType::from(&GateError::MalformedPacket),
            SecurityEventType::MalformedPacket
        );
        assert_eq!(
            SecurityEventType::from(&GateError::Internal("test".into())),
            SecurityEventType::Other
        );
    }
}
