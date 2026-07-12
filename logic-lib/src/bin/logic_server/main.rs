//! Mock 游戏逻辑服 (完整 MMORPG 版)
//!
//! 模拟真实游戏服务器的行为：
//! 1. 维护在线玩家状态（位置、HP、MP、等级、经验、背包、装备、技能、任务）
//! 2. NPC/怪物实体（巡逻、仇恨、攻击、死亡、复活、掉落）
//! 3. 物品系统（拾取、使用、装备、卸下）
//! 4. 任务系统（接取、进度、完成、奖励）
//! 5. 处理移动同步、聊天、战斗、附近玩家/NPC查询
//! 6. 玩家上线/下线时广播通知
//!
//! 运行方式: cargo run --bin logic-server
//!
//! 消息协议约定:
//!   上行 (Client -> Gateway -> LogicServer):
//!     100:  初始化/请求玩家列表
//!     101:  请求配置数据 (v0.8) — 下发 9100
//!     1001: 基础攻击        {"targetUid":12345}
//!     1002: 技能攻击        {"skillId":1,"targetUid":12345}
//!     1003: 拾取物品        {"itemId":1}
//!     1004: 装备/卸下       {"itemId":1,"slot":"weapon"}
//!     1005: 接受任务        {"questId":1}
//!     1006: 完成任务        {"questId":1}
//!     1007: NPC交互         {"npcId":1}
//!     1008: 使用物品        {"itemId":1}
//!     2001: 聊天            {"text":"hello","channel":"world"}
//!     3001: 移动            {"x":100.0,"y":200.0,"dir":0}
//!     4001: 查询附近玩家
//!     4002: 查询附近实体(NPC/怪物)
//!
//!   下行 (LogicServer -> Gateway -> Client):
//!     5001: 玩家属性        {"uid","name","hp","maxHp","mp","maxMp","level","exp","maxExp","x","y","atk","def"}
//!     5002: 经验更新        {"exp","maxExp","level"}
//!     5003: 背包更新        {"items":[...]}
//!     5004: 装备更新        {"weapon","armor","accessory"}
//!     5005: 任务更新        {"quests":[...]}
//!     5006: NPC对话         {"npcId","name","dialog","options":[...]}
//!     6001: 战斗结果        {"targetUid","dmg","targetHp","crit"}
//!     6002: 实体状态        {"entityId","hp","maxHp","state","x","y"}
//!     6003: 实体死亡/掉落   {"entityId","killer","drops":[...]}
//!     7001: 聊天ACK
//!     7002: 聊天广播
//!     8001: 玩家位置更新
//!     8002: 玩家进入
//!     8003: 玩家离开
//!     8004: 实体位置更新    {"entityId","x","y","dir"}
//!     9001: 玩家列表
//!     9002: 实体列表        {"npcs":[...],"mobs":[...]}
//!     9100: 配置数据 (v0.8) {skills,mobs,items,quests,classes,talents,npcs,maps,shopItems}

mod constants;
mod types;
mod utils;
mod state;
mod handlers;
mod combat;
mod inventory;
mod quest;
mod world;
mod event_bus;
mod codec;
pub mod config_loader;
#[cfg(test)]
mod tests;

use constants::*;
use types::*;
use utils::*;
use state::*;

use logic_lib::game_proto as gp;

use std::net::SocketAddr;
use std::sync::Arc;

use tonic::{transport::Server, Request, Response, Status};

use rust_mmo_gate::grpc_router::proto::gate::{
    logic_service_server::{LogicService, LogicServiceServer},
    ForwardBatchRequest, ForwardRequest, ForwardResponse,
    PlayerOfflineRequest, PlayerOfflineResponse, PlayerOnlineRequest, PlayerOnlineResponse,
};

#[tonic::async_trait]
impl LogicService for MockLogicService {
    async fn forward_message(
        &self,
        request: Request<ForwardRequest>,
    ) -> Result<Response<ForwardResponse>, Status> {
        let req = request.into_inner();
        let state = self.state.clone();
        // 关键：用 spawn_blocking 包裹同步 process_message，避免阻塞 tokio worker
        let response = tokio::task::spawn_blocking(move || {
            state.process_message(req.player_uid, req.msg_id, &req.payload)
        })
        .await
        .map_err(|e| Status::internal(format!("spawn_blocking: {}", e)))?;
        Ok(Response::new(response))
    }

    async fn forward_message_batch(
        &self,
        request: Request<ForwardBatchRequest>,
    ) -> Result<Response<ForwardResponse>, Status> {
        let req = request.into_inner();
        let state = self.state.clone();
        // 关键：用 spawn_blocking 包裹同步批处理，避免阻塞 tokio worker
        let response = tokio::task::spawn_blocking(move || {
            let mut all_downstream = Vec::new();
            for msg in req.messages {
                let resp = state.process_message(msg.player_uid, msg.msg_id, &msg.payload);
                all_downstream.extend(resp.messages);
            }
            ForwardResponse { messages: all_downstream }
        })
        .await
        .map_err(|e| Status::internal(format!("spawn_blocking: {}", e)))?;
        Ok(Response::new(response))
    }

    async fn player_online(
        &self,
        request: Request<PlayerOnlineRequest>,
    ) -> Result<Response<PlayerOnlineResponse>, Status> {
        let req = request.into_inner();
        let uid = req.player_uid;
        let count = self.state.online_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;

        // 尝试从 DB 加载已有角色
        let (name, mut player) = if let Some(ref db) = self.db {
            if let Ok(Some(data)) = db.load_player(uid).await {
                let name = data["name"].as_str().unwrap_or("Player").to_string();
                let mut p = PlayerState::new(uid, name.clone());
                p.level = data["level"].as_i64().unwrap_or(1) as u32;
                p.exp = data["exp"].as_u64().unwrap_or(0) as u32;
                p.hp = data["hp"].as_i64().unwrap_or(100) as i32;
                p.max_hp = data["maxHp"].as_i64().unwrap_or(100) as i32;
                p.mp = data["mp"].as_i64().unwrap_or(50) as i32;
                p.max_mp = data["maxMp"].as_i64().unwrap_or(50) as i32;
                p.atk = data["atk"].as_i64().unwrap_or(20) as i32;
                p.def = data["def"].as_i64().unwrap_or(5) as i32;
                p.weapon = data["weapon"].as_u64().map(|v| v as u32).or(data["weapon"].as_i64().map(|v| v as u32));
                p.armor = data["armor"].as_u64().map(|v| v as u32).or(data["armor"].as_i64().map(|v| v as u32));
                p.accessory = data["accessory"].as_u64().map(|v| v as u32).or(data["accessory"].as_i64().map(|v| v as u32));
                p.weapon_enhance = data["weaponEnhance"].as_u64().unwrap_or(0) as u32;
                p.armor_enhance = data["armorEnhance"].as_u64().unwrap_or(0) as u32;
                p.accessory_enhance = data["accessoryEnhance"].as_u64().unwrap_or(0) as u32;
                if let Some(items) = data["inventory"].as_array() {
                    p.inventory = items.iter().map(|i| (i["itemId"].as_u64().unwrap_or(0) as u32, i["count"].as_u64().unwrap_or(1) as u32)).collect();
                }
                if let Some(qs) = data["quests"].as_array() {
                    p.quests = qs.iter().map(|q| (q["questId"].as_u64().unwrap_or(0) as u32, q["progress"].as_u64().unwrap_or(0) as u32)).collect();
                }
                tracing::info!(uid, name = %name, level = p.level, "从 DB 加载角色");
                (name, p)
            } else {
                let name = format!("Player{}", uid);
                let p = PlayerState::new(uid, name.clone());
                (name, p)
            }
        } else {
            let name = format!("Player{}", uid);
            let p = PlayerState::new(uid, name.clone());
            (name, p)
        };

        tracing::info!(
            uid,
            session = req.session_id,
            gate = %req.gate_node,
            online = count,
            "玩家上线"
        );

        let mut messages = Vec::new();

        // 1. 给自己发玩家属性 (5001)
        messages.push(codec::dm_proto(uid, 5001, &player.to_player_stats(), 1));

        // 2. 广播玩家进入通知 (8002)
        messages.push(dm(0, 8002, player.to_enter_json(), 1));

        // 3. 给自己发所有已在线玩家的进入通知 (8002)
        for entry in self.state.players.iter() {
            if entry.uid != uid {
                messages.push(dm(uid, 8002, entry.to_enter_json(), 1));
            }
        }

        // 4. 给自己发玩家列表 (9001)
        let list: Vec<String> = self.state.players
            .iter()
            .filter(|e| e.uid != uid)
            .map(|e| e.to_list_entry())
            .collect();
        let list_json = serde_json::json!({ "players": list }).to_string();
        messages.push(dm(uid, 9001, list_json, 0));

        // 5. 给自己发背包 (5003)
        messages.push(dm(uid, 5003, player.to_inventory_json(), 1));

        // 6. 给自己发装备 (5004)
        messages.push(codec::dm_proto(uid, 5004, &player.to_equipment_proto(), 1));

        // 7. 技能数据 (通过 5004 下发，不再占用 5500 避免与升级消息冲突)
        // 5004 上行是装备，下行复用为技能列表
        // messages.push(dm(uid, 5004, player.to_skills_json(), 1));

        // 8. 给自己发任务列表 (5005)
        messages.push(codec::dm_proto(uid, 5005, &player.to_quests_proto(), 1));

        // 9. 给自己发NPC和怪物列表 (9002, proto 编码)
        let entity_list = gp::EntityList {
            npcs: self.state.npcs.iter().map(|n| n.to_entity_list_entry()).collect(),
            mobs: self.state.mobs.iter().map(|m| m.to_entity_list_entry()).collect(),
        };
        messages.push(codec::dm_proto(uid, 9002, &entity_list, 0));

        self.state.players.insert(uid, player);

        Ok(Response::new(PlayerOnlineResponse {
            ok: true,
            messages,
        }))
    }

    async fn player_offline(
        &self,
        request: Request<PlayerOfflineRequest>,
    ) -> Result<Response<PlayerOfflineResponse>, Status> {
        let req = request.into_inner();
        let uid = req.player_uid;
        let count = self.state.online_count
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed)
            .saturating_sub(1);

        tracing::info!(
            uid,
            session = req.session_id,
            reason = %req.reason,
            online = count,
            "玩家离线"
        );

        // 保存到 DB
        if let (Some(ref db), Some(p)) = (&self.db, self.state.players.get(&uid)) {
            let data = serde_json::json!({
                "uid": p.uid, "name": p.name, "level": p.level, "exp": p.exp,
                "x": p.x, "y": p.y, "hp": p.hp, "maxHp": p.max_hp,
                "mp": p.mp, "maxMp": p.max_mp, "atk": p.atk, "def": p.def,
                "weapon": p.weapon, "armor": p.armor, "accessory": p.accessory,
                "weaponEnhance": p.weapon_enhance, "armorEnhance": p.armor_enhance, "accessoryEnhance": p.accessory_enhance,
                "inventory": p.inventory.iter().map(|(id,c)| serde_json::json!({"itemId":id,"count":c})).collect::<Vec<_>>(),
                "quests": p.quests.iter().map(|(qid,pr)| serde_json::json!({"questId":qid,"progress":pr})).collect::<Vec<_>>(),
            });
            let _ = db.save_player(uid, &data).await;
        }

        self.state.players.remove(&uid);

        // 清除以该玩家为目标的怪物仇恨
        // 修复锁竞争：先收集ID，逐个获取写锁，避免持有全部分片锁
        let mob_ids: Vec<u64> = self.state.mobs.iter()
            .filter(|m| m.target_uid == Some(uid))
            .map(|m| m.entity_id)
            .collect();
        for eid in mob_ids {
            if let Some(mut mob) = self.state.mobs.get_mut(&eid) {
                if mob.target_uid == Some(uid) {
                    mob.target_uid = None;
                    mob.state = MobState::Idle;
                }
            }
        }

        let leave_json = serde_json::json!({ "uid": uid }).to_string();

        Ok(Response::new(PlayerOfflineResponse {
            ok: true,
            messages: vec![dm(0, 8003, leave_json, 1)],
        }))
    }
}

// ════════════════════════════════════════════════════════════════
// Main
// ════════════════════════════════════════════════════════════════

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 初始化 tracing 日志
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| {
                    tracing_subscriber::EnvFilter::new(
                        "info,rust_mmo_gate=info,logic_server=info",
                    )
                }),
        )
        .with_target(true)
        .with_file(true)
        .with_line_number(true)
        .init();

    let addr: SocketAddr = "0.0.0.0:50051".parse()?;

    tracing::info!(
        addr = %addr,
        world = %format!("{}x{}", WORLD_W, WORLD_H),
        npcs = NPC_DEFS.len(),
        mob_kinds = MOB_DEFS.len(),
        mob_instances = MOB_DEFS.len() * 3,
        items = ITEM_DEFS.len(),
        quests = QUEST_DEFS.len(),
        skills = SKILLS.len(),
        "Mock MMORPG 逻辑服 (完整版) 启动中"
    );

    // 预加载配置（从 config/ 目录读取 JSON，缺失则用 const fallback）
    let _ = config_loader::get_config();

    let mut service = MockLogicService::default();

    // 尝试连接 PostgreSQL（不可用则降级，游戏仍正常运行）
    match logic_lib::db::Database::new("postgres://mmo:mmo_dev_pass@127.0.0.1:5433/mmorpg").await {
        Ok(db) => {
            tracing::info!("PostgreSQL 已连接");
            let db = Arc::new(db);
            service.db = Some(db.clone());

            // 后台自动存盘 — 每 30 秒
            let state = service.state.clone();
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
                    for player in state.players.iter() {
                        let data = serde_json::json!({
                            "uid": player.uid,
                            "name": player.name,
                            "level": player.level,
                            "exp": player.exp,
                            "x": player.x, "y": player.y,
                            "hp": player.hp, "maxHp": player.max_hp,
                            "mp": player.mp, "maxMp": player.max_mp,
                            "atk": player.atk, "def": player.def,
                            "weapon": player.weapon, "armor": player.armor, "accessory": player.accessory,
                            "inventory": player.inventory.iter().map(|(id,c)| serde_json::json!({"itemId":id,"count":c})).collect::<Vec<_>>(),
                            "quests": player.quests.iter().map(|(qid,p)| serde_json::json!({"questId":qid,"progress":p})).collect::<Vec<_>>(),
                        });
                        let _ = db.save_player(player.uid, &data).await;
                    }
                }
            });
        }
        Err(e) => {
            tracing::info!(error = %e, "PostgreSQL 不可用, 数据仅存内存");
        }
    }

    Server::builder()
        .add_service(LogicServiceServer::new(service))
        .serve(addr)
        .await?;

    tracing::info!("逻辑服已停止");
    Ok(())
}
