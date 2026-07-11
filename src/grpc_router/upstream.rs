//! 上行路由模块
//!
//! 客户端上行包 -> 网关 -> gRPC -> 逻辑服
//!
//! 使用 tonic gRPC 客户端将消息转发到逻辑服，并接收逻辑服的下行响应。

use std::sync::Arc;

use tonic::transport::Channel;
use tracing::{debug, info, warn};

use crate::foundation::GateError;
use crate::grpc_router::conn_pool::GrpcConnPool;
use crate::grpc_router::proto::gate::{
    logic_service_client::LogicServiceClient, ForwardBatchRequest, ForwardRequest,
    ForwardResponse, DownstreamMessage,
};

/// gRPC 端点 URL 缓存（endpoint -> Channel）
/// 使用 DashMap 实现，支持并发访问
type ChannelCache = dashmap::DashMap<String, Channel>;

/// 全局 Channel 缓存（进程内共享）
static CHANNEL_CACHE: std::sync::OnceLock<ChannelCache> = std::sync::OnceLock::new();

fn get_channel_cache() -> &'static ChannelCache {
    CHANNEL_CACHE.get_or_init(dashmap::DashMap::new)
}

/// 将 gRPC 端点 URL 转换为 tonic 可用的 URL
/// "grpc://127.0.0.1:50051" -> "http://127.0.0.1:50051"
fn normalize_endpoint(endpoint: &str) -> String {
    if let Some(addr) = endpoint.strip_prefix("grpc://") {
        format!("http://{}", addr)
    } else if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
        endpoint.to_string()
    } else {
        format!("http://{}", endpoint)
    }
}

/// 获取或创建到指定端点的 gRPC Channel
async fn get_or_create_channel(endpoint: &str) -> Result<Channel, GateError> {
    let normalized = normalize_endpoint(endpoint);

    // 检查缓存
    if let Some(channel) = get_channel_cache().get(&normalized) {
        return Ok(channel.clone());
    }

    // 创建新连接
    let channel = Channel::from_shared(normalized.clone())
        .map_err(|e| GateError::Grpc(format!("gRPC Channel 创建失败: {}", e)))?
        .connect_timeout(std::time::Duration::from_secs(3))
        .timeout(std::time::Duration::from_secs(5))
        .connect()
        .await
        .map_err(|e| {
            // 标记端点不健康
            GateError::Grpc(format!("gRPC 连接失败 {}: {}", normalized, e))
        })?;

    // 缓存
    get_channel_cache().insert(normalized, channel.clone());

    Ok(channel)
}

/// 上行路由器
pub struct UpstreamRouter {
    conn_pool: Arc<GrpcConnPool>,
}

impl UpstreamRouter {
    pub fn new(conn_pool: Arc<GrpcConnPool>) -> Self {
        Self { conn_pool }
    }

    /// 路由上行消息到逻辑服
    ///
    /// # 参数
    /// - `player_uid`：玩家UID（用于一致性哈希路由）
    /// - `msg_id`：消息ID
    /// - `payload`：消息体
    ///
    /// # 返回
    /// 逻辑服的下行响应消息列表
    pub async fn route(
        &self,
        player_uid: u64,
        msg_id: u16,
        payload: Vec<u8>,
    ) -> Result<ForwardResponse, GateError> {
        let key = player_uid.to_string();
        let endpoint = self
            .conn_pool
            .route_by_key(&key)
            .ok_or_else(|| GateError::Grpc("无可用逻辑服端点".into()))?;

        debug!(
            "上行路由: uid={} msg_id={} -> {} ({}bytes)",
            player_uid,
            msg_id,
            endpoint,
            payload.len()
        );

        // 获取 gRPC Channel
        let channel = match get_or_create_channel(&endpoint).await {
            Ok(ch) => ch,
            Err(e) => {
                // 连接失败，标记端点不健康
                self.conn_pool.mark_unhealthy(&endpoint);
                return Err(e);
            }
        };

        // 创建 gRPC 客户端
        let mut client = LogicServiceClient::new(channel);

        // 构造请求
        let request = tonic::Request::new(ForwardRequest {
            player_uid,
            msg_id: msg_id as u32,
            payload: payload.clone(),
        });

        // 调用逻辑服
        match client.forward_message(request).await {
            Ok(response) => {
                let resp = response.into_inner();
                debug!(
                    "上行路由成功: uid={} msg_id={} -> {} 条下行消息",
                    player_uid,
                    msg_id,
                    resp.messages.len()
                );
                // 恢复端点健康状态
                self.conn_pool.mark_healthy(&endpoint);
                Ok(resp)
            }
            Err(e) => {
                warn!(
                    "gRPC 调用失败: uid={} msg_id={} endpoint={} err={}",
                    player_uid, msg_id, endpoint, e
                );
                self.conn_pool.mark_unhealthy(&endpoint);
                Err(GateError::Grpc(format!("gRPC 调用失败: {}", e)))
            }
        }
    }

    /// 批量路由（团战场景高频小包聚合后批量发送）
    pub async fn route_batch(
        &self,
        messages: Vec<(u64, u16, Vec<u8>)>,
    ) -> Result<ForwardResponse, GateError> {
        if messages.is_empty() {
            return Ok(ForwardResponse { messages: vec![] });
        }

        info!("批量上行路由: {} 条消息", messages.len());

        // 使用第一个消息的 uid 来路由（同一批次应该路由到同一分片）
        let key = messages[0].0.to_string();
        let endpoint = self
            .conn_pool
            .route_by_key(&key)
            .ok_or_else(|| GateError::Grpc("无可用逻辑服端点".into()))?;

        let channel = get_or_create_channel(&endpoint).await?;
        let mut client = LogicServiceClient::new(channel);

        let batch_messages: Vec<ForwardRequest> = messages
            .into_iter()
            .map(|(uid, mid, payload)| ForwardRequest {
                player_uid: uid,
                msg_id: mid as u32,
                payload,
            })
            .collect();

        let request = tonic::Request::new(ForwardBatchRequest {
            messages: batch_messages,
        });

        match client.forward_message_batch(request).await {
            Ok(response) => {
                let resp = response.into_inner();
                debug!("批量上行路由成功: {} 条下行消息", resp.messages.len());
                Ok(resp)
            }
            Err(e) => {
                warn!("批量 gRPC 调用失败: endpoint={} err={}", endpoint, e);
                self.conn_pool.mark_unhealthy(&endpoint);
                Err(GateError::Grpc(format!("批量 gRPC 调用失败: {}", e)))
            }
        }
    }

    /// 通知逻辑服玩家上线
    ///
    /// 返回逻辑服需要下发的消息列表
    pub async fn notify_player_online(
        &self,
        player_uid: u64,
        session_id: u64,
        gate_node: &str,
    ) -> Result<Vec<DownstreamMessage>, GateError> {
        let endpoint = self
            .conn_pool
            .next_endpoint()
            .ok_or_else(|| GateError::Grpc("无可用逻辑服端点".into()))?;

        let channel = get_or_create_channel(&endpoint).await?;
        let mut client = LogicServiceClient::new(channel);

        let request = tonic::Request::new(
            crate::grpc_router::proto::gate::PlayerOnlineRequest {
                player_uid,
                session_id,
                gate_node: gate_node.to_string(),
            },
        );

        let response = client
            .player_online(request)
            .await
            .map_err(|e| GateError::Grpc(format!("玩家上线通知失败: {}", e)))?;

        Ok(response.into_inner().messages)
    }

    /// 通知逻辑服玩家离线
    ///
    /// 返回逻辑服需要下发的消息列表
    pub async fn notify_player_offline(
        &self,
        player_uid: u64,
        session_id: u64,
        reason: &str,
    ) -> Result<Vec<DownstreamMessage>, GateError> {
        let endpoint = self
            .conn_pool
            .next_endpoint()
            .ok_or_else(|| GateError::Grpc("无可用逻辑服端点".into()))?;

        let channel = match get_or_create_channel(&endpoint).await {
            Ok(ch) => ch,
            Err(_) => {
                // 离线通知失败不阻断流程
                debug!("玩家离线通知跳过（逻辑服不可达）: uid={}", player_uid);
                return Ok(vec![]);
            }
        };

        let mut client = LogicServiceClient::new(channel);

        let request = tonic::Request::new(
            crate::grpc_router::proto::gate::PlayerOfflineRequest {
                player_uid,
                session_id,
                reason: reason.to_string(),
            },
        );

        match client.player_offline(request).await {
            Ok(response) => Ok(response.into_inner().messages),
            Err(e) => {
                debug!("玩家离线通知失败（不阻断）: uid={} err={}", player_uid, e);
                Ok(vec![])
            }
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grpc_url_normalization() {
        assert_eq!(normalize_endpoint("grpc://127.0.0.1:50051"), "http://127.0.0.1:50051");
        assert_eq!(normalize_endpoint("http://127.0.0.1:50051"), "http://127.0.0.1:50051");
        assert_eq!(normalize_endpoint("127.0.0.1:50051"), "http://127.0.0.1:50051");
    }

    #[test]
    fn test_upstream_router_creation() {
        // 验证 UpstreamRouter 可正常创建（需要 GrpcConnPool）
        let pool = Arc::new(GrpcConnPool::new(vec!["grpc://127.0.0.1:50051".into()]));
        let _router = UpstreamRouter::new(pool);
    }
}
