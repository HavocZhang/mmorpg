//! 握手鉴权模块
//!
//! 处理新连接的握手流程：
//! 1. 接收握手包（包含 token、客户端版本、uid）
//! 2. 校验客户端版本
//! 3. 校验 token 合法性与过期时间
//! 4. 创建会话并绑定 player_uid
//! 5. 启动 ReadLoop / WriteLoop
//!
//! 握手消息格式（JSON，AES-GCM 加密后放入协议包体）：
//! ```json
//! {"uid": 12345, "token": "xxxx", "version": 1, "timestamp": 1700000000}
//! ```

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tracing::{debug, warn};

use crate::cluster::cross_gate_pubsub::{CrossGateMsg, CrossGatePubSub};
use crate::cluster::route_index::RouteIndex;
use crate::cluster::ClusterManager;
use crate::config::AppConfig;
use crate::crypto::aes_gcm::AesGcmCipher;
use crate::foundation::GateError;
use crate::grpc_router::downstream::DownstreamDispatcher;
use crate::grpc_router::proto::gate::DownstreamMessage;
use crate::grpc_router::RouterManager;
use crate::io_engine::read_loop::ReadLoop;
use crate::io_engine::write_loop::WriteLoop;
use crate::protocol::decoder::PacketDecoder;
use crate::protocol::packet_struct::HEADER_SIZE;
use crate::security::SecurityManager;
use crate::session::session_mgr::SessionManager;
use crate::session::session_struct::{MsgPriority, PendingMsg};

/// Token 最大有效期（秒）：10 分钟
const TOKEN_MAX_TTL_SECS: u64 = 600;

/// 握手请求载荷（客户端发送，AES-GCM 加密后放入协议包体）
#[derive(Debug, Serialize, Deserialize)]
pub struct HandshakePayload {
    /// 玩家UID
    pub uid: u64,
    /// 认证令牌
    pub token: String,
    /// 客户端协议版本
    pub version: u32,
    /// 时间戳（秒），用于 token 过期校验
    pub timestamp: u64,
}

/// 握手结果信息
pub struct HandshakeInfo {
    pub player_uid: u64,
    pub client_version: u32,
    pub token: String,
}

/// 处理单个TCP连接的完整生命周期
pub async fn handle_connection(
    stream: TcpStream,
    peer_addr: SocketAddr,
    session_mgr: Arc<SessionManager>,
    security_mgr: Arc<SecurityManager>,
    router_mgr: Arc<RouterManager>,
    cluster_mgr: Arc<ClusterManager>,
    config: &AppConfig,
) -> Result<(), GateError> {
    debug!("新连接: {}", peer_addr);

    // 设置 TCP_NODELAY
    stream.set_nodelay(true).ok();

    // 读写分离
    let (mut read_half, write_half) = stream.into_split();

    // --- 握手 ---
    let cipher = AesGcmCipher::from_hex_key(&config.crypto.aes_key)
        .map_err(|e| GateError::Internal(format!("AES密钥初始化失败: {}", e)))?;

    let handshake_result = perform_handshake(&mut read_half, &cipher, config).await;

    match handshake_result {
        Ok(handshake_info) => {
            debug!(
                "握手成功: {} uid={} version={}",
                peer_addr, handshake_info.player_uid, handshake_info.client_version
            );

            // 创建会话，获取 send_rx 用于 WriteLoop
            let (session_id, send_rx) = session_mgr
                .create_session(peer_addr, handshake_info.player_uid)
                .await;

            // --- 启动 WriteLoop ---
            let mut write_loop = WriteLoop::new(write_half, config.io.packet_merge_window_ms, cipher.clone());
            let write_handle = tokio::spawn(async move {
                if let Err(e) = write_loop.run(send_rx).await {
                    warn!("WriteLoop 异常退出: {}", e);
                }
            });

            // --- 通知逻辑服玩家上线（仅在 gRPC 可用时） ---
            if router_mgr.conn_pool.has_healthy_endpoint() {
                let pool_for_online = router_mgr.conn_pool.clone();
                let node_name_for_online = config.gate.node_name.clone();
                let uid_for_online = handshake_info.player_uid;
                let session_mgr_for_online = session_mgr.clone();
                let cluster_for_online = cluster_mgr.clone();
                tokio::spawn(async move {
                    let router = crate::grpc_router::upstream::UpstreamRouter::new(pool_for_online);
                    match router
                        .notify_player_online(uid_for_online, session_id, &node_name_for_online)
                        .await
                    {
                        Ok(messages) => {
                            // 分发逻辑服返回的上线消息
                            let downstream = Arc::new(DownstreamDispatcher::new(session_mgr_for_online.clone()));
                            for dm in &messages {
                                dispatch_downstream(
                                    &downstream,
                                    dm,
                                    &session_mgr_for_online,
                                    &cluster_for_online,
                                    session_id,
                                );
                            }
                        }
                        Err(e) => {
                            debug!("玩家上线通知失败（不阻断）: uid={} err={}", uid_for_online, e);
                        }
                    }
                });
            }

            // --- 更新 Redis 路由索引 ---
            let uid_for_route = handshake_info.player_uid;
            let node_id = cluster_mgr.node_id;
            if let Some(redis_conn) = cluster_mgr.conn() {
                let route_index = RouteIndex::new(cluster_mgr.redis_url.clone());
                if let Err(e) = route_index
                    .update_route(redis_conn, uid_for_route, node_id)
                    .await
                {
                    warn!("路由索引更新失败（不阻断）: uid={} err={}", uid_for_route, e);
                }
            }

            // --- 启动 ReadLoop ---
            let decoder = PacketDecoder::new(cipher);
            let mut read_loop = ReadLoop::new(decoder, read_half);

            let session_mgr_clone = session_mgr.clone();
            let conn_pool = router_mgr.conn_pool.clone();
            let security_mgr_clone = security_mgr.clone();
            let player_uid = handshake_info.player_uid;
            let cluster_mgr_clone = cluster_mgr.clone();

            // 创建下行分发器
            let downstream = Arc::new(DownstreamDispatcher::new(session_mgr.clone()));

            let read_handle = tokio::spawn(async move {
                let result = read_loop.run(|msg_id, payload| {
                    // 更新会话活跃时间
                    session_mgr_clone.touch_session(session_id);

                    // 安全校验：消息频率限流
                    let is_battle = (1000..2000).contains(&msg_id);
                    if !security_mgr_clone.check_player_rate(player_uid, is_battle) {
                        warn!("玩家消息限流: uid={} msg_id={}", player_uid, msg_id);
                        return;
                    }

                    // 上行路由到逻辑服（快速路径优化）
                    // 当所有 gRPC 端点不健康时，跳过 spawn 直接本地回显
                    if !conn_pool.has_healthy_endpoint() {
                        // 快速降级：本地回显，不创建异步任务
                        let priority = if is_battle {
                            MsgPriority::High
                        } else if (2000..3000).contains(&msg_id) {
                            MsgPriority::Normal
                        } else {
                            MsgPriority::Low
                        };
                        if let Some(session) = session_mgr_clone.get_session(session_id) {
                            if session.is_online() {
                                let msg = PendingMsg {
                                    msg_id,
                                    payload: payload.clone(),
                                    priority,
                                };
                                let _ = session.send(msg);
                            }
                        }
                    } else {
                        let pool = conn_pool.clone();
                        let downstream_clone = downstream.clone();
                        let session_mgr_for_fallback = session_mgr_clone.clone();
                        let cluster_for_dispatch = cluster_mgr_clone.clone();
                        tokio::spawn(async move {
                            let router = crate::grpc_router::upstream::UpstreamRouter::new(pool);

                            match router.route(player_uid, msg_id, payload.clone()).await {
                                Ok(response) => {
                                    // 成功收到逻辑服响应，分发下行消息
                                    for dm in &response.messages {
                                        dispatch_downstream(
                                            &downstream_clone,
                                            dm,
                                            &session_mgr_for_fallback,
                                            &cluster_for_dispatch,
                                            session_id,
                                        );
                                    }
                                }
                                Err(e) => {
                                    // 逻辑服不可用，降级为本地回显
                                    debug!(
                                        "逻辑服不可用，降级回显: uid={} msg_id={} err={}",
                                        player_uid, msg_id, e
                                    );

                                    let priority = if is_battle {
                                        MsgPriority::High
                                    } else if (2000..3000).contains(&msg_id) {
                                        MsgPriority::Normal
                                    } else {
                                        MsgPriority::Low
                                    };

                                    // 回显给发送者
                                    if let Some(session) =
                                        session_mgr_for_fallback.get_session(session_id)
                                    {
                                        if session.is_online() {
                                            let msg = PendingMsg {
                                                msg_id,
                                                payload: payload.clone(),
                                                priority,
                                            };
                                            let _ = session.send(msg);
                                        }
                                    }
                                }
                            }
                        });
                    }
                }).await;

                match &result {
                    Ok(()) => debug!("ReadLoop 正常退出: session_id={}", session_id),
                    Err(e) => warn!("ReadLoop 异常退出: session_id={} err={}", session_id, e),
                }

                // ReadLoop 结束意味着连接断开，通知逻辑服玩家离线（仅在 gRPC 可用时）
                if conn_pool.has_healthy_endpoint() {
                    let pool_for_offline = conn_pool.clone();
                    let uid_for_offline = player_uid;
                    let cluster_for_offline = cluster_mgr_clone.clone();
                    let session_mgr_for_offline = session_mgr_clone.clone();
                    tokio::spawn(async move {
                        let router = crate::grpc_router::upstream::UpstreamRouter::new(pool_for_offline);
                        if let Ok(messages) = router
                            .notify_player_offline(uid_for_offline, session_id, "连接断开")
                            .await {
                            // 分发逻辑服返回的离线消息（如玩家离开广播）
                            let downstream = Arc::new(DownstreamDispatcher::new(session_mgr_for_offline.clone()));
                            for dm in &messages {
                                dispatch_downstream(
                                    &downstream,
                                    dm,
                                    &session_mgr_for_offline,
                                    &cluster_for_offline,
                                    session_id,
                                );
                            }
                        }

                        // 删除 Redis 路由索引
                        if let Some(redis_conn) = cluster_for_offline.conn() {
                            let route_index = RouteIndex::new(cluster_for_offline.redis_url.clone());
                            if let Err(e) = route_index.remove_route(redis_conn, uid_for_offline).await {
                                warn!("路由索引删除失败: uid={} err={}", uid_for_offline, e);
                            }
                        }
                    });
                } else {
                    // gRPC 不可用时仅清理 Redis 路由索引
                    let cluster_for_offline = cluster_mgr_clone.clone();
                    tokio::spawn(async move {
                        if let Some(redis_conn) = cluster_for_offline.conn() {
                            let route_index = RouteIndex::new(cluster_for_offline.redis_url.clone());
                            if let Err(e) = route_index.remove_route(redis_conn, player_uid).await {
                                warn!("路由索引删除失败: uid={} err={}", player_uid, e);
                            }
                        }
                    });
                }

                // 清理会话
                session_mgr_clone.remove_session(session_id);

                result
            });

            // 等待任一循环结束即断开连接
            tokio::select! {
                _ = write_handle => {
                    debug!("WriteLoop 结束，断开连接: session_id={}", session_id);
                }
                _ = read_handle => {
                    // ReadLoop 结束已自行清理会话
                }
            }

            debug!("连接关闭: session_id={} uid={}", session_id, handshake_info.player_uid);
        }
        Err(e) => {
            warn!("握手失败: {} 错误: {}", peer_addr, e);
            if e.is_security() {
                security_mgr.record_security_event(&peer_addr.ip(), &e);
            }
            return Err(e);
        }
    }

    Ok(())
}

/// 执行握手协议
///
/// 从 TCP 流读取第一个协议包，解密后解析握手信息
async fn perform_handshake(
    read_half: &mut tokio::net::tcp::OwnedReadHalf,
    cipher: &AesGcmCipher,
    _config: &AppConfig,
) -> Result<HandshakeInfo, GateError> {
    // 1. 读取包头（16 字节）
    let mut header_buf = [0u8; HEADER_SIZE];
    read_half
        .read_exact(&mut header_buf)
        .await
        .map_err(|_| GateError::Protocol("握手包读取失败: 连接过早关闭".into()))?;

    // 2. 解析包头
    let header = crate::protocol::packet_struct::PacketHeader::from_bytes(&header_buf)?;

    // 3. 校验协议版本
    let server_version = crate::protocol::packet_struct::PROTOCOL_VERSION as u32;
    if header.version as u32 != server_version {
        return Err(GateError::VersionMismatch {
            client: header.version as u32,
            server: server_version,
        });
    }

    // 4. 读取包体
    let body_len = header.body_len as usize;
    if body_len > crate::protocol::packet_struct::MAX_BODY_SIZE {
        return Err(GateError::PacketTooLarge {
            size: body_len,
            max: crate::protocol::packet_struct::MAX_BODY_SIZE,
        });
    }

    let mut body_buf = vec![0u8; body_len];
    read_half
        .read_exact(&mut body_buf)
        .await
        .map_err(|_| GateError::Protocol("握手包体读取失败: 连接过早关闭".into()))?;

    // 5. 校验 CRC32
    let expected_crc = crate::crypto::crc32::checksum(&body_buf);
    if expected_crc != header.crc32 {
        return Err(GateError::CrcMismatch);
    }

    // 6. AES-GCM 解密
    let decrypted = cipher
        .decrypt(&body_buf)
        .map_err(|_| GateError::AesDecryptFailed)?;

    // 7. 解析握手 JSON
    let payload: HandshakePayload = serde_json::from_slice(&decrypted).map_err(|e| {
        GateError::Protocol(format!("握手消息JSON解析失败: {}", e))
    })?;

    // 8. 校验客户端版本
    if payload.version != server_version {
        return Err(GateError::VersionMismatch {
            client: payload.version,
            server: server_version,
        });
    }

    // 9. 校验 token
    validate_token(&payload.token, payload.timestamp)?;

    // 10. 校验 uid 合法性
    if payload.uid == 0 {
        return Err(GateError::InvalidToken);
    }

    Ok(HandshakeInfo {
        player_uid: payload.uid,
        client_version: payload.version,
        token: payload.token,
    })
}

/// 校验 token 合法性与时效性
///
/// 当前实现为本地校验（无 Redis 依赖）：
/// - token 非空且长度 >= 8
/// - timestamp 在 TOKEN_MAX_TTL_SECS 有效期内
///
/// 生产环境应替换为 Redis 查询验证
fn validate_token(token: &str, timestamp: u64) -> Result<(), GateError> {
    // token 非空
    if token.is_empty() || token.len() < 8 {
        return Err(GateError::InvalidToken);
    }

    // 时间戳校验：防止重放攻击
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // 允许时钟偏差 60 秒
    if timestamp > now + 60 {
        return Err(GateError::ExpiredToken);
    }

    if now > timestamp + TOKEN_MAX_TTL_SECS {
        return Err(GateError::ExpiredToken);
    }

    Ok(())
}

/// 将逻辑服的下行消息分发给目标玩家
///
/// - target_uid = 0 时广播给所有在线玩家（含跨网关）
/// - 否则精准下发给指定玩家（本地优先，跨网关走 PubSub）
fn dispatch_downstream(
    dispatcher: &Arc<DownstreamDispatcher>,
    msg: &DownstreamMessage,
    _session_mgr: &Arc<SessionManager>,
    cluster_mgr: &Arc<ClusterManager>,
    _sender_session_id: u64,
) {
    let priority = match msg.priority {
        2 => MsgPriority::High,
        1 => MsgPriority::Normal,
        _ => MsgPriority::Low,
    };

    let msg_id = msg.msg_id as u16;
    let payload = msg.payload.clone();

    if msg.target_uid == 0 {
        // 广播给所有在线玩家
        dispatcher.broadcast(msg_id, payload.clone(), priority);

        // 同时通过 PubSub 广播到其他网关
        if let Some(redis_conn) = cluster_mgr.conn() {
            let pubsub = CrossGatePubSub::new(cluster_mgr.node_id, cluster_mgr.redis_url.clone());
            let cross_msg = CrossGateMsg {
                from_node: cluster_mgr.node_id,
                to_uid: 0,
                msg_id,
                payload,
                priority: msg.priority as u8,
            };
            tokio::spawn(async move {
                if let Err(e) = pubsub.publish_broadcast(redis_conn, cross_msg).await {
                    debug!("跨网关广播失败（不阻断）: {}", e);
                }
            });
        }
    } else {
        // 精准下发给目标玩家
        // 先尝试本地分发
        let local_ok = dispatcher.dispatch(msg.target_uid, msg_id, payload.clone(), priority).is_ok();

        if !local_ok {
            // 本地未找到目标玩家，查 Redis 路由表
            if let Some(redis_conn) = cluster_mgr.conn() {
                let pubsub = CrossGatePubSub::new(cluster_mgr.node_id, cluster_mgr.redis_url.clone());
                let route_index = RouteIndex::new(cluster_mgr.redis_url.clone());
                let target_uid = msg.target_uid;
                let from_node = cluster_mgr.node_id;
                let p = msg.priority as u8;

                tokio::spawn(async move {
                    match route_index.get_gate_node(redis_conn.clone(), target_uid).await {
                        Ok(Some(target_node)) => {
                            let cross_msg = CrossGateMsg {
                                from_node,
                                to_uid: target_uid,
                                msg_id,
                                payload,
                                priority: p,
                            };
                            if let Err(e) = pubsub.publish(redis_conn, target_node, cross_msg).await {
                                debug!("跨网关定向投递失败: target_uid={} err={}", target_uid, e);
                            }
                        }
                        Ok(None) => {
                            debug!("目标玩家不在任何网关: uid={}", target_uid);
                        }
                        Err(e) => {
                            debug!("路由查询失败: uid={} err={}", target_uid, e);
                        }
                    }
                });
            } else {
                debug!(
                    "Redis不可用，无法跨网关投递: target_uid={} msg_id={}",
                    msg.target_uid, msg_id
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

    #[test]
    fn test_validate_token_valid() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(validate_token("valid_token_123", now).is_ok());
    }

    #[test]
    fn test_validate_token_empty() {
        assert!(validate_token("", 0).is_err());
    }

    #[test]
    fn test_validate_token_too_short() {
        assert!(validate_token("short", 0).is_err());
    }

    #[test]
    fn test_validate_token_expired() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        // 10 分钟前的 timestamp
        assert!(validate_token("valid_token_123", now - TOKEN_MAX_TTL_SECS - 1).is_err());
    }

    #[test]
    fn test_validate_token_future() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        // 未来 2 分钟
        assert!(validate_token("valid_token_123", now + 120).is_err());
    }

    #[test]
    fn test_handshake_payload_serde() {
        let payload = HandshakePayload {
            uid: 12345,
            token: "test_token_abc".to_string(),
            version: 1,
            timestamp: 1700000000,
        };
        let json = serde_json::to_vec(&payload).unwrap();
        let restored: HandshakePayload = serde_json::from_slice(&json).unwrap();
        assert_eq!(restored.uid, 12345);
        assert_eq!(restored.token, "test_token_abc");
        assert_eq!(restored.version, 1);
    }

    #[test]
    fn test_handshake_payload_roundtrip_with_crypto() {
        let cipher = AesGcmCipher::from_hex_key(TEST_KEY).unwrap();

        let payload = HandshakePayload {
            uid: 99999,
            token: "roundtrip_token".to_string(),
            version: 1,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        // 序列化 + 加密
        let json = serde_json::to_vec(&payload).unwrap();
        let encrypted = cipher.encrypt(&json).unwrap();

        // 模拟组装协议包
        let packet = crate::protocol::packet_struct::Packet::new(0x0001, encrypted);
        let packet_bytes = packet.to_bytes();

        // 解析包头
        let header = crate::protocol::packet_struct::PacketHeader::from_bytes(
            &packet_bytes[..HEADER_SIZE],
        )
        .unwrap();

        // 提取包体
        let body = &packet_bytes[HEADER_SIZE..];

        // 校验 CRC
        assert_eq!(crate::crypto::crc32::checksum(body), header.crc32);

        // 解密
        let decrypted = cipher.decrypt(body).unwrap();

        // 反序列化
        let restored: HandshakePayload = serde_json::from_slice(&decrypted).unwrap();
        assert_eq!(restored.uid, payload.uid);
        assert_eq!(restored.token, payload.token);
        assert_eq!(restored.version, payload.version);
    }
}
