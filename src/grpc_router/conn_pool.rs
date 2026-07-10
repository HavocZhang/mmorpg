//! gRPC 连接池模块
//!
//! 管理到逻辑服的 gRPC 连接，支持：
//! - 连接复用
//! - 负载均衡（轮询/一致性哈希）
//! - 断连重连
//! - 健康检查

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use tracing::{info, warn};

/// gRPC 连接池
pub struct GrpcConnPool {
    /// 逻辑服端点列表
    endpoints: Vec<String>,
    /// 轮询计数器
    round_robin: AtomicUsize,
    /// 连接状态（端点 -> 是否健康）
    health: DashMap<String, bool>,
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
        }
    }

    /// 轮询选取下一个健康端点
    pub fn next_endpoint(&self) -> Option<String> {
        let healthy: Vec<&String> = self
            .endpoints
            .iter()
            .filter(|ep| {
                self.health
                    .get(*ep)
                    .map(|h| *h)
                    .unwrap_or(false)
            })
            .collect();

        if healthy.is_empty() {
            warn!("无可用gRPC端点");
            return None;
        }

        let idx = self.round_robin.fetch_add(1, Ordering::Relaxed) % healthy.len();
        Some(healthy[idx].clone())
    }

    /// 按一致性哈希选取端点（用于消息路由到特定分片）
    pub fn route_by_key(&self, key: &str) -> Option<String> {
        let healthy: Vec<&String> = self
            .endpoints
            .iter()
            .filter(|ep| self.health.get(*ep).map(|h| *h).unwrap_or(false))
            .collect();

        if healthy.is_empty() {
            return None;
        }

        // 简化版一致性哈希：取 hash % len
        let hash = simple_hash(key);
        let idx = (hash as usize) % healthy.len();
        Some(healthy[idx].clone())
    }

    /// 标记端点为不健康
    pub fn mark_unhealthy(&self, endpoint: &str) {
        if let Some(mut h) = self.health.get_mut(endpoint) {
            *h = false;
            warn!("gRPC端点标记不健康: {}", endpoint);
        }
    }

    /// 标记端点为健康
    pub fn mark_healthy(&self, endpoint: &str) {
        if let Some(mut h) = self.health.get_mut(endpoint) {
            *h = true;
            info!("gRPC端点恢复健康: {}", endpoint);
        }
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
