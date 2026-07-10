//! 节点心跳模块
//!
//! 3秒心跳上报，10秒无心跳自动摘除
//! Redis Key: gate:heartbeat:{node_id} -> timestamp  TTL 10s

use std::time::Duration;

use redis::AsyncCommands;
use tracing::{info, warn};

use crate::cluster::RedisConn;
use crate::foundation::GateError;

/// 节点心跳上报
pub struct NodeHeartbeat {
    node_id: u64,
    node_name: String,
    redis_url: String,
}

impl NodeHeartbeat {
    pub fn new(node_id: u64, node_name: String, redis_url: String) -> Self {
        Self {
            node_id,
            node_name,
            redis_url,
        }
    }

    /// 运行心跳上报循环
    pub async fn run(self, conn: RedisConn) {
        info!(
            "心跳上报启动: node_id={} 间隔=3s",
            self.node_id
        );

        let mut interval = tokio::time::interval(Duration::from_secs(3));

        loop {
            interval.tick().await;
            if let Err(e) = self.heartbeat(conn.clone()).await {
                warn!("心跳上报失败: {}", e);
            }
        }
    }

    /// 执行一次心跳
    async fn heartbeat(&self, mut conn: RedisConn) -> Result<(), GateError> {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let key = format!("gate:heartbeat:{}", self.node_id);

        let _: () = conn
            .set_ex(&key, timestamp, 10)
            .await
            .map_err(|e| GateError::Redis(format!("心跳SET失败: {}", e)))?;

        // 同时刷新节点信息 TTL
        let node_key = format!("gate:node:{}", self.node_id);
        let _: () = conn
            .expire(&node_key, 30)
            .await
            .map_err(|e| GateError::Redis(format!("EXPIRE失败: {}", e)))?;

        Ok(())
    }
}
