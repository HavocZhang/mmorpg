//! 玩家 Gate 路由索引模块
//!
//! 维护 player_uid -> gate_node_id 的全局映射
//! 用于跨网关消息精准投递
//!
//! Redis Key: gate:route:{uid} -> gate_node_id

use redis::AsyncCommands;
use tracing::info;

use crate::cluster::RedisConn;
use crate::foundation::GateError;

/// 路由索引管理
pub struct RouteIndex {
    #[allow(dead_code)]
    redis_url: String,
}

impl RouteIndex {
    pub fn new(redis_url: String) -> Self {
        Self { redis_url }
    }

    /// 更新玩家所在网关（玩家登录/重连时调用）
    pub async fn update_route(
        &self,
        mut conn: RedisConn,
        player_uid: u64,
        gate_node_id: u64,
    ) -> Result<(), GateError> {
        let key = format!("gate:route:{}", player_uid);

        let _: () = conn
            .set_ex(&key, gate_node_id, 3600) // TTL 1小时
            .await
            .map_err(|e| GateError::Redis(format!("路由SET失败: {}", e)))?;

        info!(
            "更新路由: uid={} -> gate_node={}",
            player_uid, gate_node_id
        );
        Ok(())
    }

    /// 查询玩家所在网关
    pub async fn get_gate_node(
        &self,
        mut conn: RedisConn,
        player_uid: u64,
    ) -> Result<Option<u64>, GateError> {
        let key = format!("gate:route:{}", player_uid);

        let result: Option<String> = conn
            .get(&key)
            .await
            .map_err(|e| GateError::Redis(format!("路由GET失败: {}", e)))?;

        match result {
            Some(val) => {
                let node_id: u64 = val
                    .parse()
                    .map_err(|e| GateError::Redis(format!("路由值解析失败: {} val={}", e, val)))?;
                Ok(Some(node_id))
            }
            None => Ok(None),
        }
    }

    /// 删除路由（玩家下线时调用）
    pub async fn remove_route(
        &self,
        mut conn: RedisConn,
        player_uid: u64,
    ) -> Result<(), GateError> {
        let key = format!("gate:route:{}", player_uid);

        let _: () = conn
            .del(&key)
            .await
            .map_err(|e| GateError::Redis(format!("路由DEL失败: {}", e)))?;

        info!("删除路由: uid={}", player_uid);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn test_route_index_creation() {
        let _idx = RouteIndex::new("redis://127.0.0.1:6379".into());
        // 验证 RouteIndex 可正常创建
        assert!(true);
    }
}
