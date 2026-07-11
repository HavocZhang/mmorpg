//! 聊天服 (Chat Server) — 频道管理 + 消息收发 + 频率限制 + 敏感词过滤
//!
//! 作为 gRPC LogicService 后端，接收网关转发的玩家消息。
//!
//! ## 消息协议
//! 上行: 2001=发送聊天 2002=查询历史 2003=加入频道 2004=离开频道
//! 下行: 7001=聊天ACK 7002=聊天广播 7003=历史消息 7100=系统消息
//!
//! ## 运行
//! ```bash
//! cargo run --bin chat-server
//! ```

use std::net::SocketAddr;
use std::sync::Arc;

use parking_lot::RwLock;
use serde_json::Value;
use tonic::{transport::Server, Request, Response, Status};

use rust_mmo_gate::grpc_router::proto::gate::{
    logic_service_server::{LogicService, LogicServiceServer},
    DownstreamMessage, ForwardBatchRequest, ForwardRequest, ForwardResponse,
    PlayerOfflineRequest, PlayerOfflineResponse, PlayerOnlineRequest, PlayerOnlineResponse,
};
use logic_lib::chat::ChatManager;

pub struct ChatService {
    manager: Arc<RwLock<ChatManager>>,
}

impl ChatService {
    fn new() -> Self {
        let mut mgr = ChatManager::new();
        // Pre-create default channels
        mgr.ensure_world_channel();
        Self {
            manager: Arc::new(RwLock::new(mgr)),
        }
    }
}

#[tonic::async_trait]
impl LogicService for ChatService {
    async fn forward_message(&self, req: Request<ForwardRequest>) -> Result<Response<ForwardResponse>, Status> {
        let r = req.into_inner();
        Ok(Response::new(self.process(r.player_uid, r.msg_id, &r.payload)))
    }

    async fn forward_message_batch(&self, req: Request<ForwardBatchRequest>) -> Result<Response<ForwardResponse>, Status> {
        let mut all = Vec::new();
        for m in req.into_inner().messages {
            all.extend(self.process(m.player_uid, m.msg_id, &m.payload).messages);
        }
        Ok(Response::new(ForwardResponse { messages: all }))
    }

    async fn player_online(&self, req: Request<PlayerOnlineRequest>) -> Result<Response<PlayerOnlineResponse>, Status> {
        let r = req.into_inner();
        println!("[ChatServer] 玩家上线: uid={} gate={}", r.player_uid, r.gate_node);
        self.manager.write().player_online(r.player_uid);
        Ok(Response::new(PlayerOnlineResponse { ok: true, messages: vec![] }))
    }

    async fn player_offline(&self, req: Request<PlayerOfflineRequest>) -> Result<Response<PlayerOfflineResponse>, Status> {
        let r = req.into_inner();
        println!("[ChatServer] 玩家离线: uid={} reason={}", r.player_uid, r.reason);
        self.manager.write().player_offline(r.player_uid);
        Ok(Response::new(PlayerOfflineResponse { ok: true, messages: vec![] }))
    }
}

impl ChatService {
    fn process(&self, uid: u64, msg_id: u32, payload: &[u8]) -> ForwardResponse {
        let json: Value = serde_json::from_slice(payload).unwrap_or(Value::Null);

        let messages = match msg_id {
            // 2001: 发送聊天 {"channel":"world","text":"hello","channelType":"world"|"guild"|"party"|"private","targetUid":0}
            2001 => self.handle_send(uid, &json),
            // 2002: 查询历史 {"channel":"world","limit":10}
            2002 => self.handle_history(uid, &json),
            // 2003: 加入频道 {"channel":"world"}
            2003 => self.handle_join_channel(uid, &json),
            // 2004: 离开频道 {"channel":"world"}
            2004 => self.handle_leave_channel(uid, &json),
            _ => vec![dm(uid, 7100, r#"{"type":"unknown","msgId":0}"#.to_string(), 0)],
        };

        ForwardResponse { messages }
    }

    fn handle_send(&self, uid: u64, json: &Value) -> Vec<DownstreamMessage> {
        let channel = json.get("channel").and_then(|v| v.as_str()).unwrap_or("world");
        let text = json.get("text").and_then(|v| v.as_str()).unwrap_or("");
        let channel_type = json.get("channelType").and_then(|v| v.as_str()).unwrap_or("world");
        let target_uid = json.get("targetUid").and_then(|v| v.as_u64()).unwrap_or(0);

        let mut mgr = self.manager.write();

        match channel_type {
            "private" => {
                match mgr.send_private(uid, target_uid, text) {
                    Ok(_) => {
                        let ack = serde_json::json!({"success":true,"msgId":2001}).to_string();
                        let mut msgs = vec![dm(uid, 7001, ack, 1)];
                        // Also deliver to target if online
                        let broadcast = serde_json::json!({
                            "from": uid,
                            "channelType": "private",
                            "text": text,
                        }).to_string();
                        msgs.push(dm(target_uid, 7002, broadcast, 1));
                        msgs
                    }
                    Err(e) => vec![err(uid, 7100, &e.to_string())],
                }
            }
            "guild" => {
                let guild_name = channel;
                match mgr.send_guild(uid, guild_name, text) {
                    Ok(members) => {
                        let ack = serde_json::json!({"success":true,"msgId":2001}).to_string();
                        let mut msgs = vec![dm(uid, 7001, ack, 1)];
                        let broadcast = serde_json::json!({
                            "from": uid,
                            "channelType": "guild",
                            "guild": guild_name,
                            "text": text,
                        }).to_string();
                        for member_uid in members {
                            if member_uid != uid {
                                msgs.push(dm(member_uid, 7002, broadcast.clone(), 1));
                            }
                        }
                        msgs
                    }
                    Err(e) => vec![err(uid, 7100, &e.to_string())],
                }
            }
            "party" => {
                let party_id = json.get("partyId").and_then(|v| v.as_u64()).unwrap_or(0);
                match mgr.send_party(uid, party_id, text) {
                    Ok(members) => {
                        let ack = serde_json::json!({"success":true,"msgId":2001}).to_string();
                        let mut msgs = vec![dm(uid, 7001, ack, 1)];
                        let broadcast = serde_json::json!({
                            "from": uid,
                            "channelType": "party",
                            "partyId": party_id,
                            "text": text,
                        }).to_string();
                        for member_uid in members {
                            if member_uid != uid {
                                msgs.push(dm(member_uid, 7002, broadcast.clone(), 1));
                            }
                        }
                        msgs
                    }
                    Err(e) => vec![err(uid, 7100, &e.to_string())],
                }
            }
            _ => {
                // world or any other channel
                match mgr.send_to_channel(uid, channel, text) {
                    Ok(members) => {
                        let ack = serde_json::json!({"success":true,"msgId":2001}).to_string();
                        let mut msgs = vec![dm(uid, 7001, ack, 1)];
                        let broadcast = serde_json::json!({
                            "from": uid,
                            "channelType": "world",
                            "channel": channel,
                            "text": text,
                        }).to_string();
                        for member_uid in members {
                            if member_uid != uid {
                                msgs.push(dm(member_uid, 7002, broadcast.clone(), 1));
                            }
                        }
                        msgs
                    }
                    Err(e) => vec![err(uid, 7100, &e.to_string())],
                }
            }
        }
    }

    fn handle_history(&self, uid: u64, json: &Value) -> Vec<DownstreamMessage> {
        let channel = json.get("channel").and_then(|v| v.as_str()).unwrap_or("world");
        let limit = json.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

        let mgr = self.manager.read();
        let history = mgr.query_history(channel, limit);
        let entries: Vec<Value> = history.iter().map(|(from_uid, text)| {
            serde_json::json!({"from": from_uid, "text": text})
        }).collect();
        let result = serde_json::json!({
            "channel": channel,
            "messages": entries,
            "count": entries.len(),
        }).to_string();

        vec![dm(uid, 7003, result, 0)]
    }

    fn handle_join_channel(&self, uid: u64, json: &Value) -> Vec<DownstreamMessage> {
        let channel = json.get("channel").and_then(|v| v.as_str()).unwrap_or("world");
        let mut mgr = self.manager.write();
        match mgr.join_channel(uid, channel) {
            Ok(_) => {
                let sys_msg = serde_json::json!({
                    "type": "system",
                    "text": format!("加入了频道 {}", channel),
                }).to_string();
                vec![dm(uid, 7100, sys_msg, 1)]
            }
            Err(e) => vec![err(uid, 7100, &e.to_string())],
        }
    }

    fn handle_leave_channel(&self, uid: u64, json: &Value) -> Vec<DownstreamMessage> {
        let channel = json.get("channel").and_then(|v| v.as_str()).unwrap_or("world");
        self.manager.write().leave_channel(uid, channel);
        let sys_msg = serde_json::json!({
            "type": "system",
            "text": format!("离开了频道 {}", channel),
        }).to_string();
        vec![dm(uid, 7100, sys_msg, 1)]
    }
}

fn dm(target_uid: u64, msg_id: u32, payload: String, priority: u32) -> DownstreamMessage {
    DownstreamMessage { target_uid, msg_id, payload: payload.into_bytes(), priority }
}

fn err(uid: u64, msg_id: u32, error: &str) -> DownstreamMessage {
    let json = serde_json::json!({"success":false,"error":error}).to_string();
    DownstreamMessage { target_uid: uid, msg_id, payload: json.into_bytes(), priority: 2 }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr: SocketAddr = "0.0.0.0:50055".parse()?;

    println!("╔═══════════════════════════════════════════╗");
    println!("║   MMORPG 聊天服 (Chat Server)             ║");
    println!("╠═══════════════════════════════════════════╣");
    println!("║   gRPC 监听: {}                    ║", addr);
    println!("║   频道: 世界/私聊/公会/队伍               ║");
    println!("║   功能: 频率限制/敏感词过滤/离线消息       ║");
    println!("╚═══════════════════════════════════════════╝");
    println!();
    println!("消息协议:");
    println!("  上行: 2001=发送聊天 2002=查询历史 2003=加入频道 2004=离开频道");
    println!("  下行: 7001=聊天ACK 7002=聊天广播 7003=历史消息 7100=系统消息");
    println!();

    let service = ChatService::new();
    Server::builder()
        .add_service(LogicServiceServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}
