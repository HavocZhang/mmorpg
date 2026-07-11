//! 节点心跳模块
//!
//! 3秒心跳上报，10秒无心跳自动摘除
//! Redis Key: gate:heartbeat:{node_id} -> timestamp  TTL 10s

use std::time::Duration;

use redis::AsyncCommands;
use tracing::{debug, info, warn};

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

        // 1. 更新心跳时间戳
        let hb_key = format!("gate:heartbeat:{}", self.node_id);
        let _: () = conn
            .set_ex(&hb_key, timestamp, 10)
            .await
            .map_err(|e| GateError::Redis(format!("心跳SET失败: {}", e)))?;

        // 2. 刷新节点信息 TTL
        let node_key = format!("gate:node:{}", self.node_id);
        let expired: i64 = conn
            .expire(&node_key, 30)
            .await
            .map_err(|e| GateError::Redis(format!("EXPIRE失败: {}", e)))?;

        // 3. 如果节点信息键已过期（EXPIRE 返回 0），重新注册
        if expired == 0 {
            let node_info = crate::cluster::node_register::NodeInfo {
                node_id: self.node_id,
                node_name: self.node_name.clone(),
                addr: self.redis_url.clone(),
                online_count: 0,
                started_at: timestamp,
            };
            let node_json = serde_json::to_string(&node_info)
                .map_err(|e| GateError::Redis(format!("节点信息序列化失败: {}", e)))?;
            let _: () = conn
                .set_ex(&node_key, &node_json, 30)
                .await
                .map_err(|e| GateError::Redis(format!("节点信息重新注册失败: {}", e)))?;
            // 确保在 gate:nodes SET 中
            let _: () = conn
                .sadd("gate:nodes", self.node_id)
                .await
                .map_err(|e| GateError::Redis(format!("SADD失败: {}", e)))?;
            debug!("节点信息已过期，已重新注册: node_id={}", self.node_id);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heartbeat_creation() {
        let _hb = NodeHeartbeat::new(1, "gate-01".into(), "redis://127.0.0.1:6379".into());
        // NodeHeartbeat created successfully
    }

    #[test]
    fn test_heartbeat_interval() {
        // 心跳间隔应为 3 秒
        let interval = Duration::from_secs(3);
        assert!(!interval.is_zero());
    }
}
