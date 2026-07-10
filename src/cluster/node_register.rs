//! 节点注册模块
//!
//! 网关启动时向 Redis 注册节点信息
//! Redis Key 设计：
//! - gate:node:{node_id} -> JSON(node_info)  TTL 30s
//! - gate:nodes -> SET of node_id

use crate::cluster::RedisConn;
use crate::foundation::GateError;
use redis::AsyncCommands;
use tracing::info;

/// 节点注册器
pub struct NodeRegister {
    node_id: u64,
    node_name: String,
    redis_url: String,
}

/// 节点信息（存储在 Redis 中）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NodeInfo {
    pub node_id: u64,
    pub node_name: String,
    pub addr: String,
    pub online_count: usize,
    pub started_at: u64,
}

impl NodeRegister {
    pub fn new(node_id: u64, node_name: String, redis_url: String) -> Self {
        Self {
            node_id,
            node_name,
            redis_url,
        }
    }

    /// 注册节点到 Redis
    pub async fn register(&self, conn: RedisConn) -> Result<(), GateError> {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let node_info = NodeInfo {
            node_id: self.node_id,
            node_name: self.node_name.clone(),
            addr: self.redis_url.clone(),
            online_count: 0,
            started_at: timestamp,
        };

        let node_key = format!("gate:node:{}", self.node_id);
        let node_json = serde_json::to_string(&node_info)
            .map_err(|e| GateError::Redis(format!("节点信息序列化失败: {}", e)))?;

        let mut conn = conn;
        let _: () = conn
            .set_ex(&node_key, &node_json, 30)
            .await
            .map_err(|e| GateError::Redis(format!("SET失败: {}", e)))?;

        let _: () = conn
            .sadd("gate:nodes", self.node_id)
            .await
            .map_err(|e| GateError::Redis(format!("SADD失败: {}", e)))?;

        info!(
            "注册网关节点成功: id={} name={} redis={}",
            self.node_id, self.node_name, self.redis_url
        );

        Ok(())
    }

    /// 注销节点
    pub async fn unregister(&self, conn: RedisConn) -> Result<(), GateError> {
        let node_key = format!("gate:node:{}", self.node_id);

        let mut conn = conn;
        let _: () = conn
            .del(&node_key)
            .await
            .map_err(|e| GateError::Redis(format!("DEL失败: {}", e)))?;

        let _: () = conn
            .srem("gate:nodes", self.node_id)
            .await
            .map_err(|e| GateError::Redis(format!("SREM失败: {}", e)))?;

        info!("注销网关节点: id={}", self.node_id);

        Ok(())
    }
}
