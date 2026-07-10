//! Redis 集群服务模块
//!
//! 阶段7核心：节点注册、心跳、服务发现、玩家gate路由、跨网关PubSub消息总线
//!
//! Redis 降级策略：当 Redis 不可用时，网关仍可正常运行（仅本地能力），
//! 所有 Redis 操作失败时记录 warn 日志但不阻断主流程。

pub mod cross_gate_pubsub;
pub mod node_heartbeat;
pub mod node_register;
pub mod route_index;

use std::sync::Arc;

use tracing::{info, warn};

use crate::config::AppConfig;
use crate::foundation::GateError;

/// Redis 连接管理器（多路复用连接，可 Clone 共享）
pub type RedisConn = redis::aio::MultiplexedConnection;

/// 集群管理器
pub struct ClusterManager {
    pub node_id: u64,
    pub node_name: String,
    pub redis_url: String,
    /// Redis 连接（None 表示 Redis 不可用，降级运行）
    pub redis_conn: Option<RedisConn>,
}

impl ClusterManager {
    pub async fn new(config: &AppConfig) -> Result<Self, GateError> {
        let redis_url = config.redis.url.clone();

        // 尝试连接 Redis，失败时降级运行
        let redis_conn = match Self::connect_redis(&redis_url).await {
            Ok(conn) => {
                info!("✅ Redis连接成功: {}", redis_url);
                Some(conn)
            }
            Err(e) => {
                warn!("⚠️ Redis连接失败（降级运行）: {} 错误: {}", redis_url, e);
                None
            }
        };

        Ok(Self {
            node_id: config.gate.node_id,
            node_name: config.gate.node_name.clone(),
            redis_url,
            redis_conn,
        })
    }

    /// 尝试连接 Redis
    async fn connect_redis(url: &str) -> Result<RedisConn, Box<dyn std::error::Error + Send + Sync>> {
        let client = redis::Client::open(url)?;
        let conn = client.get_multiplexed_async_connection().await?;
        Ok(conn)
    }

    /// Redis 是否可用
    pub fn is_redis_available(&self) -> bool {
        self.redis_conn.is_some()
    }

    /// 获取 Redis 连接（如果可用）
    pub fn conn(&self) -> Option<RedisConn> {
        self.redis_conn.clone()
    }

    /// 启动集群服务
    pub async fn start(self: Arc<Self>) {
        // 启动节点注册
        let register = node_register::NodeRegister::new(
            self.node_id,
            self.node_name.clone(),
            self.redis_url.clone(),
        );

        if let Some(ref conn) = self.redis_conn {
            if let Err(e) = register.register(conn.clone()).await {
                warn!("节点注册失败: {}", e);
            }
        } else {
            info!("Redis不可用，跳过节点注册");
        }

        // 启动心跳上报
        let heartbeat = node_heartbeat::NodeHeartbeat::new(
            self.node_id,
            self.node_name.clone(),
            self.redis_url.clone(),
        );

        if let Some(conn) = self.redis_conn.clone() {
            tokio::spawn(heartbeat.run(conn));
        } else {
            info!("Redis不可用，跳过心跳上报");
        }
    }

    /// 注销节点（优雅停机时调用）
    pub async fn shutdown(&self) {
        if let Some(ref conn) = self.redis_conn {
            let register = node_register::NodeRegister::new(
                self.node_id,
                self.node_name.clone(),
                self.redis_url.clone(),
            );
            if let Err(e) = register.unregister(conn.clone()).await {
                warn!("节点注销失败: {}", e);
            }
        }
    }
}
