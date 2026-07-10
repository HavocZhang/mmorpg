//! IP 黑名单模块
//!
//! 使用 DashMap 无锁容器管理黑名单
//! 支持手动添加、自动封禁、TTL过期

use std::net::IpAddr;
use std::time::{Duration, Instant};

use dashmap::DashMap;

/// IP 黑名单
pub struct IpBlacklist {
    /// 被封禁的IP列表：ip -> 过期时间
    blocked: DashMap<IpAddr, Option<Instant>>,
}

impl IpBlacklist {
    pub fn new() -> Self {
        Self {
            blocked: DashMap::new(),
        }
    }

    /// 添加永久封禁
    pub fn block(&self, ip: IpAddr) {
        self.blocked.insert(ip, None);
        tracing::warn!("IP永久封禁: {}", ip);
    }

    /// 添加临时封禁
    pub fn block_temp(&self, ip: IpAddr, duration: Duration) {
        let expire = Some(Instant::now() + duration);
        self.blocked.insert(ip, expire);
        tracing::warn!("IP临时封禁: {} 时长={:?}", ip, duration);
    }

    /// 解封
    pub fn unblock(&self, ip: &IpAddr) -> bool {
        self.blocked.remove(ip).is_some()
    }

    /// 检查是否被封禁
    pub fn is_blocked(&self, ip: &IpAddr) -> bool {
        if let Some(entry) = self.blocked.get(ip) {
            if let Some(expire) = *entry {
                if Instant::now() > expire {
                    // 已过期
                    return false;
                }
            }
            return true;
        }
        false
    }

    /// 清理过期封禁
    pub fn cleanup_expired(&self) {
        let now = Instant::now();
        let to_remove: Vec<IpAddr> = self
            .blocked
            .iter()
            .filter_map(|entry| {
                if let Some(expire) = entry.value() {
                    if now > *expire {
                        return Some(*entry.key());
                    }
                }
                None
            })
            .collect();

        for ip in to_remove {
            self.blocked.remove(&ip);
        }
    }

    /// 获取封禁列表大小
    pub fn len(&self) -> usize {
        self.blocked.len()
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.blocked.is_empty()
    }

    /// 获取所有被封禁的IP列表（字符串形式）
    pub fn list_all(&self) -> Vec<String> {
        self.blocked
            .iter()
            .map(|entry| entry.key().to_string())
            .collect()
    }

    /// 添加永久封禁（block 的别名，便于 API 调用）
    pub fn add(&self, ip: &IpAddr) {
        self.block(*ip);
    }

    /// 移除封禁（unblock 的别名，便于 API 调用）
    pub fn remove(&self, ip: &IpAddr) {
        self.unblock(ip);
    }
}

impl Default for IpBlacklist {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_block_unblock() {
        let bl = IpBlacklist::new();
        let ip = IpAddr::from_str("192.168.1.1").unwrap();

        assert!(!bl.is_blocked(&ip));
        bl.block(ip);
        assert!(bl.is_blocked(&ip));
        bl.unblock(&ip);
        assert!(!bl.is_blocked(&ip));
    }

    #[test]
    fn test_temp_block_not_expired() {
        let bl = IpBlacklist::new();
        let ip = IpAddr::from_str("10.0.0.1").unwrap();

        bl.block_temp(ip, Duration::from_secs(60));
        assert!(bl.is_blocked(&ip));
    }

    #[test]
    fn test_temp_block_expired() {
        let bl = IpBlacklist::new();
        let ip = IpAddr::from_str("10.0.0.2").unwrap();

        bl.block_temp(ip, Duration::from_millis(1));
        std::thread::sleep(Duration::from_millis(10));
        assert!(!bl.is_blocked(&ip));
    }

    #[test]
    fn test_cleanup_expired() {
        let bl = IpBlacklist::new();
        let ip1 = IpAddr::from_str("10.0.0.3").unwrap();
        let ip2 = IpAddr::from_str("10.0.0.4").unwrap();

        bl.block_temp(ip1, Duration::from_millis(1));
        bl.block(ip2); // 永久
        std::thread::sleep(Duration::from_millis(10));

        bl.cleanup_expired();
        assert!(!bl.is_blocked(&ip1));
        assert!(bl.is_blocked(&ip2));
    }

    #[test]
    fn test_ipv6() {
        let bl = IpBlacklist::new();
        let ip = IpAddr::from_str("::1").unwrap();
        bl.block(ip);
        assert!(bl.is_blocked(&ip));
    }

    #[test]
    fn test_concurrent_access() {
        use std::sync::Arc;
        use std::thread;

        let bl = Arc::new(IpBlacklist::new());
        let mut handles = vec![];

        for i in 0..4 {
            let b = bl.clone();
            handles.push(thread::spawn(move || {
                let ip = IpAddr::from_str(&format!("10.0.{}.1", i)).unwrap();
                b.block(ip);
                assert!(b.is_blocked(&ip));
            }));
        }

        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(bl.len(), 4);
    }
}
