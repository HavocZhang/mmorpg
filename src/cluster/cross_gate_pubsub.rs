//! 跨网关 PubSub 消息总线模块
//!
//! 通过 Redis PubSub 实现跨网关消息精准投递
//! - 同网关玩家消息本地直接下发，零中间件
//! - 跨网关玩家消息通过 Redis PubSub 精准投递不丢不重
//! - 广播消息通过 gate:broadcast 通道投递到所有网关
//!
//! Redis Channels:
//! - gate:msg:{target_gate_node}  — 定向投递到指定网关
//! - gate:broadcast               — 广播到所有网关

use std::time::Duration;

use redis::AsyncCommands;
use tracing::{info, warn};

use crate::cluster::RedisConn;
use crate::foundation::GateError;

/// 跨网关消息总线
pub struct CrossGatePubSub {
    node_id: u64,
    redis_url: String,
}

/// 跨网关消息
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CrossGateMsg {
    pub from_node: u64,
    /// 目标玩家UID (0 = 广播给本网关所有在线玩家)
    pub to_uid: u64,
    pub msg_id: u16,
    pub payload: Vec<u8>,
    pub priority: u8,
}

impl CrossGatePubSub {
    pub fn new(node_id: u64, redis_url: String) -> Self {
        Self { node_id, redis_url }
    }

    /// 发布定向跨网关消息
    ///
    /// 当目标玩家不在当前网关时，通过 PubSub 投递到目标网关
    pub async fn publish(
        &self,
        mut conn: RedisConn,
        target_node_id: u64,
        msg: CrossGateMsg,
    ) -> Result<(), GateError> {
        let channel = format!("gate:msg:{}", target_node_id);
        let data = serde_json::to_vec(&msg)
            .map_err(|e| GateError::Redis(format!("跨网关消息序列化失败: {}", e)))?;

        let _: i64 = conn
            .publish(&channel, data)
            .await
            .map_err(|e| GateError::Redis(format!("PUBLISH失败: {}", e)))?;

        info!(
            "跨网关定向消息: to_uid={} msg_id={} from_node={} -> target_node={}",
            msg.to_uid, msg.msg_id, msg.from_node, target_node_id
        );
        Ok(())
    }

    /// 发布广播消息到所有网关
    ///
    /// 广播消息通过 gate:broadcast 通道投递到所有订阅的网关
    pub async fn publish_broadcast(
        &self,
        mut conn: RedisConn,
        msg: CrossGateMsg,
    ) -> Result<(), GateError> {
        let channel = "gate:broadcast";
        let data = serde_json::to_vec(&msg)
            .map_err(|e| GateError::Redis(format!("广播消息序列化失败: {}", e)))?;

        let _: i64 = conn
            .publish(channel, data)
            .await
            .map_err(|e| GateError::Redis(format!("PUBLISH广播失败: {}", e)))?;

        info!(
            "跨网关广播消息: msg_id={} from_node={}",
            msg.msg_id, msg.from_node
        );
        Ok(())
    }

    /// 订阅本网关消息通道并处理
    ///
    /// 使用独立的 Redis 连接进行 PubSub 订阅
    /// 同时订阅定向通道和广播通道
    pub async fn subscribe<F>(&self, on_msg: F) -> Result<(), GateError>
    where
        F: Fn(CrossGateMsg) + Send + 'static,
    {
        let channel_targeted = format!("gate:msg:{}", self.node_id);
        let channel_broadcast = "gate:broadcast".to_string();
        info!(
            "订阅跨网关消息: node_id={} channels=[{}, {}]",
            self.node_id, channel_targeted, channel_broadcast
        );

        let client = redis::Client::open(self.redis_url.as_str())
            .map_err(|e| GateError::Redis(format!("Redis Client创建失败: {}", e)))?;

        // 外层循环：断线重连
        loop {
            let mut pubsub = match client.get_async_pubsub().await {
                Ok(p) => p,
                Err(e) => {
                    warn!("PubSub连接失败: {} 3秒后重试...", e);
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    continue;
                }
            };

            // 订阅定向通道和广播通道
            if let Err(e) = pubsub.subscribe(&channel_targeted).await {
                warn!("SUBSCRIBE失败: {} 3秒后重试...", e);
                tokio::time::sleep(Duration::from_secs(3)).await;
                continue;
            }
            if let Err(e) = pubsub.subscribe(&channel_broadcast).await {
                warn!("SUBSCRIBE广播失败: {} 3秒后重试...", e);
                tokio::time::sleep(Duration::from_secs(3)).await;
                continue;
            }

            info!(
                "PubSub订阅成功: channels=[{}, {}]",
                channel_targeted, channel_broadcast
            );

            // 内层循环：处理消息
            loop {
                match pubsub.on_message().next().await {
                    Some(msg) => {
                        let data = msg.get_payload_bytes().to_vec();
                        match serde_json::from_slice::<CrossGateMsg>(&data) {
                            Ok(cross_msg) => {
                                // 忽略自己发出的广播消息（避免重复处理）
                                if cross_msg.from_node == self.node_id {
                                    continue;
                                }
                                on_msg(cross_msg);
                            }
                            Err(e) => {
                                warn!("跨网关消息反序列化失败: {}", e);
                            }
                        }
                    }
                    None => {
                        warn!("PubSub连接断开，3秒后重连...");
                        break; // 退出内层循环，外层循环将重连
                    }
                }
            }
        }
    }
}

use futures_util::StreamExt;
