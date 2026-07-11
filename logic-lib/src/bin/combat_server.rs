//! 战斗服 (Combat Server) — 攻击结算 + Buff 管理 + 经验系统
//!
//! 作为 gRPC LogicService 后端，接收网关转发的玩家战斗消息。
//!
//! ## 消息协议
//! 上行: 1001=基础攻击 1002=技能攻击
//! 下行: 6001=战斗结果 6002=实体状态 6003=实体死亡/掉落 6100=经验更新
//!
//! ## 运行
//! ```bash
//! cargo run --bin combat-server
//! ```

use std::net::SocketAddr;
use std::sync::Arc;

use dashmap::DashMap;
use parking_lot::RwLock;
use serde_json::Value;
use tonic::{transport::Server, Request, Response, Status};

use logic_lib::combat::{BuffType, CombatManager, CombatStats};
use rust_mmo_gate::grpc_router::proto::gate::{
    logic_service_server::{LogicService, LogicServiceServer},
    DownstreamMessage, ForwardBatchRequest, ForwardRequest, ForwardResponse,
    PlayerOfflineRequest, PlayerOfflineResponse, PlayerOnlineRequest, PlayerOnlineResponse,
};

pub struct CombatService {
    manager: Arc<RwLock<CombatManager>>,
    entity_names: DashMap<u64, String>,
}

impl CombatService {
    fn new() -> Self {
        Self {
            manager: Arc::new(RwLock::new(CombatManager::new())),
            entity_names: DashMap::new(),
        }
    }

    /// 获取或创建实体
    fn ensure_entity(&self, uid: u64, stats: Option<CombatStats>) {
        let mgr = self.manager.read();
        if mgr.entity_stats(uid).is_none() {
            drop(mgr);
            let mut mgr = self.manager.write();
            let s = stats.unwrap_or(CombatStats {
                hp: 1000,
                max_hp: 1000,
                atk: 100,
                def: 50,
                crit_rate: 5.0,
                crit_dmg: 1.5,
                level: 10,
                xp: 0,
                alive: true,
            });
            mgr.create_entity(uid, s);
        }
    }
}

#[tonic::async_trait]
impl LogicService for CombatService {
    async fn forward_message(
        &self,
        req: Request<ForwardRequest>,
    ) -> Result<Response<ForwardResponse>, Status> {
        let r = req.into_inner();
        Ok(Response::new(self.process(r.player_uid, r.msg_id, &r.payload)))
    }

    async fn forward_message_batch(
        &self,
        req: Request<ForwardBatchRequest>,
    ) -> Result<Response<ForwardResponse>, Status> {
        let mut all = Vec::new();
        for m in req.into_inner().messages {
            all.extend(self.process(m.player_uid, m.msg_id, &m.payload).messages);
        }
        Ok(Response::new(ForwardResponse { messages: all }))
    }

    async fn player_online(
        &self,
        req: Request<PlayerOnlineRequest>,
    ) -> Result<Response<PlayerOnlineResponse>, Status> {
        let r = req.into_inner();
        println!(
            "[CombatServer] 玩家上线: uid={} gate={}",
            r.player_uid, r.gate_node
        );
        self.ensure_entity(r.player_uid, None);
        Ok(Response::new(PlayerOnlineResponse {
            ok: true,
            messages: vec![],
        }))
    }

    async fn player_offline(
        &self,
        req: Request<PlayerOfflineRequest>,
    ) -> Result<Response<PlayerOfflineResponse>, Status> {
        let r = req.into_inner();
        println!(
            "[CombatServer] 玩家离线: uid={} reason={}",
            r.player_uid, r.reason
        );
        self.entity_names.remove(&r.player_uid);
        let mut mgr = self.manager.write();
        mgr.remove_entity(r.player_uid);
        Ok(Response::new(PlayerOfflineResponse {
            ok: true,
            messages: vec![],
        }))
    }
}

impl CombatService {
    fn process(&self, uid: u64, msg_id: u32, payload: &[u8]) -> ForwardResponse {
        let json: Value = serde_json::from_slice(payload).unwrap_or(Value::Null);

        let messages = match msg_id {
            // 1001: 基础攻击 {"targetId":2001}
            1001 => self.handle_basic_attack(uid, &json),
            // 1002: 技能攻击 {"targetId":10002,"skillMult":1.5}
            1002 => self.handle_skill_attack(uid, &json),
            // 1003: AOE 攻击 {"targetIds":[8001,8002,8003],"skillMult":0.8}
            1003 => self.handle_aoe_attack(uid, &json),
            // 1004: 应用 Buff {"targetId":6001,"buffType":"AttackUp","value":50,"duration":10}
            1004 => self.handle_apply_buff(uid, &json),
            // 1005: 查询实体状态
            1005 => self.handle_query_status(uid),
            _ => vec![dm(uid, msg_id + 5000, json.to_string(), 0)],
        };

        ForwardResponse { messages }
    }

    fn handle_basic_attack(&self, uid: u64, json: &Value) -> Vec<DownstreamMessage> {
        let target_id = json
            .get("targetId")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        self.ensure_entity(uid, None);
        self.ensure_entity(target_id, None);

        let mut mgr = self.manager.write();
        let (dmg, is_crit) = mgr.attack(uid, target_id, 1.0);

        self.build_combat_result(uid, target_id, dmg, is_crit, &mgr)
    }

    fn handle_skill_attack(&self, uid: u64, json: &Value) -> Vec<DownstreamMessage> {
        let target_id = json
            .get("targetId")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let skill_mult = json.get("skillMult").and_then(|v| v.as_f64()).unwrap_or(1.0);
        self.ensure_entity(uid, None);
        self.ensure_entity(target_id, None);

        let mut mgr = self.manager.write();
        let (dmg, is_crit) = mgr.attack(uid, target_id, skill_mult);

        self.build_combat_result(uid, target_id, dmg, is_crit, &mgr)
    }

    fn handle_aoe_attack(&self, uid: u64, json: &Value) -> Vec<DownstreamMessage> {
        let target_ids: Vec<u64> = json
            .get("targetIds")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_u64()).collect())
            .unwrap_or_default();
        let skill_mult = json
            .get("skillMult")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.8);
        self.ensure_entity(uid, None);
        for &tid in &target_ids {
            self.ensure_entity(tid, None);
        }

        let mut mgr = self.manager.write();
        let results = mgr.aoe_attack(uid, &target_ids, skill_mult);

        let mut messages = Vec::new();
        for (tid, dmg) in &results {
            let result_json = serde_json::json!({
                "attackerId": uid,
                "targetId": tid,
                "damage": dmg,
                "isCrit": false,
                "isAoe": true
            })
            .to_string();
            messages.push(dm(uid, 6001, result_json.clone(), 1));
            messages.push(dm(*tid, 6001, result_json, 1));
        }

        messages
    }

    fn handle_apply_buff(&self, uid: u64, json: &Value) -> Vec<DownstreamMessage> {
        let target_id = json
            .get("targetId")
            .and_then(|v| v.as_u64())
            .unwrap_or(uid);
        let buff_type = json
            .get("buffType")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let value = json.get("value").and_then(|v| v.as_i64()).unwrap_or(0);
        let duration = json.get("duration").and_then(|v| v.as_u64()).unwrap_or(10);

        let buf = match buff_type {
            "AttackUp" => BuffType::AttackUp { value },
            "DefenseDown" => BuffType::DefenseDown { value },
            _ => {
                return vec![err(
                    uid,
                    6002,
                    &format!("未知Buff类型: {}", buff_type),
                )]
            }
        };

        self.ensure_entity(uid, None);
        let mut mgr = self.manager.write();
        mgr.buff_manager.apply(target_id, buf, duration as u32);

        let result_json =
            serde_json::json!({"success":true,"targetId":target_id,"buffType":buff_type}).to_string();
        vec![dm(uid, 6002, result_json, 1)]
    }

    fn handle_query_status(&self, uid: u64) -> Vec<DownstreamMessage> {
        let mgr = self.manager.read();
        let stats = match mgr.entity_stats(uid) {
            Some(s) => s,
            None => return vec![err(uid, 6002, "实体不存在")],
        };

        let buffs: Vec<String> = mgr
            .buff_manager
            .active_buffs(uid)
            .iter()
            .map(|b| format!("{:?}", b.buff_type))
            .collect();

        let result_json = serde_json::json!({
            "uid": uid,
            "hp": stats.hp,
            "maxHp": stats.max_hp,
            "atk": stats.atk,
            "def": stats.def,
            "critRate": stats.crit_rate,
            "critDmg": stats.crit_dmg,
            "level": stats.level,
            "xp": stats.xp,
            "alive": stats.alive,
            "buffs": buffs
        })
        .to_string();

        vec![dm(uid, 6002, result_json, 0)]
    }

    fn build_combat_result(
        &self,
        uid: u64,
        target_id: u64,
        dmg: i64,
        is_crit: bool,
        mgr: &CombatManager,
    ) -> Vec<DownstreamMessage> {
        let mut messages = Vec::new();

        let target_stats = mgr.entity_stats(target_id);
        let is_dead = target_stats.map(|s| !s.alive).unwrap_or(false);

        // 6001: 战斗结果
        let result_json = serde_json::json!({
            "attackerId": uid,
            "targetId": target_id,
            "damage": dmg,
            "isCrit": is_crit,
            "targetHpLeft": target_stats.map(|s| s.hp).unwrap_or(0),
            "isDead": is_dead
        })
        .to_string();
        messages.push(dm(uid, 6001, result_json.clone(), 2));
        messages.push(dm(target_id, 6001, result_json, 2));

        // 6002: 实体状态更新
        if let Some(t) = target_stats {
            let status_json = serde_json::json!({
                "uid": target_id,
                "hp": t.hp,
                "maxHp": t.max_hp,
                "alive": t.alive
            })
            .to_string();
            messages.push(dm(target_id, 6002, status_json, 0));
        }

        // 6003: 实体死亡/掉落
        if is_dead {
            let drop_items = vec![
                serde_json::json!({"itemName":"金币","quantity": target_stats.map(|t| t.level * 10).unwrap_or(0)}),
                serde_json::json!({"itemName":"经验药水","quantity":1}),
            ];
            let death_json = serde_json::json!({
                "entityId": target_id,
                "killerId": uid,
                "drops": drop_items
            })
            .to_string();
            messages.push(dm(uid, 6003, death_json.clone(), 2));
            messages.push(dm(0, 6003, death_json, 1)); // broadcast to all

            // 6100: 经验更新
            if let Some(attacker) = mgr.entity_stats(uid) {
                let xp_json = serde_json::json!({
                    "uid": uid,
                    "xp": attacker.xp,
                    "level": attacker.level,
                    "leveledUp": attacker.level > 0
                })
                .to_string();
                messages.push(dm(uid, 6100, xp_json, 2));
            }
        }

        // Check attacker xp too
        if let Some(atk_stats) = mgr.entity_stats(uid) {
            let status_json = serde_json::json!({
                "uid": uid,
                "hp": atk_stats.hp,
                "maxHp": atk_stats.max_hp,
                "alive": atk_stats.alive
            })
            .to_string();
            messages.push(dm(uid, 6002, status_json, 0));
        }

        messages
    }
}

fn dm(target_uid: u64, msg_id: u32, payload: String, priority: u32) -> DownstreamMessage {
    DownstreamMessage {
        target_uid,
        msg_id,
        payload: payload.into_bytes(),
        priority,
    }
}

fn err(uid: u64, msg_id: u32, error: &str) -> DownstreamMessage {
    let json = serde_json::json!({"success":false,"error":error}).to_string();
    DownstreamMessage {
        target_uid: uid,
        msg_id,
        payload: json.into_bytes(),
        priority: 2,
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr: SocketAddr = "0.0.0.0:50054".parse()?;

    println!("╔═══════════════════════════════════════════╗");
    println!("║   MMORPG 战斗服 (Combat Server)           ║");
    println!("╠═══════════════════════════════════════════╣");
    println!("║   gRPC 监听: {}                    ║", addr);
    println!("║   伤害公式: atk*mult - def*0.5            ║");
    println!("║   暴击系统: crit_rate / crit_dmg          ║");
    println!("║   Buff系统: 攻击加成 / 防御降低            ║");
    println!("╚═══════════════════════════════════════════╝");
    println!();
    println!("消息协议:");
    println!("  上行: 1001=基础攻击 1002=技能攻击 1003=AOE 1004=Buff 1005=查询");
    println!("  下行: 6001=战斗结果 6002=实体状态 6003=死亡/掉落 6100=经验更新");
    println!();

    let service = CombatService::new();
    Server::builder()
        .add_service(LogicServiceServer::new(service))
        .serve(addr)
        .await?;
    Ok(())
}
