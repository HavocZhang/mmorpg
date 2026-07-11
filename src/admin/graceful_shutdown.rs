//! 优雅停机模块
//!
//! 收到停机信号后：
//! 1. 不再接受新连接
//! 2. 存量连接逐个优雅下线
//! 3. 通知逻辑服玩家离线
//! 4. 注销集群节点
//! 5. 释放所有资源

use tracing::info;

/// 等待停机信号
pub async fn wait_for_shutdown() {
    // 监听 Ctrl+C (SIGINT) 和 SIGTERM
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};

        let mut sigterm = signal(SignalKind::terminate()).expect("无法注册SIGTERM");
        let mut sigint = signal(SignalKind::interrupt()).expect("无法注册SIGINT");

        tokio::select! {
            _ = sigterm.recv() => {
                info!("收到SIGTERM信号");
            }
            _ = sigint.recv() => {
                info!("收到SIGINT信号");
            }
        }
    }

    #[cfg(not(unix))]
    {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {
                info!("收到Ctrl+C信号");
            }
            Err(e) => {
                tracing::error!("信号监听错误: {}", e);
            }
        }
    }
}

/// 执行优雅停机流程
///
/// 注意：完整的优雅停机逻辑在 main.rs 中实现（信号监听 → 停止接受连接
/// → 等待存量 → 清理会话 → 注销集群 → 停止HTTP）。本函数为占位接口，
/// 保留供未来模块化重构使用。
pub async fn graceful_shutdown() {
    info!("开始优雅停机...");

    // 1. 停止接受新连接（main.rs: tcp_handle.abort()）
    // 2. 通知逻辑服所有玩家即将离线（main.rs: 遍历会话 kick）
    // 3. 注销集群节点（main.rs: cluster_mgr.shutdown()）
    // 4. 等待存量消息处理完成（main.rs: 5s grace period）

    info!("优雅停机完成");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_graceful_shutdown() {
        // 不应 panic
        graceful_shutdown().await;
    }
}
