//! 限流模块
//!
//! 三级限流：
//! - 单玩家野外限流：30包/秒
//! - 团战动态放宽：80包/秒
//! - 全局峰值限流：8万包/秒

use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

use parking_lot::RwLock;

/// 限流器
pub struct RateLimiter {
    /// 玩家普通限流（包/秒）
    player_normal_limit: u32,
    /// 玩家团战限流（包/秒）
    player_battle_limit: u32,
    /// 全局限流（包/秒）
    global_limit: u32,
    /// IP连接频率上限
    ip_connect_max: u32,
    /// 玩家计数器
    player_counters: RwLock<HashMap<u64, RateCounter>>,
    /// IP连接频率计数器
    ip_connect_counters: RwLock<HashMap<IpAddr, ConnectCounter>>,
    /// 全局计数器
    global_counter: RwLock<RateCounter>,
}

/// 滑动窗口计数器
#[derive(Clone)]
struct RateCounter {
    count: u32,
    window_start: Instant,
}

impl RateCounter {
    fn new() -> Self {
        Self {
            count: 0,
            window_start: Instant::now(),
        }
    }

    /// 检查并递增，返回是否允许
    fn check_and_inc(&mut self, limit: u32) -> bool {
        let now = Instant::now();
        if now.duration_since(self.window_start) >= Duration::from_secs(1) {
            self.count = 0;
            self.window_start = now;
        }

        if self.count >= limit {
            return false;
        }

        self.count += 1;
        true
    }

    /// 仅检查不递增
    fn check(&self, limit: u32) -> bool {
        let now = Instant::now();
        if now.duration_since(self.window_start) >= Duration::from_secs(1) {
            return true; // 窗口已重置
        }
        self.count < limit
    }
}

/// IP连接频率计数器
#[derive(Clone)]
struct ConnectCounter {
    count: u32,
    window_start: Instant,
}

impl ConnectCounter {
    fn new() -> Self {
        Self {
            count: 0,
            window_start: Instant::now(),
        }
    }

    fn check_and_inc(&mut self, max_per_window: u32, window: Duration) -> bool {
        let now = Instant::now();
        if now.duration_since(self.window_start) >= window {
            self.count = 0;
            self.window_start = now;
        }

        if self.count >= max_per_window {
            return false;
        }

        self.count += 1;
        true
    }
}

/// IP 连接频率限制（可配置，默认每5秒最多20个新连接）
const IP_CONNECT_WINDOW: Duration = Duration::from_secs(5);
const IP_CONNECT_MAX_DEFAULT: u32 = 20;

impl RateLimiter {
    pub fn new(player_normal: u32, player_battle: u32, global: u32) -> Self {
        let ip_connect_max = std::env::var("IP_CONNECT_MAX")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(IP_CONNECT_MAX_DEFAULT);
        Self {
            player_normal_limit: player_normal,
            player_battle_limit: player_battle,
            global_limit: global,
            ip_connect_max,
            player_counters: RwLock::new(HashMap::new()),
            ip_connect_counters: RwLock::new(HashMap::new()),
            global_counter: RwLock::new(RateCounter::new()),
        }
    }

    /// 检查玩家消息速率
    ///
    /// # 参数
    /// - `player_uid`：玩家UID
    /// - `is_battle`：是否团战模式（放宽限流）
    pub fn check_player_rate(&self, player_uid: u64, is_battle: bool) -> bool {
        let limit = if is_battle {
            self.player_battle_limit
        } else {
            self.player_normal_limit
        };

        let mut counters = self.player_counters.write();
        let counter = counters.entry(player_uid).or_insert_with(RateCounter::new);
        let allowed = counter.check_and_inc(limit);

        if !allowed {
            tracing::warn!(
                "玩家限流触发: uid={} limit={} battle={}",
                player_uid,
                limit,
                is_battle
            );
            crate::foundation::metric::metrics()
                .rate_limit_hits
                .with_label_values(&["player"])
                .inc();
        }

        allowed
    }

    /// 检查全局限速
    pub fn check_global_rate(&self) -> bool {
        let mut counter = self.global_counter.write();
        let allowed = counter.check_and_inc(self.global_limit);

        if !allowed {
            tracing::warn!("全局限流触发: limit={}", self.global_limit);
            crate::foundation::metric::metrics()
                .rate_limit_hits
                .with_label_values(&["global"])
                .inc();
        }

        allowed
    }

    /// 检查IP连接频率
    pub fn check_connect_rate(&self, ip: &IpAddr) -> bool {
        let mut counters = self.ip_connect_counters.write();
        let counter = counters.entry(*ip).or_insert_with(ConnectCounter::new);
        let allowed = counter.check_and_inc(self.ip_connect_max, IP_CONNECT_WINDOW);

        if !allowed {
            tracing::warn!("IP连接限流: {}", ip);
            crate::foundation::metric::metrics()
                .rate_limit_hits
                .with_label_values(&["connect"])
                .inc();
        }

        allowed
    }

    /// 清理过期计数器（定期调用减少内存占用）
    pub fn cleanup(&self) {
        let now = Instant::now();
        let cutoff = Duration::from_secs(60);

        self.player_counters
            .write()
            .retain(|_, c| now.duration_since(c.window_start) < cutoff);

        self.ip_connect_counters
            .write()
            .retain(|_, c| now.duration_since(c.window_start) < cutoff);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_player_rate_limit_normal() {
        let limiter = RateLimiter::new(5, 10, 1000);

        // 5个包应该允许
        for _ in 0..5 {
            assert!(limiter.check_player_rate(1, false));
        }
        // 第6个应该被限流
        assert!(!limiter.check_player_rate(1, false));
    }

    #[test]
    fn test_player_rate_limit_battle() {
        let limiter = RateLimiter::new(5, 10, 1000);

        // 团战模式放宽到10
        for _ in 0..10 {
            assert!(limiter.check_player_rate(1, true));
        }
        // 第11个应该被限流
        assert!(!limiter.check_player_rate(1, true));
    }

    #[test]
    fn test_different_players_independent() {
        let limiter = RateLimiter::new(5, 10, 1000);

        // 不同玩家限流独立
        for _ in 0..5 {
            assert!(limiter.check_player_rate(1, false));
        }
        // 玩家2不受玩家1影响
        assert!(limiter.check_player_rate(2, false));
    }

    #[test]
    fn test_global_rate_limit() {
        let limiter = RateLimiter::new(100, 100, 5);

        // 全局限流5
        for _ in 0..5 {
            assert!(limiter.check_global_rate());
        }
        assert!(!limiter.check_global_rate());
    }

    #[test]
    fn test_connect_rate_limit() {
        let limiter = RateLimiter::new(100, 100, 1000);
        let ip: IpAddr = "127.0.0.1".parse().unwrap();

        for _ in 0..limiter.ip_connect_max {
            assert!(limiter.check_connect_rate(&ip));
        }
        assert!(!limiter.check_connect_rate(&ip));
    }

    #[test]
    fn test_rate_limit_concurrent() {
        use std::sync::Arc;
        use std::thread;

        let limiter = Arc::new(RateLimiter::new(100, 100, 10000));
        let mut handles = vec![];

        for _ in 0..4 {
            let l = limiter.clone();
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    l.check_player_rate(1, false);
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }
        // 不应 panic，无数据竞争
    }
}
