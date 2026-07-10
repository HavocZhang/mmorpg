//! 心跳巡检模块
//!
//! 定时巡检所有会话，清理僵尸连接
//! - 45秒无心跳、无交互自动判定僵尸连接并清理
//! - 巡检间隔可配置（默认10秒）

use std::sync::Arc;
use std::time::Duration;

use tokio::time::interval;
use tracing::info;

use crate::session::session_mgr::SessionManager;

/// 启动心跳巡检协程
pub async fn run_heartbeat_check(
    session_mgr: Arc<SessionManager>,
    timeout_secs: u64,
    check_interval_secs: u64,
) {
    info!(
        "心跳巡检启动: 超时={}s 间隔={}s",
        timeout_secs, check_interval_secs
    );

    let mut tick = interval(Duration::from_secs(check_interval_secs));

    loop {
        tick.tick().await;
        let before = session_mgr.online_count();
        session_mgr.clean_idle_sessions(timeout_secs).await;
        let after = session_mgr.online_count();

        if before != after {
            info!(
                "心跳巡检完成: 清理前={} 清理后={} 清理数={}",
                before,
                after,
                before - after
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_heartbeat_check_starts() {
        let mgr = Arc::new(SessionManager::new());
        // 启动巡检，立即取消
        let handle = tokio::spawn(run_heartbeat_check(mgr, 45, 10));
        // 短暂等待后取消
        tokio::time::sleep(Duration::from_millis(100)).await;
        handle.abort();
    }
}
