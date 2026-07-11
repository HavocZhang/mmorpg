//! gRPC 连接池模块
//!
//! 管理到逻辑服的 gRPC 连接，支持：
//! - 连接复用
//! - 负载均衡（轮询/一致性哈希）
//! - 断连重连
//! - 健康检查

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use dashmap::DashMap;
use tracing::{info, warn};

/// WARN 日志去重最小间隔（秒）
const WARN_DEBOUNCE_SECS: u64 = 10;
/// 所有端点不健康时的自动恢复探测间隔（秒）
const RECOVERY_PROBE_INTERVAL_SECS: u64 = 30;

/// gRPC 连接池
pub struct GrpcConnPool {
    /// 逻辑服端点列表
    endpoints: Vec<String>,
    /// 轮询计数器
    round_robin: AtomicUsize,
    /// 连接状态（端点 -> 是否健康）
    health: DashMap<String, bool>,
    /// 上次发出"无可用端点"警告的时间戳（去重用）
    last_no_healthy_warn: AtomicU64,
    /// 是否已经发出过"所有端点都不健康"的警告（避免重复）
    all_unhealthy_warned: AtomicBool,
    /// 所有端点变为不健康的时间戳（用于自动恢复探测）
    last_all_unhealthy: AtomicU64,
}

impl GrpcConnPool {
    pub fn new(endpoints: Vec<String>) -> Self {
        let health = DashMap::new();
        for ep in &endpoints {
            health.insert(ep.clone(), true);
        }
        info!("gRPC连接池初始化: {} 个端点", endpoints.len());
        Self {
            endpoints,
            round_robin: AtomicUsize::new(0),
            health,
            last_no_healthy_warn: AtomicU64::new(0),
            all_unhealthy_warned: AtomicBool::new(false),
            last_all_unhealthy: AtomicU64::new(0),
        }
    }

    /// 检查是否有可用端点（含自动恢复探测）
    ///
    /// 当所有端点都不健康时，会记录时间戳。超过 RECOVERY_PROBE_INTERVAL_SECS
    /// 后自动将所有端点重置为健康，以便下次消息路由时重新尝试连接。
    #[inline]
    pub fn has_healthy_endpoint(&self) -> bool {
        let has_healthy = self
            .endpoints
            .iter()
            .any(|ep| self.health.get(ep).map(|h| *h).unwrap_or(false));

        if has_healthy {
            return true;
        }

        // 所有端点都不健康：记录时间戳，检查是否需要恢复探测
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // 首次记录全不健康时间
        let last = self.last_all_unhealthy.load(Ordering::Relaxed);
        if last == 0 {
            self.last_all_unhealthy.store(now, Ordering::Relaxed);
            return false;
        }

        // 超过恢复间隔，重置所有端点为健康以触发重新探测
        if now.saturating_sub(last) >= RECOVERY_PROBE_INTERVAL_SECS {
            for ep in &self.endpoints {
                self.health.insert(ep.clone(), true);
            }
            self.last_all_unhealthy.store(0, Ordering::Relaxed);
            info!("gRPC端点自动恢复探测：所有端点已重置为健康");
            return true;
        }

        false
    }

    /// 内部方法：收集所有健康端点
    fn collect_healthy(&self) -> Vec<&String> {
        self.endpoints
            .iter()
            .filter(|ep| self.health.get(*ep).map(|h| *h).unwrap_or(false))
            .collect()
    }

    /// 去重 WARN 日志：仅在距离上次警告超过 WARN_DEBOUNCE_SECS 时才输出
    fn warn_debounced(&self, msg: &str) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let last = self.last_no_healthy_warn.load(Ordering::Relaxed);
        if now.saturating_sub(last) >= WARN_DEBOUNCE_SECS {
            self.last_no_healthy_warn.store(now, Ordering::Relaxed);
            warn!("{}", msg);
        }
    }

    /// 轮询选取下一个健康端点
    pub fn next_endpoint(&self) -> Option<String> {
        let healthy = self.collect_healthy();

        if healthy.is_empty() {
            // 去重日志：最多每 10 秒输出一次
            if !self.all_unhealthy_warned.swap(true, Ordering::Relaxed) {
                warn!("所有gRPC端点均已不可用，消息将降级为本地回显");
            } else {
                self.warn_debounced("无可用gRPC端点（降级运行中）");
            }
            return None;
        }

        // 端点恢复时重置标记
        self.all_unhealthy_warned.store(false, Ordering::Relaxed);

        let idx = self.round_robin.fetch_add(1, Ordering::Relaxed) % healthy.len();
        Some(healthy[idx].clone())
    }

    /// 按一致性哈希选取端点（用于消息路由到特定分片）
    pub fn route_by_key(&self, key: &str) -> Option<String> {
        let healthy = self.collect_healthy();

        if healthy.is_empty() {
            // 去重日志
            if !self.all_unhealthy_warned.swap(true, Ordering::Relaxed) {
                warn!("所有gRPC端点均已不可用，消息将降级为本地回显");
            } else {
                self.warn_debounced("无可用gRPC端点（降级运行中）");
            }
            return None;
        }

        // 端点恢复时重置标记
        self.all_unhealthy_warned.store(false, Ordering::Relaxed);

        // 简化版一致性哈希：取 hash % len
        let hash = simple_hash(key);
        let idx = (hash as usize) % healthy.len();
        Some(healthy[idx].clone())
    }

    /// 标记端点为不健康（仅在状态变更时输出日志）
    pub fn mark_unhealthy(&self, endpoint: &str) {
        if let Some(mut h) = self.health.get_mut(endpoint) {
            let was_healthy = *h;
            *h = false;
            // 仅在从健康变为不健康时输出日志
            if was_healthy {
                warn!("gRPC端点标记不健康: {}", endpoint);
                // 检查是否所有端点都不健康了
                if !self.has_healthy_ignoring_recovery() {
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    self.last_all_unhealthy.store(now, Ordering::Relaxed);
                }
            }
        }
    }

    /// 标记端点为健康（仅在状态变更时输出日志）
    pub fn mark_healthy(&self, endpoint: &str) {
        if let Some(mut h) = self.health.get_mut(endpoint) {
            let was_unhealthy = !*h;
            *h = true;
            // 仅在从不健康恢复时输出日志
            if was_unhealthy {
                info!("gRPC端点恢复健康: {}", endpoint);
                // 有端点恢复了，重置恢复计时器
                self.last_all_unhealthy.store(0, Ordering::Relaxed);
            }
        }
    }

    /// 内部方法：检查是否有健康端点（不触发恢复逻辑，避免递归）
    fn has_healthy_ignoring_recovery(&self) -> bool {
        self.endpoints
            .iter()
            .any(|ep| self.health.get(ep).map(|h| *h).unwrap_or(false))
    }

    /// 获取健康端点数
    pub fn healthy_count(&self) -> usize {
        self.health
            .iter()
            .filter(|r| *r.value())
            .count()
    }

    /// 获取总端点数
    pub fn total_count(&self) -> usize {
        self.endpoints.len()
    }
}

/// 简化版字符串哈希
fn simple_hash(s: &str) -> u64 {
    let mut hash: u64 = 5381;
    for b in s.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(b as u64);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_round_robin() {
        let pool = GrpcConnPool::new(vec![
            "grpc://1.1.1.1:50051".into(),
            "grpc://2.2.2.2:50051".into(),
            "grpc://3.3.3.3:50051".into(),
        ]);

        let ep1 = pool.next_endpoint().unwrap();
        let ep2 = pool.next_endpoint().unwrap();
        let ep3 = pool.next_endpoint().unwrap();
        let ep4 = pool.next_endpoint().unwrap();

        // 轮询应循环
        assert_eq!(ep1, ep4);
        assert_ne!(ep1, ep2);
        assert_ne!(ep2, ep3);
    }

    #[test]
    fn test_route_by_key() {
        let pool = GrpcConnPool::new(vec![
            "grpc://1.1.1.1:50051".into(),
            "grpc://2.2.2.2:50051".into(),
        ]);

        // 相同key应路由到相同端点
        let ep1 = pool.route_by_key("player_123").unwrap();
        let ep2 = pool.route_by_key("player_123").unwrap();
        assert_eq!(ep1, ep2);
    }

    #[test]
    fn test_mark_unhealthy() {
        let pool = GrpcConnPool::new(vec![
            "grpc://1.1.1.1:50051".into(),
            "grpc://2.2.2.2:50051".into(),
        ]);

        assert_eq!(pool.healthy_count(), 2);
        pool.mark_unhealthy("grpc://1.1.1.1:50051");
        assert_eq!(pool.healthy_count(), 1);

        // 不健康的端点不应被选中
        for _ in 0..10 {
            let ep = pool.next_endpoint().unwrap();
            assert_ne!(ep, "grpc://1.1.1.1:50051");
        }
    }

    #[test]
    fn test_no_healthy_endpoints() {
        let pool = GrpcConnPool::new(vec!["grpc://1.1.1.1:50051".into()]);
        pool.mark_unhealthy("grpc://1.1.1.1:50051");
        assert!(pool.next_endpoint().is_none());
    }

    #[test]
    fn test_empty_pool() {
        let pool = GrpcConnPool::new(vec![]);
        assert!(pool.next_endpoint().is_none());
        assert_eq!(pool.total_count(), 0);
    }
}
