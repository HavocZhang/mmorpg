//! 场景服 (Scene Server) — 地图管理 + 移动同步 + AOI 视野
//!
//! 作为 gRPC LogicService 后端，接收网关转发的玩家消息。
//!
//! ## 消息协议
//! 上行: 3001=移动 4001=查询附近玩家 4002=查询附近实体 4003=加入地图
//! 下行: 8001=位置更新 8002=玩家进入视野 8003=玩家离开视野 9001=玩家列表 9002=实体列表
//!
//! ## 运行
//! ```bash
//! cargo run --bin scene-server
//! ```

use std::net::SocketAddr;
use std::sync::Arc;

use dashmap::DashMap;
use parking_lot::RwLock;
use serde_json::Value;
use tonic::{transport::Server, Request, Response, Status};

use rust_mmo_gate::grpc_router::proto::gate::{
    logic_service_server::{LogicService, LogicServiceServer},
    DownstreamMessage, ForwardBatchRequest, ForwardRequest, ForwardResponse,
    PlayerOfflineRequest, PlayerOfflineResponse, PlayerOnlineRequest, PlayerOnlineResponse,
};
use logic_lib::scene::SceneManager;

pub struct SceneService {
    manager: Arc<RwLock<SceneManager>>,
    /// uid -> username
    player_names: DashMap<u64, String>,
}

impl SceneService {
    fn new() -> Self {
        let mut mgr = SceneManager::new();
        // 预加载默认地图
        mgr.load_map("新手村", 1000.0, 1000.0, 100.0);
        mgr.load_map("主城", 2000.0, 2000.0, 150.0);
        mgr.load_map("野外", 5000.0, 5000.0, 200.0);
        mgr.load_map("副本", 500.0, 500.0, 100.0);

        Self {
            manager: Arc::new(RwLock::new(mgr)),
            player_names: DashMap::new(),
        }
    }
}

#[tonic::async_trait]
impl LogicService for SceneService {
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
        println!("[SceneServer] 玩家上线: uid={} gate={}", r.player_uid, r.gate_node);
        Ok(Response::new(PlayerOnlineResponse { ok: true, messages: vec![] }))
    }

    async fn player_offline(&self, req: Request<PlayerOfflineRequest>) -> Result<Response<PlayerOfflineResponse>, Status> {
        let r = req.into_inner();
        println!("[SceneServer] 玩家离线: uid={} reason={}", r.player_uid, r.reason);
        let mut mgr = self.manager.write();
        mgr.leave(r.player_uid);
        self.player_names.remove(&r.player_uid);
        Ok(Response::new(PlayerOfflineResponse { ok: true, messages: vec![] }))
    }
}

impl SceneService {
    fn process(&self, uid: u64, msg_id: u32, payload: &[u8]) -> ForwardResponse {
        let json: Value = serde_json::from_slice(payload).unwrap_or(Value::Null);

        let messages = match msg_id {
            // 4003: 加入地图 {"mapName":"新手村","x":500,"y":300}
            4003 => self.handle_join(uid, &json),
            // 3001: 移动 {"x":150,"y":160}
            3001 => self.handle_move(uid, &json),
            // 4001: 查询附近玩家
            4001 => self.handle_nearby_players(uid),
            // 4002: 查询附近实体
            4002 => self.handle_nearby_entities(uid),
            _ => vec![dm(uid, msg_id + 5000, json.to_string(), 0)],
        };

        ForwardResponse { messages }
    }

    fn handle_join(&self, uid: u64, json: &Value) -> Vec<DownstreamMessage> {
        let map_name = json.get("mapName").and_then(|v| v.as_str()).unwrap_or("新手村");
        let x = json.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let y = json.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let username = json.get("username").and_then(|v| v.as_str()).unwrap_or("Unknown");

        let mut mgr = self.manager.write();
        self.player_names.insert(uid, username.to_string());

        match mgr.join(uid, map_name, x, y) {
            Ok(_) => {
                let result = serde_json::json!({"success":true,"mapName":map_name,"x":x,"y":y}).to_string();
                vec![dm(uid, 8100, result, 2)]
            }
            Err(e) => vec![err(uid, 8100, &e.to_string())],
        }
    }

    fn handle_move(&self, uid: u64, json: &Value) -> Vec<DownstreamMessage> {
        let x = json.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let y = json.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0);

        let mut mgr = self.manager.write();
        match mgr.move_player(uid, x, y) {
            Ok(_) => {
                let pos_json = serde_json::json!({"uid":uid,"x":x,"y":y}).to_string();

                // 查询附近玩家，广播位置更新
                if let Ok(nearby) = mgr.query_range(uid, 300.0) {
                    let mut msgs: Vec<DownstreamMessage> = nearby.iter()
                        .filter(|&&id| id != uid)
                        .map(|&id| dm(id, 8001, pos_json.clone(), 1))
                        .collect();
                    // 自己也收到确认
                    msgs.push(dm(uid, 8001, pos_json, 1));
                    return msgs;
                }
                vec![dm(uid, 8001, pos_json, 1)]
            }
            Err(e) => vec![err(uid, 8100, &e.to_string())],
        }
    }

    fn handle_nearby_players(&self, uid: u64) -> Vec<DownstreamMessage> {
        let mgr = self.manager.read();
        match mgr.query_range(uid, 300.0) {
            Ok(nearby) => {
                let players: Vec<Value> = nearby.iter()
                    .filter(|&&id| id != uid)
                    .map(|&id| {
                        let name = self.player_names.get(&id).map(|n| n.clone()).unwrap_or_default();
                        let pos = mgr.get_position(id).unwrap_or((0.0, 0.0));
                        serde_json::json!({"uid":id,"name":name,"x":pos.0,"y":pos.1})
                    })
                    .collect();
                let result = serde_json::json!({"players":players}).to_string();
                vec![dm(uid, 9001, result, 0)]
            }
            Err(e) => vec![err(uid, 9001, &e.to_string())],
        }
    }

    fn handle_nearby_entities(&self, uid: u64) -> Vec<DownstreamMessage> {
        // 简化版：返回空实体列表（完整版本需维护 NPC 状态）
        let result = r#"{"entities":[]}"#.to_string();
        vec![dm(uid, 9002, result, 0)]
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
    let addr: SocketAddr = "0.0.0.0:50053".parse()?;

    println!("╔═══════════════════════════════════════════╗");
    println!("║   MMORPG 场景服 (Scene Server)            ║");
    println!("╠═══════════════════════════════════════════╣");
    println!("║   gRPC 监听: {}                    ║", addr);
    println!("║   AOI 算法: 九宫格                       ║");
    println!("║   预加载地图: 新手村/主城/野外/副本        ║");
    println!("╚═══════════════════════════════════════════╝");
    println!();
    println!("消息协议:");
    println!("  上行: 3001=移动 4001=查询附近玩家 4002=查询实体 4003=加入地图");
    println!("  下行: 8001=位置更新 8100=操作结果 9001=玩家列表 9002=实体列表");
    println!();

    let service = SceneService::new();
    Server::builder()
        .add_service(LogicServiceServer::new(service))
        .serve(addr)
        .await?;
    Ok(())
}
