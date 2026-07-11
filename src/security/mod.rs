//! 安全限流 + 风控模块
//!
//! 阶段8核心：单玩家限流、全局限流、IP封禁、攻击包拦截

pub mod ip_blacklist;
pub mod rate_limit;
pub mod security_audit;

use std::net::IpAddr;

use crate::config::AppConfig;
use crate::foundation::GateError;

/// 安全管理器
pub struct SecurityManager {
    pub rate_limiter: rate_limit::RateLimiter,
    pub ip_blacklist: ip_blacklist::IpBlacklist,
}

impl SecurityManager {
    pub fn new(config: &AppConfig) -> Self {
        Self {
            rate_limiter: rate_limit::RateLimiter::new(
                config.rate_limit.player_per_sec,
                config.rate_limit.player_battle_per_sec,
                config.rate_limit.global_per_sec,
            ),
            ip_blacklist: ip_blacklist::IpBlacklist::new(),
        }
    }

    /// IP 是否被封禁
    pub fn is_ip_blocked(&self, ip: &IpAddr) -> bool {
        self.ip_blacklist.is_blocked(ip)
    }

    /// 检查连接频率
    pub fn check_connect_rate(&self, ip: &IpAddr) -> bool {
        self.rate_limiter.check_connect_rate(ip)
    }

    /// 检查玩家消息速率
    pub fn check_player_rate(&self, player_uid: u64, is_battle: bool) -> bool {
        self.rate_limiter.check_player_rate(player_uid, is_battle)
    }

    /// 检查全局速率
    pub fn check_global_rate(&self) -> bool {
        self.rate_limiter.check_global_rate()
    }

    /// 记录安全事件
    pub fn record_security_event(&self, ip: &IpAddr, error: &GateError) {
        security_audit::SecurityAudit::record(ip, error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;

    #[test]
    fn test_security_manager_creation() {
        let config = AppConfig::load().unwrap();
        let mgr = SecurityManager::new(&config);
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        assert!(!mgr.is_ip_blocked(&ip));
    }

    #[test]
    fn test_security_manager_connect_rate_check() {
        let config = AppConfig::load().unwrap();
        let mgr = SecurityManager::new(&config);
        let ip: IpAddr = "192.168.1.1".parse().unwrap();
        assert!(mgr.check_connect_rate(&ip));
    }

    #[test]
    fn test_security_manager_player_rate_check() {
        let config = AppConfig::load().unwrap();
        let mgr = SecurityManager::new(&config);
        assert!(mgr.check_player_rate(12345, false));
    }
}
